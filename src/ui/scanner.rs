//! Network scanner: devices on the local network found by ARP sweeps.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::motion;
use super::theme::{self, c, Glyph};
use super::Action;
use crate::monitor::Shared;
use crate::store::DeviceRecord;

const ROW_H: f32 = 64.0;

#[derive(Default)]
pub struct ScannerPage {
    search: String,
    /// (mac, text) of the device being renamed.
    editing: Option<(String, String)>,
}

fn kind(d: &DeviceRecord, gateway: Option<&str>, local: Option<&str>) -> (Glyph, &'static str) {
    let vendor = d.vendor.clone().unwrap_or_default().to_lowercase();
    let name = d.name.clone().unwrap_or_default().to_lowercase();
    if Some(d.ip.as_str()) == gateway {
        return (Glyph::Router, "Router");
    }
    if Some(d.ip.as_str()) == local {
        return (Glyph::Computer, "This computer");
    }
    let has = |words: &[&str]| words.iter().any(|w| vendor.contains(w) || name.contains(w));
    if has(&["iphone", "android", "galaxy", "pixel", "phone"]) || vendor.contains("randomized") {
        (Glyph::Phone, "Phone")
    } else if has(&["tv", "roku", "chromecast", "streamer", "sonos", "partnertv", "fire", "sei robotics"]) {
        (Glyph::Tv, "Media")
    } else if has(&["access point", "router", "tp-link", "netgear", "ubiquiti", "eero", "arris", "vantiva", "technicolor", "sagemcom", "mikrotik"]) {
        (Glyph::Router, "Network")
    } else if has(&["desktop", "laptop", "intel", "dell", "lenovo", "hewlett", "gigabyte", "asustek", "micro-star", "apple"]) {
        (Glyph::Computer, "Computer")
    } else {
        (Glyph::Device, "Device")
    }
}

