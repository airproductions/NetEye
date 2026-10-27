//! Traffic Monitor: per-app usage list on the left, live graph / usage bars / hosts on the right.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::motion;
use super::theme::{self, c, Glyph, Icons};
use crate::monitor::{Shared, OTHER_KEY};
use crate::store::{self, HOUR, MINUTE};

const RANGES: [&str; 5] = ["5 Minutes", "3 Hours", "24 Hours", "Week", "Month"];
const RANGE_CODES: [&str; 5] = ["5m", "3h", "24h", "7d", "30d"];
const ROW_H: f32 = 32.0;
const GUTTER: f32 = 16.0;
const GAP: f32 = 10.0;
const STRIP_H: f32 = 96.0;
const CARD_R: u8 = 14;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Normal,
    Warn,
    Over,
}

/// Fractions of the semicircle gauge covered by download and upload.
#[derive(Clone, Copy, Debug)]
struct Gauge {
    down: f32,
    up: f32,
    level: Level,
}

/// Today's usage against the daily cap. No cap: a full arc split by the download/upload ratio.
fn gauge(today: [u64; 2], cap_bytes: u64) -> Gauge {
    let (d, u) = (today[0] as f64, today[1] as f64);
    let total = d + u;
    if cap_bytes == 0 {
        if total <= 0.0 {
            return Gauge { down: 0.0, up: 0.0, level: Level::Normal };
        }
        let down = (d / total) as f32;
        return Gauge { down, up: 1.0 - down, level: Level::Normal };
    }
    let used = (total / cap_bytes as f64).min(1.0);
    let (down, up) = if total > 0.0 { ((d / total * used) as f32, (u / total * used) as f32) } else { (0.0, 0.0) };
    let level = if used >= 1.0 { Level::Over } else if used > 0.8 { Level::Warn } else { Level::Normal };
    Gauge { down, up, level }
}

/// Identifies what the strip is showing; a change means the user picked another range or app.
fn selection_key(range: usize, selected: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (range, selected).hash(&mut h);
    h.finish()
}

/// Below this page width the strip drops the apps/connections section instead of squeezing it.
fn strip_shows_counts(width: f32) -> bool {
    width >= 1180.0
}

#[derive(Default)]
pub struct TrafficPage {
    range: usize,
    mode: usize,
    selected: Option<String>,
    search: String,
    show_search: bool,
    /// (range, newest sample time, ctx time it arrived) for the live-graph glide.
    glide: Option<(usize, i64, f64)>,
    /// Strip numbers, gauge and bars ease only right after a selection change; data ticks snap.
    ease: motion::EaseGate,
}

struct Series {
    times: Vec<i64>,
    step: i64,
    down: Vec<f64>, // bytes per step
    up: Vec<f64>,
    apps: Vec<Vec<(String, [u64; 2])>>,
    app_totals: HashMap<String, [u64; 2]>,
    host_totals: HashMap<String, [u64; 2]>,
    lan: [u64; 2],
}

