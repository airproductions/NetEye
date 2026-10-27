//! Globe view (Traffic chart mode 2): a dotted 3D Earth that lights up the countries your traffic
//! goes to, with arcs from your own country and hotspots you can hover or click.
//!
//! Land comes from `assets/geo/globe-dots.bin` (Natural Earth, public domain; see
//! tools/globe-dots). Drawing is plain egui shapes: an orthographic projection of a rotated
//! sphere, with the dots batched into one mesh.

use std::collections::HashMap;
use std::sync::OnceLock;

use eframe::egui::{self, Align2, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

use super::host_popup::{self, Detail};
use super::theme::{self, c, Icons};
use super::traffic_data::Agg;
use crate::geo;
use crate::monitor::Shared;

type V3 = [f32; 3];

struct Data {
    /// (ISO-2 code, unit vector of its label point)
    countries: Vec<(String, V3)>,
    /// (unit vector, index into `countries`)
    dots: Vec<(V3, u8)>,
}

/// Unit vector for a latitude/longitude in degrees: y up, z toward longitude 0 on the equator.
pub fn unit(lat: f32, lon: f32) -> V3 {
    let (la, lo) = (lat.to_radians(), lon.to_radians());
    [la.cos() * lo.sin(), la.sin(), la.cos() * lo.cos()]
}

/// Turns the globe: `yaw` about the vertical axis, then `pitch` about the horizontal one.
pub fn rotate(v: V3, yaw: f32, pitch: f32) -> V3 {
    let (sy, cy) = yaw.sin_cos();
    let x = v[0] * cy + v[2] * sy;
    let z = -v[0] * sy + v[2] * cy;
    let (sp, cp) = pitch.sin_cos();
    [x, v[1] * cp - z * sp, v[1] * sp + z * cp]
}

/// Points along the great circle from `a` to `b`, lifted off the surface toward the middle
/// (more for longer hops) so arcs read as flights rather than painted lines.
pub fn arc(a: V3, b: V3, n: usize) -> Vec<V3> {
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    let ang = dot.acos();
    let lift = 0.04 + 0.22 * ang / std::f32::consts::PI;
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let (wa, wb) = if ang < 1e-4 { (1.0 - t, t) } else { (((1.0 - t) * ang).sin() / ang.sin(), (t * ang).sin() / ang.sin()) };
            let h = 1.0 + lift * (std::f32::consts::PI * t).sin();
            [(a[0] * wa + b[0] * wb) * h, (a[1] * wa + b[1] * wb) * h, (a[2] * wa + b[2] * wb) * h]
        })
        .collect()
}

/// A rotated point is hidden when it is behind the sphere (lifted arc points can still show
/// past the edge).
fn hidden(p: V3) -> bool {
    p[2] < 0.0 && p[0] * p[0] + p[1] * p[1] < 1.0
}

fn parse(b: &[u8]) -> Option<Data> {
    if b.get(..8)? != b"NEDOTS1\0" {
        return None;
    }
    let u16_at = |o: usize| Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?));
    let f32_at = |o: usize| Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?));
    let n = u16_at(8)? as usize;
    let mut o = 10;
    let mut countries = Vec::with_capacity(n);
    for _ in 0..n {
        let code = std::str::from_utf8(b.get(o..o + 2)?).ok()?.to_string();
        countries.push((code, unit(f32_at(o + 2)?, f32_at(o + 6)?)));
        o += 10;
    }
    let m = u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize;
    o += 4;
    let mut dots = Vec::with_capacity(m);
    for _ in 0..m {
        let la = i16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as f32 / 100.0;
        let lo = i16::from_le_bytes(b.get(o + 2..o + 4)?.try_into().ok()?) as f32 / 100.0;
        dots.push((unit(la, lo), *b.get(o + 4)?));
        o += 5;
    }
    Some(Data { countries, dots })
}

fn data() -> &'static Data {
    static D: OnceLock<Data> = OnceLock::new();
    D.get_or_init(|| parse(include_bytes!("../../assets/geo/globe-dots.bin")).unwrap_or(Data { countries: Vec::new(), dots: Vec::new() }))
}

