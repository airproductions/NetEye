//! Traffic Monitor: per-app usage list on the left, live graph / usage bars / hosts on the right.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::theme::{self, c, Glyph, Icons};
use crate::monitor::{Shared, OTHER_KEY};
use crate::store::{self, HOUR, MINUTE};

const RANGES: [&str; 5] = ["5 Minutes", "3 Hours", "24 Hours", "Week", "Month"];
const ROW_H: f32 = 32.0;

#[derive(Default)]
pub struct TrafficPage {
    range: usize,
    mode: usize,
    selected: Option<String>,
    search: String,
    show_search: bool,
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
        let series = self.series(s);
        let full = ui.max_rect();
        let left_w = 470.0f32.min(full.width() * 0.4);
        let footer_h = 92.0;
        let left = Rect::from_min_max(full.min, Pos2::new(full.left() + left_w, full.bottom() - footer_h));
        let right = Rect::from_min_max(Pos2::new(left.right(), full.top()), Pos2::new(full.right(), full.bottom() - footer_h));
        let footer = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - footer_h), full.max);

        ui.painter().rect_filled(left, CornerRadius::ZERO, c::PANEL);
        ui.painter().line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0_f32, c::BORDER));
        let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left));
        self.app_list(&mut lui, s, icons, &series);
        let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right.shrink2(Vec2::new(24.0, 0.0))));
        self.chart_area(&mut rui, s, icons, &series);
        let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(footer));
        self.footer(&mut fui, s, &series);
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
                theme::label(ui, "Apps", theme::semibold(15.0), c::TEXT);
                theme::label(ui, format!("· {}", RANGES[self.range]), theme::regular(14.0), c::MUTED);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                    if theme::icon_button(ui, Glyph::Search, 30.0, "Search").clicked() {
                        self.show_search = true;
                    }
                });
            }
        });
        ui.add_space(8.0);

        let (cd, cu) = (r.right() - 88.0, r.right() - 14.0);
        let (hr, _) = ui.allocate_exact_size(Vec2::new(r.width(), 28.0), Sense::hover());
        let p = ui.painter();
        p.text(Pos2::new(r.left() + 46.0, hr.center().y), Align2::LEFT_CENTER, "App", theme::regular(13.0), c::MUTED);
        theme::glyph(p, Rect::from_center_size(Pos2::new(cd - 26.0, hr.center().y), Vec2::splat(13.0)), Glyph::Down, c::DOWN);
        theme::glyph(p, Rect::from_center_size(Pos2::new(cu - 26.0, hr.center().y), Vec2::splat(13.0)), Glyph::Up, c::UP);
        p.line_segment([Pos2::new(r.left(), hr.bottom()), Pos2::new(r.right(), hr.bottom())], Stroke::new(1.0_f32, c::BORDER));

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
                        ui.painter().text(Pos2::new(row.left() + 16.0, row.center().y + 4.0), Align2::LEFT_CENTER, "CONNECTED NOW", theme::semibold(11.5), c::MUTED);
                        continue;
                    }
                    let (k, name, n) = &connected[j - 1];
                    if resp.hovered() {
                        ui.painter().rect_filled(row, CornerRadius::ZERO, c::HOVER);
                    }
                    icons.draw(ui, s, k, Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(18.0)), name);
                    let p = ui.painter();
                    theme::text(p, Pos2::new(row.left() + 46.0, row.center().y), name, theme::regular(14.0), c::TEXT, cd - row.left() - 120.0);
                    theme::text_right(p, Pos2::new(cu, row.center().y), &format!("{n} conn{}", if *n == 1 { "" } else { "s" }), theme::regular(13.0), c::MUTED);
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
                    p.rect_filled(row, CornerRadius::ZERO, c::SELECTED);
                } else if resp.hovered() {
                    p.rect_filled(row, CornerRadius::ZERO, c::HOVER);
                } else if i % 2 == 1 {
                    p.rect_filled(row, CornerRadius::ZERO, c::ROW_ALT);
                }
                let ir = Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(18.0));
                match &key {
                    None => theme::glyph(p, ir, Glyph::Apps, c::ACCENT),
                    Some(k) if k == OTHER_KEY => theme::glyph(p, ir, Glyph::Globe, c::MUTED),
                    Some(k) => icons.draw(ui, s, k, ir, &name),
                }
                let p = ui.painter();
                theme::text(p, Pos2::new(row.left() + 46.0, row.center().y), &name, if i == 0 { theme::semibold(14.0) } else { theme::regular(14.0) }, c::TEXT, cd - row.left() - 120.0);
                theme::text_right(p, Pos2::new(cd, row.center().y), &theme::bytes(v[0]), theme::regular(13.5), c::TEXT);
                theme::text_right(p, Pos2::new(cu, row.center().y), &theme::bytes(v[1]), theme::regular(13.5), c::TEXT);
                if resp.clicked() {
                    self.selected = key;
                }
            }
        });
    }

    fn chart_area(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, series: &Series) {
        let r = ui.max_rect();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if let Some(k) = self.selected.clone() {
                let name = s.app_name(&k);
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                if k == OTHER_KEY {
                    theme::glyph(ui.painter(), ir, Glyph::Globe, c::MUTED);
                } else {
                    icons.draw(ui, s, &k, ir, &name);
                }
                theme::label(ui, name, theme::semibold(16.0), c::TEXT);
                if theme::pill(ui, "Show all apps", None, false).clicked() {
                    self.selected = None;
                }
            } else {
                theme::label(ui, "All apps", theme::semibold(16.0), c::TEXT);
            }
        });
        // centered mode switch + range dropdown at the right, overlaid on the same row
        let seg_w = 46.0 * 3.0 + 6.0;
        let seg_rect = Rect::from_min_size(Pos2::new(r.center().x - seg_w / 2.0, r.top() + 10.0), Vec2::new(seg_w, 38.0));
        let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(seg_rect));
        theme::segmented(&mut sui, &[(Glyph::Graph, "Live graph"), (Glyph::Bars, "Usage"), (Glyph::Globe, "Hosts")], &mut self.mode);
        let dd = Rect::from_min_size(Pos2::new(r.right() - 140.0, r.top() + 12.0), Vec2::new(140.0, 34.0));
        let mut dui = ui.new_child(egui::UiBuilder::new().max_rect(dd).layout(egui::Layout::right_to_left(egui::Align::Center)));
        theme::dropdown(&mut dui, "range", &RANGES, &mut self.range);

        let chart = Rect::from_min_max(Pos2::new(r.left(), r.top() + 84.0), Pos2::new(r.right(), r.bottom() - 40.0));
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
            p.line_segment([Pos2::new(x, chart.bottom()), Pos2::new(x, chart.bottom() + 4.0)], Stroke::new(1.0_f32, c::BORDER));
            let align = if i == 0 { Align2::LEFT_TOP } else if i == ticks { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
            p.text(Pos2::new(x, chart.bottom() + 8.0), align, label, theme::regular(12.0), c::MUTED);
        }
    }

    fn graph(&self, ui: &mut egui::Ui, s: &mut Shared, chart: Rect, series: &Series) {
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
            p.line_segment([Pos2::new(chart.left(), y), Pos2::new(chart.right(), y)], Stroke::new(1.0_f32, Color32::from_rgb(0x20, 0x24, 0x2c)));
            p.text(Pos2::new(chart.right() - 4.0, y - 9.0), Align2::RIGHT_CENTER, theme::rate(nice * i as f64 / 4.0), theme::regular(11.5), c::FAINT);
        }
        p.line_segment([chart.left_bottom(), chart.right_bottom()], Stroke::new(1.0_f32, c::BORDER));
        theme::area(&p, chart, &down, nice, c::DOWN);
        theme::area(&p, chart, &up, nice, c::UP);
        self.time_axis(&p, chart, series);

        let resp = ui.interact(chart, ui.id().with("graph"), Sense::hover());
        if let Some(pos) = resp.hover_pos() {
            let n = series.times.len();
            let idx = (((pos.x - chart.left()) / chart.width()) * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32) as usize;
            let x = chart.left() + chart.width() * idx as f32 / (n - 1) as f32;
            p.line_segment([Pos2::new(x, chart.top()), Pos2::new(x, chart.bottom())], Stroke::new(1.0_f32, c::MUTED));
            for (vals, col) in [(&down, c::DOWN), (&up, c::UP)] {
                let y = chart.bottom() - (vals[idx] / nice).clamp(0.0, 1.0) as f32 * chart.height();
                p.circle(Pos2::new(x, y), 4.0, col, Stroke::new(2.0_f32, c::BG));
            }
            let t = series.times[idx];
            let apps = series.apps[idx].clone();
            let (d, u) = (down[idx], up[idx]);
            let range = self.range;
            let names: Vec<(String, [u64; 2])> = apps.iter().take(4).map(|(k, v)| (s.app_name(k), *v)).collect();
            let filtered = self.selected.is_some();
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("tip"), |ui| {
                let when = if range == 0 { theme::local(t).format("%-I:%M:%S %p").to_string() } else if range <= 2 { theme::clock(t) } else { theme::local(t).format("%a %-d %b, %-I %p").to_string() };
                theme::label(ui, when, theme::semibold(13.0), c::TEXT);
                ui.horizontal(|ui| {
                    theme::label(ui, format!("↓ {}", theme::rate(d)), theme::regular(13.0), c::DOWN);
                    theme::label(ui, format!("↑ {}", theme::rate(u)), theme::regular(13.0), c::UP);
                });
                if !filtered && !names.is_empty() {
                    ui.separator();
                    for (n, v) in names {
                        ui.horizontal(|ui| {
                            theme::label(ui, n, theme::regular(12.5), c::TEXT);
                            theme::label(ui, theme::bytes(v[0] + v[1]), theme::regular(12.5), c::MUTED);
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
            p.line_segment([Pos2::new(chart.left(), y), Pos2::new(chart.right(), y)], Stroke::new(1.0_f32, Color32::from_rgb(0x20, 0x24, 0x2c)));
            p.text(Pos2::new(chart.right() - 4.0, y - 9.0), Align2::RIGHT_CENTER, theme::bytes((nice * i as f64 / 4.0) as u64), theme::regular(11.5), c::FAINT);
        }
        let slot = chart.width() / groups.len() as f32;
        let bw = (slot * 0.34).min(16.0);
        let resp = ui.interact(chart, ui.id().with("bars"), Sense::hover());
        let hover = resp.hover_pos().map(|pos| ((pos.x - chart.left()) / slot) as usize);
        for (i, (t, d, u)) in groups.iter().enumerate() {
            let cx = chart.left() + slot * (i as f32 + 0.5);
            if hover == Some(i) {
                p.rect_filled(Rect::from_center_size(Pos2::new(cx, chart.center().y), Vec2::new(slot, chart.height())), CornerRadius::same(4), c::HOVER);
            }
            for (v, col, off) in [(d, c::DOWN, -bw / 2.0 - 1.0), (u, c::UP, bw / 2.0 + 1.0)] {
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
                p.text(Pos2::new(cx, chart.bottom() + 8.0), Align2::CENTER_TOP, label, theme::regular(12.0), c::MUTED);
            }
            if hover == Some(i) {
                let (d, u, t) = (*d, *u, *t);
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("btip"), |ui| {
                    theme::label(ui, theme::date_time(t), theme::semibold(13.0), c::TEXT);
                    theme::label(ui, format!("↓ {}   ↑ {}", theme::bytes(d as u64), theme::bytes(u as u64)), theme::regular(13.0), c::TEXT);
                });
            }
        }
        p.line_segment([chart.left_bottom(), chart.right_bottom()], Stroke::new(1.0_f32, c::BORDER));
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
                    p.rect_filled(row, CornerRadius::same(6), c::HOVER);
                }
                theme::glyph(p, Rect::from_center_size(Pos2::new(row.left() + 18.0, row.center().y), Vec2::splat(16.0)), Glyph::Globe, c::MUTED);
                let name_w = (row.width() * 0.42).min(420.0);
                theme::text(p, Pos2::new(row.left() + 38.0, row.center().y), host, theme::regular(14.0), c::TEXT, name_w - 44.0);
                let bar_l = row.left() + name_w;
                let bar_w = row.width() - name_w - 190.0;
                let total = v[0] + v[1];
                let wd = bar_w * v[0] as f32 / max as f32;
                let wu = bar_w * v[1] as f32 / max as f32;
                let by = row.center().y;
                p.rect_filled(Rect::from_min_size(Pos2::new(bar_l, by - 4.0), Vec2::new(bar_w, 8.0)), CornerRadius::same(4), c::RAISED);
                p.rect_filled(Rect::from_min_size(Pos2::new(bar_l, by - 4.0), Vec2::new(wd.max(2.0), 8.0)), CornerRadius::same(4), c::DOWN);
                p.rect_filled(Rect::from_min_size(Pos2::new(bar_l + wd, by - 4.0), Vec2::new(wu, 8.0)), CornerRadius::same(4), c::UP);
                theme::text_right(p, Pos2::new(row.right() - 100.0, by), &format!("↓ {}", theme::bytes(v[0])), theme::regular(13.0), c::TEXT);
                theme::text_right(p, Pos2::new(row.right() - 8.0, by), &format!("↑ {}", theme::bytes(v[1])), theme::regular(13.0), c::MUTED);
                let _ = total;
            }
        });
    }

    fn footer(&self, ui: &mut egui::Ui, s: &Shared, series: &Series) {
        let r = ui.max_rect();
        let p = ui.painter();
        p.rect_filled(r, CornerRadius::ZERO, c::PANEL);
        p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, c::BORDER));
        let cur = s.live.back().map(|l| (l.down, l.up)).unwrap_or((0, 0));
        let today = s.store.data.daily.get(&chrono::Local::now().format("%Y-%m-%d").to_string()).copied().unwrap_or([0, 0]);
        let cy = r.center().y + 4.0;

        // gauge: today's usage against the cap (or a full ring when there is no cap)
        let gc = Pos2::new(r.left() + 200.0, cy + 10.0);
        let cap = s.store.data.settings.daily_cap_mb * 1_000_000;
        let frac = if cap > 0 { ((today[0] + today[1]) as f32 / cap as f32).min(1.0) } else { 0.0 };
        let arc = |a0: f32, a1: f32| -> Vec<Pos2> {
            (0..=40).map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / 40.0;
                Pos2::new(gc.x + a.cos() * 34.0, gc.y + a.sin() * 34.0)
            }).collect()
        };
        let (a0, a1) = (std::f32::consts::PI * 1.1, std::f32::consts::PI * 1.9);
        p.add(egui::Shape::line(arc(a0, a1), Stroke::new(6.0_f32, c::RAISED)));
        if frac > 0.0 {
            let col = if frac >= 1.0 { c::BAD } else if frac > 0.8 { c::WARN } else { c::ACCENT };
            p.add(egui::Shape::line(arc(a0, a0 + (a1 - a0) * frac), Stroke::new(6.0_f32, col)));
        }
        p.text(Pos2::new(gc.x, gc.y - 12.0), Align2::CENTER_CENTER, theme::bytes(today[0] + today[1]), theme::semibold(15.0), c::TEXT);
        p.text(Pos2::new(gc.x, gc.y + 6.0), Align2::CENTER_CENTER, if cap > 0 { "today of cap" } else { "today" }, theme::regular(11.5), c::MUTED);

        theme::glyph(p, Rect::from_center_size(Pos2::new(r.left() + 52.0, cy), Vec2::splat(14.0)), Glyph::Down, c::DOWN);
        p.text(Pos2::new(r.left() + 64.0, cy), Align2::LEFT_CENTER, theme::rate(cur.0 as f64), theme::semibold(16.0), c::TEXT);
        theme::glyph(p, Rect::from_center_size(Pos2::new(r.left() + 260.0, cy), Vec2::splat(14.0)), Glyph::Up, c::UP);
        p.text(Pos2::new(r.left() + 272.0, cy), Align2::LEFT_CENTER, theme::rate(cur.1 as f64), theme::semibold(16.0), c::TEXT);

        let x = r.left() + 420.0;
        p.line_segment([Pos2::new(x - 24.0, r.top() + 18.0), Pos2::new(x - 24.0, r.bottom() - 18.0)], Stroke::new(1.0_f32, c::BORDER));
        let total = [series.down.iter().sum::<f64>() as u64, series.up.iter().sum::<f64>() as u64];
        let wan = [total[0].saturating_sub(series.lan[0]), total[1].saturating_sub(series.lan[1])];
        for (i, (label, v)) in [("Internet", wan), ("Local network", series.lan)].iter().enumerate() {
            let y = cy - 12.0 + i as f32 * 24.0;
            p.text(Pos2::new(x, y), Align2::LEFT_CENTER, *label, theme::regular(13.0), c::MUTED);
            p.text(Pos2::new(x + 110.0, y), Align2::LEFT_CENTER, format!("↓ {}   ↑ {}", theme::bytes(v[0]), theme::bytes(v[1])), theme::regular(13.0), c::TEXT);
        }

        let conns: usize = s.apps_live.values().map(|a| a.in_conns + a.out_conns).sum();
        let active = s.apps_live.values().filter(|a| a.in_conns + a.out_conns > 0).count();
        let status = match s.internet {
            Some(true) => ("Online", c::GOOD),
            Some(false) => ("Offline", c::BAD),
            None => ("Checking…", c::MUTED),
        };
        let rx = r.right() - 28.0;
        p.circle_filled(Pos2::new(rx - p.layout_no_wrap(status.0.into(), theme::semibold(14.0), c::TEXT).size().x - 14.0, cy - 12.0), 4.5, status.1);
        p.text(Pos2::new(rx, cy - 12.0), Align2::RIGHT_CENTER, status.0, theme::semibold(14.0), c::TEXT);
        p.text(Pos2::new(rx, cy + 12.0), Align2::RIGHT_CENTER, format!("{active} apps · {conns} connections"), theme::regular(13.0), c::MUTED);
    }
}