/// Primary action with a radar glyph; same look as `theme::primary_button`.
fn scan_button(ui: &mut egui::Ui) -> egui::Response {
    let galley = ui.painter().layout_no_wrap("Scan".to_string(), theme::semibold(13.5), Color32::WHITE);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 28.0 + 22.0, 32.0), Sense::click());
    let t = motion::hover(ui, resp.id, resp.hovered());
    theme::glow(ui.painter(), r, 10, t);
    ui.painter().rect_filled(r, CornerRadius::same(10), theme::lerp_color(c::accent(), c::accent2(), t * 0.35));
    theme::glyph(ui.painter(), Rect::from_center_size(Pos2::new(r.left() + 22.0, r.center().y), Vec2::splat(16.0)), Glyph::Radar, Color32::WHITE);
    ui.painter().galley(Pos2::new(r.left() + 36.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
    resp
}

impl ScannerPage {
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, actions: &mut Vec<Action>) {
        let full = ui.max_rect();
        let (bar, _) = ui.allocate_exact_size(Vec2::new(full.width(), 64.0), Sense::hover());
        let tool = bar.shrink2(Vec2::new(16.0, 4.0));
        theme::glass(ui.painter(), tool, 14);
        let mut b = ui.new_child(egui::UiBuilder::new().max_rect(tool.shrink2(Vec2::new(16.0, 8.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let adapter = s.adapters.first().map(|a| format!("{}  ·  {}", a.kind, a.description)).unwrap_or_else(|| "No active adapter".into());
        theme::pill(&mut b, &adapter, Some(Glyph::Scanner), false);
        b.add_space(6.0);
        if s.scanning {
            b.add(egui::Spinner::new().size(18.0).color(c::accent()));
            theme::label(&mut b, "Scanning your network…", theme::regular(13.5), c::muted());
        } else {
            if scan_button(&mut b).clicked() {
                actions.push(Action::Scan);
                s.scanning = true;
            }
            if let Some(t) = s.store.data.last_scan {
                theme::label(&mut b, format!("Scanned: {}", theme::date_time(t)), theme::regular(13.5), c::muted());
            }
        }
        b.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let auto = s.store.data.settings.auto_scan;
            if theme::switch(ui, auto).on_hover_text("Rescan every 5 minutes and alert on new devices").clicked() {
                s.store.data.settings.auto_scan = !auto;
                s.store.mark_dirty();
            }
            theme::label(ui, "Auto-scan", theme::semibold(13.5), c::text());
            ui.add_space(14.0);
            theme::search_box(ui, &mut self.search, 300.0, "Search devices");
        });

        // Table card: header and rows share one glass surface.
        let card = Rect::from_min_max(Pos2::new(full.left() + 16.0, bar.bottom() + 4.0), Pos2::new(full.right() - 16.0, full.bottom() - 16.0));
        theme::glass(ui.painter(), card, 14);
        let x0 = card.left();
        let w = card.width();
        let col = |f: f32| x0 + w * f;
        let cols = [col(0.0) + 14.0, col(0.04), col(0.105), col(0.27), col(0.46), col(0.56), col(0.69), col(0.85)];
        let headers = ["", "Type", "Name", "Description", "IP", "MAC address", "Last seen", "First seen"];
        let hr = Rect::from_min_size(card.min, Vec2::new(card.width(), 36.0));
        let p = ui.painter();
        for (x, h) in cols.iter().zip(headers) {
            p.text(Pos2::new(*x, hr.center().y), Align2::LEFT_CENTER, h, theme::semibold(12.0), c::muted());
        }
        p.circle_filled(Pos2::new(cols[0] + 6.0, hr.center().y), 4.0, c::good());
        p.line_segment([Pos2::new(hr.left() + 12.0, hr.bottom()), Pos2::new(hr.right() - 12.0, hr.bottom())], Stroke::new(1.0_f32, c::border()));

        let q = self.search.to_lowercase();
        let mut devices: Vec<DeviceRecord> = s
            .store
            .data
            .devices
            .values()
            .filter(|d| q.is_empty() || format!("{} {} {} {} {}", d.label.clone().unwrap_or_default(), d.name.clone().unwrap_or_default(), d.vendor.clone().unwrap_or_default(), d.ip, d.mac).to_lowercase().contains(&q))
            .cloned()
            .collect();
        devices.sort_by_key(|d| (!s.online.contains(&d.mac), d.ip.split('.').map(|o| o.parse::<u32>().unwrap_or(0)).fold(0u32, |a, o| a * 256 + o)));

        if devices.is_empty() {
            let p = ui.painter_at(card);
            theme::empty_state(&p, card.center(), if s.scanning { "Looking for devices…" } else { "No devices found yet. Press Scan." });
            return;
        }
        let gw = s.gateway.clone();
        let me = s.local_ip.clone();
        let body = Rect::from_min_max(Pos2::new(card.left() + 6.0, hr.bottom() + 4.0), Pos2::new(card.right() - 6.0, card.bottom() - 6.0));
        let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(&mut bui, ROW_H, devices.len(), |ui, range| {
            for i in range {
                let d = &devices[i];
                let (r, resp) = ui.allocate_exact_size(Vec2::new(body.width(), ROW_H), Sense::hover());
                let t = motion::hover(ui, resp.id, resp.hovered());
                let p = ui.painter();
                let base = if i % 2 == 0 { c::row_alt() } else { Color32::TRANSPARENT };
                p.rect_filled(r, CornerRadius::same(10), theme::lerp_color(base, c::hover(), t));
                let y = r.center().y;
                let online = s.online.contains(&d.mac);
                if online {
                    p.circle_filled(Pos2::new(cols[0] + 6.0, y), 4.0, c::good());
                } else {
                    p.circle_stroke(Pos2::new(cols[0] + 6.0, y), 3.5, Stroke::new(1.2_f32, c::faint()));
                }
                let (g, kname) = kind(d, gw.as_deref(), me.as_deref());
                let ir = Rect::from_center_size(Pos2::new(cols[1] + 16.0, y), Vec2::splat(30.0));
                p.circle_stroke(ir.center(), 17.0, Stroke::new(1.0_f32, c::border()));
                theme::glyph(p, ir.shrink(5.0), g, if online { c::text() } else { c::muted() });
                ui.interact(ir, ui.id().with(("kind", i)), Sense::hover()).on_hover_text(kname);
                let tc = if online { c::text() } else { c::muted() };
                let name = d.label.clone().or(d.name.clone()).unwrap_or_else(|| kname.to_string());
                let nr = Rect::from_min_max(Pos2::new(cols[2] - 4.0, y - 14.0), Pos2::new(cols[3] - 12.0, y + 14.0));
                match self.editing.as_mut().filter(|(mac, _)| *mac == d.mac) {
                    Some((mac, text)) => {
                        let r = ui.put(nr, egui::TextEdit::singleline(text).font(theme::semibold(14.0)).hint_text("Device name"));
                        r.request_focus();
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.editing = None;
                        } else if r.lost_focus() {
                            let label = text.trim().to_string();
                            if let Some(dev) = s.store.data.devices.get_mut(mac.as_str()) {
                                dev.label = (!label.is_empty()).then_some(label);
                                s.store.mark_dirty();
                            }
                            self.editing = None;
                        }
                    }
                    None => {
                        let p = ui.painter();
                        theme::text(p, Pos2::new(cols[2], y), &name, theme::semibold(14.0), tc, cols[3] - cols[2] - 12.0);
                        let tip = match &d.name {
                            Some(n) if d.label.is_some() => format!("Double-click to rename (discovered as {n})"),
                            _ => "Double-click to rename".to_string(),
                        };
                        if ui.interact(nr, ui.id().with(("name", i)), Sense::click()).on_hover_text(tip).double_clicked() {
                            self.editing = Some((d.mac.clone(), d.label.clone().or(d.name.clone()).unwrap_or_default()));
                        }
                    }
                }
                let p = ui.painter();
                theme::text(p, Pos2::new(cols[3], y), d.vendor.as_deref().unwrap_or("Unknown vendor"), theme::regular(13.5), c::muted(), cols[4] - cols[3] - 12.0);
                theme::text(p, Pos2::new(cols[4], y), &d.ip, theme::regular(13.5), tc, cols[5] - cols[4] - 8.0);
                theme::text(p, Pos2::new(cols[5], y), &d.mac.to_uppercase(), theme::regular(13.5), tc, cols[6] - cols[5] - 8.0);
                theme::text(p, Pos2::new(cols[6], y), &if online { "Now".to_string() } else { theme::date_time(d.last_seen) }, theme::regular(13.5), c::muted(), cols[7] - cols[6] - 8.0);
                theme::text(p, Pos2::new(cols[7], y), &theme::date_time(d.first_seen), theme::regular(13.5), c::muted(), card.right() - cols[7] - 14.0);
            }
        });
    }
}