/// The country Windows is set to (Settings → Time & language → Region), e.g. "IL".
fn home_country() -> Option<String> {
    static HOME: OnceLock<Option<String>> = OnceLock::new();
    HOME.get_or_init(|| {
        let mut buf = [0u16; 16];
        // SAFETY: the buffer length is passed; the call writes a NUL-terminated name into it.
        let n = unsafe { windows_sys::Win32::Globalization::GetUserDefaultGeoName(buf.as_mut_ptr(), buf.len() as i32) };
        let s = String::from_utf16_lossy(&buf[..(n.max(1) as usize - 1).min(buf.len())]);
        (s.len() == 2 && s.chars().all(|c| c.is_ascii_uppercase())).then_some(s)
    })
    .clone()
}

/// What one country shows: traffic in the range (or live connections) and how many hosts.
#[derive(Clone, Copy, Default)]
struct Spot {
    down: u64,
    up: u64,
    count: usize,
}

pub struct GlobeView {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    /// ctx time of the last drag or zoom; the globe spins by itself a few seconds after.
    touched: f64,
    started: bool,
    pub live: bool,
}

impl Default for GlobeView {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.35, zoom: 1.0, touched: f64::NEG_INFINITY, started: false, live: false }
    }
}

impl GlobeView {
    pub fn show(&mut self, ui: &mut Ui, s: &mut Shared, icons: &mut Icons, agg: &Agg, rect: Rect) {
        let d = data();
        let now = ui.input(|i| i.time);
        let index: HashMap<&str, usize> = d.countries.iter().enumerate().map(|(i, (c, _))| (c.as_str(), i)).collect();
        let home = home_country();
        if !self.started {
            // start over the user's own part of the world
            if let Some(v) = home.as_deref().and_then(|h| index.get(h)).map(|&i| d.countries[i].1) {
                self.yaw = -v[0].atan2(v[2]);
                self.pitch = v[1].asin() * 0.8;
            }
            self.started = true;
        }

        // input: drag turns, scroll zooms, idle spins slowly
        let resp = ui.interact(rect, ui.id().with("globe"), Sense::click_and_drag());
        if resp.dragged() {
            let dv = resp.drag_delta();
            self.yaw += dv.x * 0.006 / self.zoom;
            self.pitch = (self.pitch + dv.y * 0.006 / self.zoom).clamp(-1.3, 1.3);
            self.touched = now;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.7, 3.5);
                self.touched = now;
            }
        }
        if now - self.touched > 4.0 {
            self.yaw += ui.input(|i| i.stable_dt).min(0.1) * 0.06;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));

        // what each country shows
        let mut spots: HashMap<String, Spot> = HashMap::new();
        if self.live {
            for l in s.apps_live.values() {
                for cv in &l.conns {
                    let cc = geo::country_of(cv.remote);
                    if cc.len() == 2 && cc != geo::LAN && cc != geo::UNKNOWN {
                        let e = spots.entry(cc).or_default();
                        e.count += 1;
                        e.down += cv.rate[0];
                        e.up += cv.rate[1];
                    }
                }
            }
        } else {
            for r in &agg.countries {
                if let Some(cc) = &r.cc {
                    spots.insert(cc.clone(), Spot { down: r.down, up: r.up, count: 0 });
                }
            }
            for h in &agg.hosts {
                if let Some(e) = h.cc.as_ref().and_then(|cc| spots.get_mut(cc)) {
                    e.count += 1;
                }
            }
        }
        let weight = |sp: &Spot| if self.live { sp.count as f64 } else { (sp.down + sp.up) as f64 };
        let max = spots.values().map(weight).fold(0.0, f64::max).max(1.0);
        let heat: Vec<f32> = d.countries.iter().map(|(cc, _)| spots.get(cc).map_or(0.0, |sp| (weight(sp) / max).sqrt() as f32)).collect();

        let center = rect.center();
        let radius = rect.width().min(rect.height()) * 0.42 * self.zoom;
        let proj = |p: V3| Pos2::new(center.x + p[0] * radius, center.y - p[1] * radius);
        let p = ui.painter_at(rect);

        // atmosphere and ocean
        for (k, a) in [(1.10, 10u8), (1.06, 18), (1.03, 28)] {
            p.circle_filled(center, radius * k, theme::with_alpha(c::accent(), a));
        }
        p.circle_filled(center, radius, theme::lerp_color(c::bg(), c::raised(), 0.55));
        p.circle_filled(center + Vec2::new(-radius * 0.18, -radius * 0.2), radius * 0.72, theme::with_alpha(Color32::WHITE, 4));
        p.circle_stroke(center, radius, Stroke::new(1.0_f32, theme::with_alpha(c::accent(), 60)));

        // land dots in one mesh; nearer dots are larger and brighter, lit countries glow
        let base = theme::lerp_color(c::faint(), c::muted(), 0.4);
        let lit = c::down();
        let size = (radius / 210.0).clamp(0.8, 3.2);
        let mut mesh = Mesh::default();
        for (v, ci) in &d.dots {
            let q = rotate(*v, self.yaw, self.pitch);
            if q[2] <= 0.02 {
                continue;
            }
            let h = heat.get(usize::from(*ci)).copied().unwrap_or(0.0);
            // any traffic at all lights a country clearly; more traffic, brighter
            let col = if h > 0.0 { theme::lerp_color(base, lit, 0.5 + 0.5 * h) } else { base };
            let col = theme::with_alpha(col, (90.0 + 165.0 * q[2]) as u8);
            let half = size * (0.55 + 0.45 * q[2]) * if h > 0.0 { 1.25 } else { 1.0 };
            mesh.add_colored_rect(Rect::from_center_size(proj(q), Vec2::splat(half * 2.0)), col);
        }
        p.add(Shape::mesh(mesh));

        // arcs from home with particles flowing out (download colour) and back (upload colour)
        let home_v = home.as_deref().and_then(|h| index.get(h)).map(|&i| d.countries[i].1);
        let mut ranked: Vec<(&String, &Spot)> = spots.iter().filter(|(cc, _)| Some(cc.as_str()) != home.as_deref()).collect();
        ranked.sort_by(|a, b| weight(b.1).total_cmp(&weight(a.1)));
        ranked.truncate(25);
        if let Some(hv) = home_v {
            for (n, (cc, sp)) in ranked.iter().enumerate() {
                let Some(&i) = index.get(cc.as_str()) else { continue };
                let pts: Vec<V3> = arc(hv, d.countries[i].1, 48).into_iter().map(|v| rotate(v, self.yaw, self.pitch)).collect();
                let w = 0.8 + 1.8 * (weight(sp) / max).sqrt() as f32;
                for seg in pts.windows(2) {
                    if hidden(seg[0]) || hidden(seg[1]) {
                        continue;
                    }
                    p.line_segment([proj(seg[0]), proj(seg[1])], Stroke::new(w, theme::with_alpha(c::down(), 150)));
                }
                for (k, col, back) in [(0.0, c::down(), false), (0.5, c::up(), true)] {
                    let t = ((now * 0.25 + n as f64 * 0.137 + k) % 1.0) as f32;
                    let t = if back { 1.0 - t } else { t };
                    let q = pts[((t * 48.0) as usize).min(48)];
                    if !hidden(q) {
                        p.circle_filled(proj(q), 1.5 + w * 0.6, col);
                    }
                }
            }
            let hq = rotate(hv, self.yaw, self.pitch);
            if !hidden(hq) {
                let pulse = ((now * 2.0).sin() * 0.5 + 0.5) as f32;
                p.circle_stroke(proj(hq), 6.0 + 6.0 * pulse, Stroke::new(1.5_f32, theme::with_alpha(Color32::WHITE, (160.0 * (1.0 - pulse)) as u8)));
                p.circle_filled(proj(hq), 4.5, Color32::WHITE);
            }
        }

        // hotspots: count badges on the busiest countries, names on the top few
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut hovered: Option<(String, Pos2)> = None;
        for (rank, (cc, sp)) in ranked.iter().enumerate() {
            let Some(&i) = index.get(cc.as_str()) else { continue };
            let q = rotate(d.countries[i].1, self.yaw, self.pitch);
            if q[2] <= 0.05 {
                continue;
            }
            let at = proj(q);
            let label = if self.live || sp.count > 0 { sp.count.to_string() } else { String::new() };
            let r = if label.len() > 2 { 12.0 } else { 10.0 };
            p.circle_filled(at, r, Color32::WHITE);
            p.circle_stroke(at, r, Stroke::new(1.5_f32, c::down()));
            if !label.is_empty() {
                p.text(at, Align2::CENTER_CENTER, &label, theme::bold(11.0), Color32::from_rgb(0x14, 0x16, 0x1b));
            }
            if rank < 8 {
                p.text(at + Vec2::new(0.0, r + 9.0), Align2::CENTER_CENTER, geo::name(cc), theme::semibold(12.0), c::text());
            }
            if pointer.is_some_and(|m| m.distance(at) <= r + 4.0) {
                hovered = Some(((*cc).clone(), at));
            }
        }
        if let Some((cc, at)) = &hovered {
            let sp = spots[cc];
            let detail = if self.live {
                format!("{} connection{} · ↓ {}  ↑ {}", sp.count, if sp.count == 1 { "" } else { "s" }, theme::rate(sp.down as f64), theme::rate(sp.up as f64))
            } else {
                format!("{} host{} · ↓ {}  ↑ {}", sp.count, if sp.count == 1 { "" } else { "s" }, theme::bytes(sp.down), theme::bytes(sp.up))
            };
            resp.clone().on_hover_ui_at_pointer(|ui| {
                theme::label(ui, geo::name(cc), theme::semibold(13.5), c::text());
                theme::label(ui, detail, theme::regular(12.5), c::muted());
                if !self.live {
                    theme::label(ui, "Click for its hosts", theme::regular(12.0), c::faint());
                }
            });
            if resp.clicked() && !self.live {
                host_popup::open(ui.ctx(), Detail::Country(cc.clone()), *at);
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }

        if spots.is_empty() {
            let msg = if self.live { "No internet connections right now." } else if s.admin { "No traffic to other countries in this period." } else { "Countries need administrator mode." };
            p.text(Pos2::new(center.x, rect.bottom() - 60.0), Align2::CENTER_CENTER, msg, theme::regular(13.5), c::muted());
        }

        // controls: Live Connections (bottom left), zoom (bottom right)
        let bl = Rect::from_min_size(Pos2::new(rect.left() + 8.0, rect.bottom() - 44.0), Vec2::new(220.0, 36.0));
        let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(bl).layout(egui::Layout::left_to_right(egui::Align::Center)));
        theme::label(&mut lui, "Live connections", theme::semibold(13.0), c::text());
        lui.add_space(8.0);
        if theme::switch(&mut lui, self.live).on_hover_text("Show where your apps are connected right now instead of the chosen period").clicked() {
            self.live = !self.live;
        }
        let br = Rect::from_min_size(Pos2::new(rect.right() - 76.0, rect.bottom() - 44.0), Vec2::new(70.0, 36.0));
        let mut zui = ui.new_child(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if theme::pill(&mut zui, "−", None, false).on_hover_text("Zoom out").clicked() {
            self.zoom = (self.zoom / 1.25).max(0.7);
            self.touched = now;
        }
        if theme::pill(&mut zui, "+", None, false).on_hover_text("Zoom in").clicked() {
            self.zoom = (self.zoom * 1.25).min(3.5);
            self.touched = now;
        }

        host_popup::show(ui, agg, s, icons);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V3, b: V3) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn unit_vectors_point_where_expected() {
        assert!(close(unit(0.0, 0.0), [0.0, 0.0, 1.0]));
        assert!(close(unit(90.0, 0.0), [0.0, 1.0, 0.0]));
        assert!(close(unit(0.0, 90.0), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn yaw_brings_a_longitude_to_the_front() {
        // turning by -lon faces that longitude toward the viewer (+z)
        let v = unit(0.0, 35.0);
        let q = rotate(v, -35f32.to_radians(), 0.0);
        assert!(close(q, [0.0, 0.0, 1.0]));
    }

    #[test]
    fn arcs_start_and_end_on_the_surface_and_rise_between() {
        let a = unit(32.0, 35.0);
        let b = unit(38.0, -97.0);
        let pts = arc(a, b, 20);
        let len = |v: V3| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len(pts[0]) - 1.0).abs() < 1e-4 && (len(pts[20]) - 1.0).abs() < 1e-4);
        assert!(len(pts[10]) > 1.05);
    }

    #[test]
    fn bundled_dots_parse() {
        let d = data();
        assert!(d.countries.len() > 150);
        assert!(d.dots.len() > 5000);
        assert!(d.countries.iter().any(|(c, _)| c == "IL") && d.countries.iter().any(|(c, _)| c == "SG"));
        assert!(d.dots.iter().all(|(_, ci)| usize::from(*ci) < d.countries.len()));
    }
}