impl TrafficPage {
    fn series(&self, s: &Shared) -> Series {
        let filter = self.selected.as_deref();
        let pick = |down: u64, up: u64, apps: &HashMap<String, [u64; 2]>| -> (f64, f64) {
            match filter {
                None => (down as f64, up as f64),
                Some(k) => apps.get(k).map(|v| (v[0] as f64, v[1] as f64)).unwrap_or((0.0, 0.0)),
            }
        };
        let mut out = Series {
            times: vec![],
            step: 1,
            down: vec![],
            up: vec![],
            apps: vec![],
            app_totals: HashMap::new(),
            host_totals: HashMap::new(),
            lan: [0, 0],
        };
        let now = s.live.back().map(|l| l.t).unwrap_or_else(store::now);
        if self.range == 0 {
            let by_t: HashMap<i64, &crate::monitor::LiveSample> = s.live.iter().rev().take(300).map(|l| (l.t, l)).collect();
            for t in (now - 299)..=now {
                out.times.push(t);
                match by_t.get(&t) {
                    Some(l) => {
                        let apps: HashMap<String, [u64; 2]> = l.apps.iter().cloned().collect();
                        let (d, u) = pick(l.down, l.up, &apps);
                        out.down.push(d);
                        out.up.push(u);
                        for (k, v) in &l.apps {
                            let e = out.app_totals.entry(k.clone()).or_default();
                            e[0] += v[0];
                            e[1] += v[1];
                        }
                        out.apps.push(l.apps.clone());
                    }
                    None => {
                        out.down.push(0.0);
                        out.up.push(0.0);
                        out.apps.push(vec![]);
                    }
                }
            }
            for b in s.store.data.minutes.iter().rev().take(5) {
                out.lan[0] += b.lan_down;
                out.lan[1] += b.lan_up;
                for (k, v) in &b.hosts {
                    let e = out.host_totals.entry(k.clone()).or_default();
                    e[0] += v[0];
                    e[1] += v[1];
                }
            }
            return out;
        }
        let (step, count, list) = match self.range {
            1 => (MINUTE, 180, &s.store.data.minutes),
            2 => (MINUTE, 1440, &s.store.data.minutes),
            3 => (HOUR, 168, &s.store.data.hours),
            _ => (HOUR, 720, &s.store.data.hours),
        };
        out.step = step;
        let end = now - now.rem_euclid(step);
        let start = end - step * (count - 1);
        let by_t: HashMap<i64, &store::Bucket> = list.iter().filter(|b| b.t >= start).map(|b| (b.t, b)).collect();
        let mut t = start;
        while t <= end {
            out.times.push(t);
            match by_t.get(&t) {
                Some(b) => {
                    let (d, u) = pick(b.down, b.up, &b.apps);
                    out.down.push(d);
                    out.up.push(u);
                    out.lan[0] += b.lan_down;
                    out.lan[1] += b.lan_up;
                    for (k, v) in &b.apps {
                        let e = out.app_totals.entry(k.clone()).or_default();
                        e[0] += v[0];
                        e[1] += v[1];
                    }
                    for (k, v) in &b.hosts {
                        let e = out.host_totals.entry(k.clone()).or_default();
                        e[0] += v[0];
                        e[1] += v[1];
                    }
                    let mut apps: Vec<(String, [u64; 2])> = b.apps.iter().map(|(k, v)| (k.clone(), *v)).collect();
                    apps.sort_by_key(|(_, v)| std::cmp::Reverse(v[0] + v[1]));
                    apps.truncate(4);
                    out.apps.push(apps);
                }
                None => {
                    out.down.push(0.0);
                    out.up.push(0.0);
                    out.apps.push(vec![]);
                }
            }
            t += step;
        }
        out
    }

    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons) {
        // decided with the selection that `series` is built from, so the key and the data change in the same frame
        let ease = self.ease.eases(selection_key(self.range, self.selected.as_deref()), ui.input(|i| i.time));
        let series = self.series(s);
        let full = ui.max_rect();
        let inner = full.shrink(GUTTER);
        let body_h = (inner.height() - STRIP_H - GAP).max(0.0);
        let left_w = 400.0f32.min(inner.width() * 0.34);
        let left = Rect::from_min_size(inner.min, Vec2::new(left_w, body_h));
        let right = Rect::from_min_max(Pos2::new(left.right() + GAP, inner.top()), Pos2::new(inner.right(), inner.top() + body_h));
        let strip = Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - STRIP_H), inner.max);

        for r in [left, right, strip] {
            theme::glass(ui.painter(), r, CARD_R);
        }
        let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left.shrink2(Vec2::new(0.0, 8.0))));
        lui.set_clip_rect(left.shrink(1.0).intersect(ui.clip_rect()));
        self.app_list(&mut lui, s, icons, &series);
        let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right.shrink(18.0)));
        self.chart_area(&mut rui, s, icons, &series);
        let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(strip));
        self.strip(&mut fui, s, &series, full.width(), ease);
    }

    fn app_list(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, series: &Series) {
        let r = ui.max_rect();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            if self.show_search {
                let resp = theme::search_box(ui, &mut self.search, r.width() - 80.0, "Search apps");
                resp.request_focus();
                if theme::icon_button(ui, Glyph::Search, 30.0, "Close search").clicked() {
                    self.show_search = false;
                    self.search.clear();
                }
            } else {
                let (gr, gresp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                theme::glyph(ui.painter(), Rect::from_center_size(gr.center(), Vec2::splat(18.0)), Glyph::Grid, c::accent());
                gresp.on_hover_text(format!("Apps · {}", RANGES[self.range]));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                    if theme::icon_button(ui, Glyph::Search, 30.0, "Search apps").clicked() {
                        self.show_search = true;
                    }
                });
            }
        });
        ui.add_space(8.0);

        let (cd, cu) = (r.right() - 88.0, r.right() - 14.0);
        let (hr, _) = ui.allocate_exact_size(Vec2::new(r.width(), 28.0), Sense::hover());
        let p = ui.painter();
        theme::glyph(p, Rect::from_center_size(Pos2::new(cd - 26.0, hr.center().y), Vec2::splat(13.0)), Glyph::Down, c::down());
        theme::glyph(p, Rect::from_center_size(Pos2::new(cu - 26.0, hr.center().y), Vec2::splat(13.0)), Glyph::Up, c::up());
        p.line_segment([Pos2::new(r.left(), hr.bottom()), Pos2::new(r.right(), hr.bottom())], Stroke::new(1.0_f32, c::border()));

        let mut apps: Vec<(String, String, [u64; 2])> = series
            .app_totals
            .iter()
            .filter(|(_, v)| v[0] + v[1] > 0)
            .map(|(k, v)| (k.clone(), s.app_name(k), *v))
            .collect();
        let q = self.search.to_lowercase();
        apps.retain(|(_, n, _)| q.is_empty() || n.to_lowercase().contains(&q));
        apps.sort_by_key(|(_, _, v)| std::cmp::Reverse(v[0] + v[1]));
        // Without admin there are no per-app byte counts; list connected apps by connection count.
        let mut connected: Vec<(String, String, usize)> = Vec::new();
        if !s.admin {
            for (k, l) in &s.apps_live {
                let n = l.in_conns + l.out_conns;
                let name = s.app_name(k);
                if n > 0 && k != OTHER_KEY && (q.is_empty() || name.to_lowercase().contains(&q)) {
                    connected.push((k.clone(), name, n));
                }
            }
            connected.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
        }
        let all: [u64; 2] = [series.down.iter().sum::<f64>() as u64, series.up.iter().sum::<f64>() as u64];
        let all = if self.selected.is_some() {
            series.app_totals.values().fold([0, 0], |a, v| [a[0] + v[0], a[1] + v[1]])
        } else {
            all
        };

        let extra = if connected.is_empty() { 0 } else { connected.len() + 1 };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, apps.len() + 1 + extra, |ui, range| {
            for i in range {
                if i > apps.len() {
                    let j = i - apps.len() - 1;
                    let (row, resp) = ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::hover());
                    if j == 0 {
                        ui.painter().text(Pos2::new(row.left() + 16.0, row.center().y + 4.0), Align2::LEFT_CENTER, "CONNECTED NOW", theme::semibold(11.5), c::muted());
                        continue;
                    }
                    let (k, name, n) = &connected[j - 1];
                    if resp.hovered() {
                        ui.painter().rect_filled(row, CornerRadius::ZERO, c::hover());
                    }
                    icons.draw(ui, s, k, Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(18.0)), name);
                    let p = ui.painter();
                    theme::text(p, Pos2::new(row.left() + 46.0, row.center().y), name, theme::regular(14.0), c::text(), cd - row.left() - 120.0);
                    theme::text_right(p, Pos2::new(cu, row.center().y), &format!("{n} conn{}", if *n == 1 { "" } else { "s" }), theme::regular(13.0), c::muted());
                    continue;
                }
                let (key, name, v) = if i == 0 { (None, "All apps".to_string(), all) } else {
                    let (k, n, v) = &apps[i - 1];
                    (Some(k.clone()), n.clone(), *v)
                };
                let (row, resp) = ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::click());
                let selected = self.selected == key;
                let p = ui.painter();
                if selected {
                    p.rect_filled(row, CornerRadius::ZERO, c::selected());
                } else if resp.hovered() {
                    p.rect_filled(row, CornerRadius::ZERO, c::hover());
                } else if i % 2 == 1 {
                    p.rect_filled(row, CornerRadius::ZERO, c::row_alt());
                }
                let ir = Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(18.0));
                match &key {
                    None => theme::glyph(p, ir, Glyph::Grid, c::accent()),
                    Some(k) if k == OTHER_KEY => theme::glyph(p, ir, Glyph::Globe, c::muted()),
                    Some(k) => icons.draw(ui, s, k, ir, &name),
                }
                let p = ui.painter();
                theme::text(p, Pos2::new(row.left() + 46.0, row.center().y), &name, if i == 0 { theme::semibold(14.0) } else { theme::regular(14.0) }, c::text(), cd - row.left() - 120.0);
                theme::text_right(p, Pos2::new(cd, row.center().y), &theme::bytes(v[0]), theme::regular(13.5), c::text());
                theme::text_right(p, Pos2::new(cu, row.center().y), &theme::bytes(v[1]), theme::regular(13.5), c::text());
                if resp.clicked() {
                    self.selected = key;
                }
            }
        });
    }

    fn chart_area(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, series: &Series) {
        let r = ui.max_rect();
        // header: mode switch on the left, clock + range codes on the right
        let head = Rect::from_min_size(r.min, Vec2::new(r.width(), 36.0));
        let mut hl = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
        theme::segmented(&mut hl, &[(Glyph::Graph, "Live graph"), (Glyph::Bars, "Usage"), (Glyph::Globe, "Hosts")], &mut self.mode);
        let mut hr = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::right_to_left(egui::Align::Center)));
        theme::segmented_text(&mut hr, &RANGE_CODES, &mut self.range, Some(&RANGES));
        let (cr, cresp) = hr.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        theme::glyph(hr.painter(), Rect::from_center_size(cr.center(), Vec2::splat(16.0)), Glyph::Clock, c::muted());
        cresp.on_hover_text("Time range");

        // title row: which app the chart and strip are showing
        let title = Rect::from_min_size(Pos2::new(r.left(), r.top() + 44.0), Vec2::new(r.width(), 28.0));
        let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(title).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if let Some(k) = self.selected.clone() {
            let name = s.app_name(&k);
            let (ir, _) = tui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            if k == OTHER_KEY {
                theme::glyph(tui.painter(), ir, Glyph::Globe, c::muted());
            } else {
                icons.draw(&tui, s, &k, ir, &name);
            }
            theme::label(&mut tui, name, theme::semibold(16.0), c::text());
            if theme::pill(&mut tui, "Show all apps", None, false).clicked() {
                self.selected = None;
            }
        } else {
            theme::label(&mut tui, "All apps", theme::semibold(16.0), c::text());
        }

        let chart = Rect::from_min_max(Pos2::new(r.left(), r.top() + 90.0), Pos2::new(r.right(), r.bottom() - 28.0));
        match self.mode {
            0 => self.graph(ui, s, chart, series),
            1 => self.bars(ui, chart, series),
            _ => self.hosts(ui, s, chart, series),
        }
    }

    fn time_axis(&self, p: &egui::Painter, chart: Rect, series: &Series) {
        let n = series.times.len();
        if n < 2 {
            return;
        }
        let ticks = ((chart.width() / 130.0) as usize).clamp(2, 12);
        for i in 0..=ticks {
            let idx = (n - 1) * i / ticks;
            let x = chart.left() + chart.width() * idx as f32 / (n - 1) as f32;
            let t = series.times[idx];
            let label = match self.range {
                0 => theme::local(t).format("%-I:%M:%S").to_string(),
                1 | 2 => theme::clock(t),
                _ => theme::local(t).format("%a %-d").to_string(),
            };
            p.line_segment([Pos2::new(x, chart.bottom()), Pos2::new(x, chart.bottom() + 4.0)], Stroke::new(1.0_f32, c::border()));
            let align = if i == 0 { Align2::LEFT_TOP } else if i == ticks { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
            p.text(Pos2::new(x, chart.bottom() + 8.0), align, label, theme::regular(12.0), c::muted());
        }
    }

    fn graph(&mut self, ui: &mut egui::Ui, s: &mut Shared, chart: Rect, series: &Series) {
        let step = series.step as f64;
        let down: Vec<f64> = series.down.iter().map(|v| v / step).collect();
        let up: Vec<f64> = series.up.iter().map(|v| v / step).collect();
        let max = down.iter().chain(up.iter()).cloned().fold(0.0, f64::max);
        let p = ui.painter_at(chart.expand(30.0));
        if max <= 0.0 {
            theme::empty_state(&p, chart.center(), "No traffic in this period. Try another time range.");
            self.time_axis(&p, chart, series);
            return;
        }
        let nice = theme::nice_max(max);
        for i in 1..=4 {
            let y = chart.bottom() - chart.height() * i as f32 / 4.0;
            p.line_segment([Pos2::new(chart.left(), y), Pos2::new(chart.right(), y)], Stroke::new(1.0_f32, c::border()));
            p.text(Pos2::new(chart.right() - 4.0, y - 9.0), Align2::RIGHT_CENTER, theme::rate(nice * i as f64 / 4.0), theme::regular(11.5), c::faint());
        }
        p.line_segment([chart.left_bottom(), chart.right_bottom()], Stroke::new(1.0_f32, c::border()));
        // a new live sample slides the series in from the left over NORMAL
        let mut shift = 0.0;
        if self.range == 0 && series.times.len() > 1 {
            let newest = *series.times.last().unwrap();
            let now = ui.input(|i| i.time);
            match self.glide {
                Some((r, t, _)) if r == self.range && t == newest => {}
                Some((r, t, _)) if r == self.range && (1..=5).contains(&(newest - t)) => self.glide = Some((self.range, newest, now)),
                _ => self.glide = Some((self.range, newest, f64::NEG_INFINITY)),
            }
            if let Some((_, _, started)) = self.glide {
                let t = ((now - started) / motion::NORMAL as f64) as f32;
                if t < 1.0 {
                    shift = chart.width() / (series.times.len() - 1) as f32 * (1.0 - motion::ease_out(t));
                    // capped at ~30 fps: the glide runs after every live sample, so full-rate frames would cost CPU
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
                }
            }
        }
        let sp = ui.painter_at(chart);
        let moved = chart.translate(Vec2::new(shift, 0.0));
        theme::area(&sp, moved, &down, nice, c::down());
        theme::area(&sp, moved, &up, nice, c::up());
        self.time_axis(&p, chart, series);

        let resp = ui.interact(chart, ui.id().with("graph"), Sense::hover());
        if let Some(pos) = resp.hover_pos() {
            let n = series.times.len();
            let idx = (((pos.x - chart.left()) / chart.width()) * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32) as usize;
            let x = chart.left() + chart.width() * idx as f32 / (n - 1) as f32;
            p.line_segment([Pos2::new(x, chart.top()), Pos2::new(x, chart.bottom())], Stroke::new(1.0_f32, c::muted()));
            for (vals, col) in [(&down, c::down()), (&up, c::up())] {
                let y = chart.bottom() - (vals[idx] / nice).clamp(0.0, 1.0) as f32 * chart.height();
                p.circle(Pos2::new(x, y), 4.0, col, Stroke::new(2.0_f32, c::bg()));
            }
            let t = series.times[idx];
            let apps = series.apps[idx].clone();
            let (d, u) = (down[idx], up[idx]);
            let range = self.range;
            let names: Vec<(String, [u64; 2])> = apps.iter().take(4).map(|(k, v)| (s.app_name(k), *v)).collect();
            let filtered = self.selected.is_some();
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("tip"), |ui| {
                let when = if range == 0 { theme::local(t).format("%-I:%M:%S %p").to_string() } else if range <= 2 { theme::clock(t) } else { theme::local(t).format("%a %-d %b, %-I %p").to_string() };
                theme::label(ui, when, theme::semibold(13.0), c::text());
                ui.horizontal(|ui| {
                    theme::label(ui, format!("↓ {}", theme::rate(d)), theme::regular(13.0), c::down());
                    theme::label(ui, format!("↑ {}", theme::rate(u)), theme::regular(13.0), c::up());
                });
                if !filtered && !names.is_empty() {
                    ui.separator();
                    for (n, v) in names {
                        ui.horizontal(|ui| {
                            theme::label(ui, n, theme::regular(12.5), c::text());
                            theme::label(ui, theme::bytes(v[0] + v[1]), theme::regular(12.5), c::muted());
                        });
                    }
                }
            });
        }
    }

    fn bars(&self, ui: &mut egui::Ui, chart: Rect, series: &Series) {
        // Group into calendar-friendly buckets: 10s, 5min, 1h, 1 day, 1 day
        let group = match self.range {
            0 => 10,
            1 => 5,
            2 => 60,
            _ => 24,
        };
        let mut groups: Vec<(i64, f64, f64)> = Vec::new();
        for (i, chunk) in series.down.chunks(group).enumerate() {
            let up: f64 = series.up[i * group..(i * group + chunk.len())].iter().sum();
            groups.push((series.times[i * group], chunk.iter().sum(), up));
        }
        let max = groups.iter().map(|g| g.1.max(g.2)).fold(0.0, f64::max);
        let p = ui.painter_at(chart.expand(30.0));
        if max <= 0.0 {
            theme::empty_state(&p, chart.center(), "No traffic in this period. Try another time range.");
            return;
        }
        let nice = theme::nice_max(max);
        for i in 1..=4 {
            let y = chart.bottom() - chart.height() * i as f32 / 4.0;
            p.line_segment([Pos2::new(chart.left(), y), Pos2::new(chart.right(), y)], Stroke::new(1.0_f32, c::border()));
            p.text(Pos2::new(chart.right() - 4.0, y - 9.0), Align2::RIGHT_CENTER, theme::bytes((nice * i as f64 / 4.0) as u64), theme::regular(11.5), c::faint());
        }
        let slot = chart.width() / groups.len() as f32;
        let bw = (slot * 0.34).min(16.0);
        let resp = ui.interact(chart, ui.id().with("bars"), Sense::hover());
        let hover = resp.hover_pos().map(|pos| ((pos.x - chart.left()) / slot) as usize);
        for (i, (t, d, u)) in groups.iter().enumerate() {
            let cx = chart.left() + slot * (i as f32 + 0.5);
            if hover == Some(i) {
                p.rect_filled(Rect::from_center_size(Pos2::new(cx, chart.center().y), Vec2::new(slot, chart.height())), CornerRadius::same(4), c::hover());
            }
            for (v, col, off) in [(d, c::down(), -bw / 2.0 - 1.0), (u, c::up(), bw / 2.0 + 1.0)] {
                let h = (*v / nice) as f32 * chart.height();
                if h > 0.5 {
                    p.rect_filled(
                        Rect::from_min_max(Pos2::new(cx + off - bw / 2.0, chart.bottom() - h), Pos2::new(cx + off + bw / 2.0, chart.bottom())),
                        CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 },
                        col,
                    );
                }
            }
            let every = (groups.len() / 10).max(1);
            if i % every == 0 {
                let label = match self.range {
                    0 => theme::local(*t).format("%-I:%M:%S").to_string(),
                    1 | 2 => theme::clock(*t),
                    _ => theme::local(*t).format("%a %-d").to_string(),
                };
                p.text(Pos2::new(cx, chart.bottom() + 8.0), Align2::CENTER_TOP, label, theme::regular(12.0), c::muted());
            }
            if hover == Some(i) {
                let (d, u, t) = (*d, *u, *t);
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("btip"), |ui| {
                    theme::label(ui, theme::date_time(t), theme::semibold(13.0), c::text());
                    theme::label(ui, format!("↓ {}   ↑ {}", theme::bytes(d as u64), theme::bytes(u as u64)), theme::regular(13.0), c::text());
                });
            }
        }
        p.line_segment([chart.left_bottom(), chart.right_bottom()], Stroke::new(1.0_f32, c::border()));
    }

    fn hosts(&self, ui: &mut egui::Ui, s: &mut Shared, chart: Rect, series: &Series) {
        let mut hosts: Vec<(&String, &[u64; 2])> = series.host_totals.iter().collect();
        hosts.sort_by_key(|(_, v)| std::cmp::Reverse(v[0] + v[1]));
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(chart));
        if hosts.is_empty() {
            let msg = if s.admin { "No host traffic recorded in this period." } else { "Per-host traffic needs administrator mode." };
            theme::empty_state(&cui.painter_at(chart), chart.center(), msg);
            return;
        }
        let max = hosts.first().map(|(_, v)| v[0] + v[1]).unwrap_or(1).max(1);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut cui, |ui| {
            for (host, v) in hosts.iter().take(200) {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(chart.width(), 40.0), Sense::hover());
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(row, CornerRadius::same(6), c::hover());
                }
                theme::glyph(p, Rect::from_center_size(Pos2::new(row.left() + 18.0, row.center().y), Vec2::splat(16.0)), Glyph::Globe, c::muted());
                let name_w = (row.width() * 0.42).min(420.0);
                theme::text(p, Pos2::new(row.left() + 38.0, row.center().y), host, theme::regular(14.0), c::text(), name_w - 44.0);
                let bar_l = row.left() + name_w;
                let bar_w = row.width() - name_w - 190.0;
                let total = v[0] + v[1];
                let wd = bar_w * v[0] as f32 / max as f32;
                let wu = bar_w * v[1] as f32 / max as f32;
                let by = row.center().y;
                p.rect_filled(Rect::from_min_size(Pos2::new(bar_l, by - 4.0), Vec2::new(bar_w, 8.0)), CornerRadius::same(4), c::raised());
                p.rect_filled(Rect::from_min_size(Pos2::new(bar_l, by - 4.0), Vec2::new(wd.max(2.0), 8.0)), CornerRadius::same(4), c::down());
                p.rect_filled(Rect::from_min_size(Pos2::new(bar_l + wd, by - 4.0), Vec2::new(wu, 8.0)), CornerRadius::same(4), c::up());
                theme::text_right(p, Pos2::new(row.right() - 100.0, by), &format!("↓ {}", theme::bytes(v[0])), theme::regular(13.0), c::text());
                theme::text_right(p, Pos2::new(row.right() - 8.0, by), &format!("↑ {}", theme::bytes(v[1])), theme::regular(13.0), c::muted());
                let _ = total;
            }
        });
    }

    /// Bottom usage strip: download total, today gauge, upload total, then the Internet/LAN split.
    fn strip(&self, ui: &mut egui::Ui, s: &Shared, series: &Series, page_w: f32, ease: bool) {
        let r = ui.max_rect();
        let ctx = ui.ctx().clone();
        let id = ui.id().with("strip");
        let cy = r.center().y;

        // live rate follows the selected app
        let cur = s
            .live
            .back()
            .map(|l| match &self.selected {
                None => (l.down, l.up),
                Some(k) => l.apps.iter().find(|(a, _)| a == k).map(|(_, v)| (v[0], v[1])).unwrap_or((0, 0)),
            })
            .unwrap_or((0, 0));
        let total = [series.down.iter().sum::<f64>() as u64, series.up.iter().sum::<f64>() as u64];
        let mut today = s.store.data.daily.get(&chrono::Local::now().format("%Y-%m-%d").to_string()).copied().unwrap_or([0, 0]);
        let cap = s.store.data.settings.daily_cap_mb.saturating_mul(1_000_000);
        let period = match s.store.data.settings.plan_period {
            store::PlanPeriod::Day => "today",
            store::PlanPeriod::Week => "this week",
            store::PlanPeriod::Month => "this month",
        };
        if cap > 0 {
            // Plan usage, split down/up by today's ratio so the arc keeps both colours.
            let (used, _) = s.store.plan_usage();
            let t = (today[0] + today[1]).max(1) as f64;
            let down = (used as f64 * today[0] as f64 / t) as u64;
            today = if today == [0, 0] { [used, 0] } else { [down, used - down.min(used)] };
        }

        let big = theme::bold(17.0);
        let small = theme::regular(12.0);
        let slot_w = motion::num_slot(ui, &big, "999.99 GB").max(motion::num_slot(ui, &small, "999.9 MB/s"));
        let num = |key: &str, v: f64| motion::number(&ctx, id.with(key), v, ease).max(0.0);
        let col = |p: &egui::Painter, x: f32, right: bool, bytes_v: f64, rate_v: f64, key: &str| {
            let (al, ax) = if right { (Align2::RIGHT_CENTER, x + slot_w) } else { (Align2::LEFT_CENTER, x) };
            p.text(Pos2::new(ax, cy - 9.0), al, theme::bytes(num(&format!("{key}b"), bytes_v) as u64), big.clone(), c::text());
            p.text(Pos2::new(ax, cy + 12.0), al, theme::rate(num(&format!("{key}r"), rate_v)), small.clone(), c::muted());
        };

        let p = ui.painter().clone();
        let mut x = r.left() + 22.0;
        col(&p, x, true, total[0] as f64, cur.0 as f64, "d");
        x += slot_w + 8.0;
        let arrow = |x: f32, g: Glyph, color: Color32| {
            theme::glyph(&p, Rect::from_center_size(Pos2::new(x + 8.0, cy), Vec2::splat(16.0)), g, color);
        };
        arrow(x, Glyph::Down, c::down());
        x += 16.0 + 14.0;

        // gauge: 120x70 semicircle
        let g = gauge(today, cap);
        let gr = Rect::from_min_size(Pos2::new(x, cy - 35.0), Vec2::new(120.0, 70.0));
        let gc = Pos2::new(gr.center().x, gr.bottom() - 8.0);
        let rad = 52.0;
        let gd = (motion::gated(&ctx, id.with("gd"), g.down as f64, ease) as f32).clamp(0.0, 1.0);
        let gu = (motion::gated(&ctx, id.with("gu"), g.up as f64, ease) as f32).clamp(0.0, 1.0 - gd);
        let arc = |f0: f32, f1: f32| -> Vec<Pos2> {
            let n = ((f1 - f0).abs() * 48.0).ceil().max(1.0) as usize;
            (0..=n)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + f0 + (f1 - f0) * i as f32 / n as f32);
                    Pos2::new(gc.x + a.cos() * rad, gc.y + a.sin() * rad)
                })
                .collect()
        };
        p.add(egui::Shape::line(arc(0.0, 1.0), Stroke::new(8.0_f32, c::raised())));
        match g.level {
            Level::Normal => {
                if gd > 0.002 {
                    p.add(egui::Shape::line(arc(0.0, gd), Stroke::new(8.0_f32, c::down())));
                }
                if gu > 0.002 {
                    p.add(egui::Shape::line(arc(gd, gd + gu), Stroke::new(8.0_f32, c::up())));
                }
            }
            lvl => {
                let color = if lvl == Level::Over { c::bad() } else { c::warn() };
                if gd + gu > 0.002 {
                    p.add(egui::Shape::line(arc(0.0, gd + gu), Stroke::new(8.0_f32, color)));
                }
            }
        }
        p.text(Pos2::new(gc.x, gc.y - 22.0), Align2::CENTER_CENTER, theme::bytes(num("today", (today[0] + today[1]) as f64) as u64), theme::bold(14.0), c::text());
        let sub = if cap > 0 { format!("of {}", theme::bytes(cap)) } else { "today".to_string() };
        p.text(Pos2::new(gc.x, gc.y - 5.0), Align2::CENTER_CENTER, sub, theme::regular(11.0), c::muted());
        let tip = if cap > 0 { format!("Usage {period} against your data plan") } else { "Today's usage, download and upload".to_string() };
        ui.interact(gr, id.with("gaugetip"), Sense::hover()).on_hover_text(tip);
        x = gr.right() + 14.0;

        arrow(x, Glyph::Up, c::up());
        x += 16.0 + 8.0;
        col(&p, x, false, total[1] as f64, cur.1 as f64, "u");
        x += slot_w + 22.0;

        let divider = |x: f32| p.line_segment([Pos2::new(x, r.top() + 16.0), Pos2::new(x, r.bottom() - 16.0)], Stroke::new(1.0_f32, c::border()));
        divider(x);
        x += 22.0;

        // Internet / Local network rows
        let show_counts = strip_shows_counts(page_w);
        let counts_w = if show_counts { 210.0 } else { 0.0 };
        let lan = series.lan;
        let wan = [total[0].saturating_sub(lan[0]), total[1].saturating_sub(lan[1])];
        let rows_right = r.right() - 22.0 - counts_w;
        let tot_font = theme::semibold(13.5);
        let tot_w = motion::num_slot(ui, &tot_font, "999.99 GB");
        let bar_x = x + 22.0 + tot_w + 10.0;
        let bar_w = (rows_right - bar_x).clamp(40.0, 260.0);
        let sum = |v: [u64; 2]| v[0] as f64 + v[1] as f64;
        let maxv = sum(wan).max(sum(lan)).max(1.0);
        for (i, (gl, tip, v)) in [(Glyph::Globe, "Internet", wan), (Glyph::Wifi, "Local network", lan)].into_iter().enumerate() {
            let y = cy + if i == 0 { -14.0 } else { 14.0 };
            let ir = Rect::from_center_size(Pos2::new(x + 8.0, y), Vec2::splat(16.0));
            theme::glyph(&p, ir, gl, c::muted());
            ui.interact(ir.expand(4.0), id.with(("rowtip", i)), Sense::hover()).on_hover_text(tip);
            let t = num(&format!("row{i}"), sum(v));
            p.text(Pos2::new(x + 22.0 + tot_w, y), Align2::RIGHT_CENTER, theme::bytes(t as u64), tot_font.clone(), c::text());
            let track = Rect::from_min_size(Pos2::new(bar_x, y - 4.0), Vec2::new(bar_w, 8.0));
            p.rect_filled(track, CornerRadius::same(4), c::raised());
            let frac = (motion::gated(&ctx, id.with(("barfrac", i)), sum(v) / maxv, ease) as f32).clamp(0.0, 1.0);
            let fill_w = bar_w * frac;
            if fill_w >= 1.0 && sum(v) > 0.0 {
                let wd = fill_w * (v[0] as f64 / sum(v)) as f32;
                let bp = p.with_clip_rect(Rect::from_min_size(track.min, Vec2::new(fill_w, 8.0)));
                if wd >= 1.0 {
                    let dr = Rect::from_min_size(track.min, Vec2::new(wd, 8.0));
                    let mut mesh = egui::Mesh::default();
                    let (c0, c1) = (c::accent(), c::accent2());
                    let sl = 12;
                    for k in 0..=sl {
                        let f = k as f32 / sl as f32;
                        let xx = dr.left() + dr.width() * f;
                        let colr = theme::lerp_color(c0, c1, f);
                        mesh.colored_vertex(Pos2::new(xx, dr.top()), colr);
                        mesh.colored_vertex(Pos2::new(xx, dr.bottom()), colr);
                        if k > 0 {
                            let b = (k as u32 - 1) * 2;
                            mesh.add_triangle(b, b + 1, b + 2);
                            mesh.add_triangle(b + 1, b + 3, b + 2);
                        }
                    }
                    bp.add(egui::Shape::mesh(mesh));
                }
                bp.rect_filled(Rect::from_min_max(Pos2::new(track.left() + wd, track.top()), Pos2::new(track.left() + fill_w, track.bottom())), CornerRadius::ZERO, c::up());
            }
        }

        if show_counts {
            divider(r.right() - counts_w - 12.0);
            let conns: usize = s.apps_live.values().map(|a| a.in_conns + a.out_conns).sum();
            let active = s.apps_live.values().filter(|a| a.in_conns + a.out_conns > 0).count();
            p.text(Pos2::new(r.right() - 22.0, cy), Align2::RIGHT_CENTER, format!("{active} apps · {conns} connections"), theme::regular(13.0), c::muted());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_no_cap_splits_ratio() {
        let g = gauge([300, 100], 0);
        assert!((g.down - 0.75).abs() < 1e-4 && (g.up - 0.25).abs() < 1e-4);
        assert_eq!(g.level, Level::Normal);
    }

    #[test]
    fn gauge_no_cap_zero() {
        let g = gauge([0, 0], 0);
        assert_eq!((g.down, g.up), (0.0, 0.0));
        assert_eq!(g.level, Level::Normal);
    }

    #[test]
    fn gauge_cap_partial() {
        let g = gauge([400, 100], 1000);
        assert!((g.down - 0.4).abs() < 1e-4 && (g.up - 0.1).abs() < 1e-4);
        assert_eq!(g.level, Level::Normal);
    }

    #[test]
    fn gauge_cap_warn() {
        assert_eq!(gauge([500, 350], 1000).level, Level::Warn);
    }

    #[test]
    fn gauge_cap_exceeded_clamps() {
        let g = gauge([3_000, 1_000], 1_000);
        assert!((g.down + g.up - 1.0).abs() < 1e-4);
        assert_eq!(g.level, Level::Over);
    }

    #[test]
    fn gauge_exactly_eighty_percent_is_normal() {
        assert_eq!(gauge([400, 400], 1000).level, Level::Normal);
    }

    #[test]
    fn gauge_exactly_cap_is_over() {
        let g = gauge([600, 400], 1000);
        assert_eq!(g.level, Level::Over);
        assert!((g.down + g.up - 1.0).abs() < 1e-4);
    }

    #[test]
    fn selection_key_tracks_range_and_app() {
        let base = selection_key(0, None);
        assert_eq!(base, selection_key(0, None));
        assert_ne!(base, selection_key(1, None));
        assert_ne!(base, selection_key(0, Some("chrome.exe")));
        assert_ne!(selection_key(0, Some("chrome.exe")), selection_key(0, Some("firefox.exe")));
        assert_eq!(selection_key(2, Some("a")), selection_key(2, Some("a")));
    }

    #[test]
    fn gauge_huge_values() {
        for cap in [0, 1, u64::MAX] {
            let g = gauge([u64::MAX / 2, u64::MAX / 2], cap);
            assert!((0.0..=1.0).contains(&g.down) && (0.0..=1.0).contains(&g.up));
            assert!(g.down + g.up <= 1.0 + 1e-4);
        }
    }

    #[test]
    fn strip_drops_counts_when_narrow() {
        assert!(!strip_shows_counts(1036.0));
        assert!(strip_shows_counts(1300.0));
    }
}
