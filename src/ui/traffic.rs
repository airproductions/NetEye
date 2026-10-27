//! Traffic Monitor: per-app usage list on the left, live graph / usage bars / hosts on the right.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::app_popup;
use super::motion;
use super::sort::{self, SortSpec};
use super::theme::{self, c, Glyph, Icons};
use super::Action;
use crate::monitor::{Shared, OTHER_KEY};
use crate::store::{self, HOUR, MINUTE};

// Task 9: graph markers, scale, scrubber and pause read the cached aggregate.
use std::sync::Arc;
use super::markers;
use super::traffic_data::{self as td, Agg, AggCache, Query, Sel, Span};

const RANGES: [&str; 5] = ["5 Minutes", "3 Hours", "24 Hours", "Week", "Month"];
const RANGE_CODES: [&str; 5] = ["5m", "3h", "24h", "7d", "30d"];
const ROW_H: f32 = 32.0;
const STRIP_H: f32 = 92.0;
/// No column picked: today's order (total traffic, biggest first). No header shows as active.
const NATURAL: SortSpec = SortSpec { col: u8::MAX, desc: true };

/// A saved column outside the list's three columns (other than the sentinel) falls back to NATURAL.
fn normalize(spec: SortSpec) -> SortSpec {
    if spec.col > 2 { NATURAL } else { spec }
}

/// (app key, display name, [down, up])
type AppRow = (String, String, [u64; 2]);
/// (app key, display name, connection count)
type ConnRow = (String, String, usize);

/// Columns: 0 name, 1 download, 2 upload; NATURAL (and anything else) is down + up.
fn order_apps(rows: &mut [AppRow], spec: SortSpec) {
    sort::apply(
        rows,
        spec,
        |a, b, col| match col {
            0 => sort::cmp_text(&a.1, &b.1),
            1 => a.2[0].cmp(&b.2[0]),
            2 => a.2[1].cmp(&b.2[1]),
            _ => (a.2[0] + a.2[1]).cmp(&(b.2[0] + b.2[1])),
        },
        |r| r.1.as_str(),
    );
}

/// "Connected now" has no byte counts: name for column 0, connection count otherwise.
fn order_connected(rows: &mut [ConnRow], spec: SortSpec) {
    sort::apply(rows, spec, |a, b, col| if col == 0 { sort::cmp_text(&a.1, &b.1) } else { a.2.cmp(&b.2) }, |r| r.1.as_str());
}

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
    /// Graph aggregate for the current query (Task 9).
    graph_cache: AggCache,
    /// Timeline scrubber: steps back from now; 0 = live.
    end_offset: usize,
    /// Paused graph: the snapshot drawn instead of the live aggregate.
    paused: Option<Arc<Agg>>,
    /// Last frame's query, to reset the offset on a span change and resume on any change.
    last_query: Option<Query>,
}

/// Marker lane under the plot.
const LANE_H: f32 = 18.0;
/// Timeline scrubber strip under the graph and Bars views.
const SCRUB_H: f32 = 26.0;
/// Scrubber: room on its right for the Live label / Now chip.
const SCRUB_TAIL: f32 = 60.0;

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

    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, actions: &mut Vec<Action>) {
        // decided with the selection that `series` is built from, so the key and the data change in the same frame
        let ease = self.ease.eases(selection_key(self.range, self.selected.as_deref()), ui.input(|i| i.time));
        let series = self.series(s);
        let full = ui.max_rect();
        let left_w = 470.0f32.min(full.width() * 0.4);
        let body_h = (full.height() - STRIP_H).max(0.0);
        let left = Rect::from_min_size(full.min, Vec2::new(left_w, body_h));
        let right = Rect::from_min_max(Pos2::new(left.right(), full.top()), Pos2::new(full.right(), full.top() + body_h));
        let strip = Rect::from_min_max(Pos2::new(full.left(), full.top() + body_h), full.max);

        ui.painter().rect_filled(left, CornerRadius::ZERO, c::panel());
        ui.painter().line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0_f32, c::border()));
        let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left));
        lui.set_clip_rect(left.intersect(ui.clip_rect()));
        self.app_list(&mut lui, s, icons, &series, actions);
        let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right.shrink2(Vec2::new(24.0, 14.0))));
        self.chart_area(&mut rui, s, icons, &series);
        let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(strip));
        self.strip(&mut fui, s, &series, full.width(), ease);
    }

    fn app_list(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, series: &Series, actions: &mut Vec<Action>) {
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
        let mut spec = normalize(s.store.data.settings.sort_for("traffic", NATURAL));
        let hh = |x0: f32, x1: f32| Rect::from_min_max(Pos2::new(x0, hr.top()), Pos2::new(x1, hr.bottom()));
        let mut changed = false;
        changed |= sort::header(ui, hh(r.left() + 46.0, cd - 100.0), "App", 0, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
        changed |= sort::header(ui, hh(cd - 70.0, cd), "↓", 1, &mut spec, true, NATURAL, Align2::RIGHT_CENTER);
        changed |= sort::header(ui, hh(cu - 70.0, cu), "↑", 2, &mut spec, true, NATURAL, Align2::RIGHT_CENTER);
        if changed {
            s.store.data.settings.set_sort("traffic", spec);
            s.store.mark_dirty();
        }
        ui.painter().line_segment([Pos2::new(r.left(), hr.bottom()), Pos2::new(r.right(), hr.bottom())], Stroke::new(1.0_f32, c::border()));

        let mut apps: Vec<AppRow> = series
            .app_totals
            .iter()
            .filter(|(_, v)| v[0] + v[1] > 0)
            .map(|(k, v)| (k.clone(), s.app_name(k), *v))
            .collect();
        let q = self.search.to_lowercase();
        apps.retain(|(_, n, _)| q.is_empty() || n.to_lowercase().contains(&q));
        order_apps(&mut apps, spec);
        // Without admin there are no per-app byte counts; list connected apps by connection count.
        let mut connected: Vec<ConnRow> = Vec::new();
        if !s.admin {
            for (k, l) in &s.apps_live {
                let n = l.in_conns + l.out_conns;
                let name = s.app_name(k);
                if n > 0 && k != OTHER_KEY && (q.is_empty() || name.to_lowercase().contains(&q)) {
                    connected.push((k.clone(), name, n));
                }
            }
            // no byte counts here: the connection count stands in for both number columns
            order_connected(&mut connected, spec);
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
                    let ir = Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(18.0));
                    app_popup::link(ui, ui.id().with((k, "app-link")), ir, Pos2::new(row.left() + 46.0, row.center().y), name, theme::regular(14.0), cd - row.left() - 120.0, k, actions);
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
                // the name and icon also open the app popup; they select the row first
                if let Some(k) = key.as_deref().filter(|k| *k != OTHER_KEY) {
                    let font = theme::regular(14.0);
                    if app_popup::link(ui, ui.id().with((k, "app-link")), ir, Pos2::new(row.left() + 46.0, row.center().y), &name, font, cd - row.left() - 120.0, k, actions) {
                        self.selected = key.clone();
                    }
                }
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

        // the timeline scrubber sits under the graph and Bars views (Preset spans only)
        let scrub = self.mode < 2;
        let bottom = if scrub { r.bottom() - SCRUB_H - 30.0 } else { r.bottom() - 28.0 };
        let chart = Rect::from_min_max(Pos2::new(r.left(), r.top() + 90.0), Pos2::new(r.right(), bottom));
        let live = self.graph_agg(s);
        match self.mode {
            0 => {
                let view = self.paused.clone().unwrap_or_else(|| live.clone());
                self.graph(ui, s, chart, &view)
            }
            1 => self.bars(ui, chart, series),
            _ => self.hosts(ui, s, chart, series),
        }
        if scrub {
            self.scrubber(ui, Rect::from_min_max(Pos2::new(r.left(), r.bottom() - SCRUB_H), r.max), &live);
        }
    }

    fn time_axis(&self, p: &egui::Painter, chart: Rect, times: &[i64]) {
        let n = times.len();
        if n < 2 {
            return;
        }
        let ticks = ((chart.width() / 130.0) as usize).clamp(2, 12);
        for i in 0..=ticks {
            let idx = (n - 1) * i / ticks;
            let x = chart.left() + chart.width() * idx as f32 / (n - 1) as f32;
            let t = times[idx];
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

    /// The graph's query: the current range and app, plus the scrubber offset. A span change
    /// returns to now, and any change of what's viewed resumes a paused graph.
    fn graph_agg(&mut self, s: &Shared) -> Arc<Agg> {
        let now = s.live.back().map(|l| l.t).unwrap_or_else(store::now);
        let mut q = Query {
            span: Span::Preset(self.range.min(4) as u8),
            group: td::Group::Apps,
            net: td::Net::All,
            dir: td::DirF::Both,
            sel: self.selected.clone().map_or(Sel::All, Sel::App),
            admin: s.admin,
            end_offset: self.end_offset,
        };
        if let Some(old) = &self.last_query {
            if old.span != q.span {
                self.end_offset = 0;
                q.end_offset = 0;
            }
            self.paused = markers::next_pause(self.paused.take(), old, &q);
        }
        self.last_query = Some(q.clone());
        let key = td::cache_key(&q, &s.store.data, &s.live, now);
        self.graph_cache.get(key, || td::aggregate(&s.store.data, &s.live, &q, now))
    }

    /// The saved scale preset for a range.
    fn scale_slot(st: &mut store::Settings, range: usize) -> &mut u8 {
        match range {
            0 => &mut st.graph_scale_5m,
            1 => &mut st.graph_scale_3h,
            2 => &mut st.graph_scale_24h,
            3 => &mut st.graph_scale_7d,
            _ => &mut st.graph_scale_30d,
        }
    }

    /// Small rounded label button for the graph overlays.
    fn chip(ui: &mut egui::Ui, text: &str, color: Color32, tip: &str) -> egui::Response {
        let font = theme::semibold(12.0);
        let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 18.0, 22.0), Sense::click());
        let fill = if resp.hovered() { c::hover() } else { c::raised() };
        ui.painter().rect_filled(r, CornerRadius::same(11), fill);
        ui.painter().galley(Pos2::new(r.left() + 9.0, r.center().y - galley.size().y / 2.0), galley, color);
        resp.on_hover_text(tip)
    }

    fn graph(&mut self, ui: &mut egui::Ui, s: &mut Shared, chart: Rect, agg: &Arc<Agg>) {
        let paused = self.paused.is_some();
        let step = agg.step.max(1) as f64;
        let down: Vec<f64> = agg.down.iter().map(|v| v / step).collect();
        let up: Vec<f64> = agg.up.iter().map(|v| v / step).collect();
        let max = down.iter().chain(up.iter()).cloned().fold(0.0, f64::max);
        // the marker lane sits under the plot; the time axis goes under the lane
        let plot = Rect::from_min_max(chart.min, Pos2::new(chart.right(), chart.bottom() - LANE_H));
        let lane = Rect::from_min_max(Pos2::new(chart.left(), plot.bottom()), chart.max);
        let p = ui.painter_at(chart.expand(30.0));
        let scale_ix = *Self::scale_slot(&mut s.store.data.settings, self.range);
        let fixed = markers::scale_max(scale_ix);
        if max <= 0.0 {
            theme::empty_state(&p, plot.center(), "No traffic in this period. Try another time range.");
            self.time_axis(&p, chart, &agg.times);
            self.draw_markers(ui, s, lane, agg, 0.0);
            self.graph_controls(ui, s, plot, agg, false);
            return;
        }
        let nice = fixed.unwrap_or_else(|| theme::nice_max(max));
        for i in 1..=4 {
            let y = plot.bottom() - plot.height() * i as f32 / 4.0;
            p.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, c::border()));
            p.text(Pos2::new(plot.right() - 4.0, y - 9.0), Align2::RIGHT_CENTER, theme::rate(nice * i as f64 / 4.0), theme::regular(11.5), c::faint());
        }
        p.line_segment([plot.left_bottom(), plot.right_bottom()], Stroke::new(1.0_f32, c::border()));
        // a new live sample slides the series in from the left over NORMAL; not while paused or
        // looking back, and not once the newest sample is stale (e.g. after hiding to the tray)
        let mut shift = 0.0;
        if self.range == 0 && agg.times.len() > 1 && !paused && self.end_offset == 0 {
            let newest = *agg.times.last().unwrap();
            let now = ui.input(|i| i.time);
            match self.glide {
                Some((r, t, _)) if r == self.range && t == newest => {}
                Some((r, t, _)) if r == self.range && (1..=5).contains(&(newest - t)) => self.glide = Some((self.range, newest, now)),
                _ => self.glide = Some((self.range, newest, f64::NEG_INFINITY)),
            }
            if let Some((_, _, started)) = self.glide {
                let t = ((now - started) / motion::NORMAL as f64) as f32;
                let age = (store::now() - newest) as f64;
                if t < 1.0 {
                    // capped at ~30 fps: the glide runs after every live sample, so full-rate frames would cost CPU
                    if let Some(frame) = markers::glide_repaint(paused, age) {
                        shift = plot.width() / (agg.times.len() - 1) as f32 * (1.0 - motion::ease_out(t));
                        ui.ctx().request_repaint_after(frame);
                    }
                }
            }
        } else {
            self.glide = None;
        }
        let sp = ui.painter_at(plot);
        let moved = plot.translate(Vec2::new(shift, 0.0));
        theme::area(&sp, moved, &down, nice, c::down());
        theme::area(&sp, moved, &up, nice, c::up());
        self.time_axis(&p, chart, &agg.times);
        self.draw_markers(ui, s, lane, agg, shift);

        let resp = ui.interact(plot, ui.id().with("graph"), Sense::hover());
        if let Some(pos) = resp.hover_pos() {
            let n = agg.times.len();
            let idx = (((pos.x - plot.left()) / plot.width()) * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32) as usize;
            let x = plot.left() + plot.width() * idx as f32 / (n - 1) as f32;
            p.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0_f32, c::muted()));
            for (vals, col) in [(&down, c::down()), (&up, c::up())] {
                let y = plot.bottom() - (vals[idx] / nice).clamp(0.0, 1.0) as f32 * plot.height();
                p.circle(Pos2::new(x, y), 4.0, col, Stroke::new(2.0_f32, c::bg()));
            }
            let t = agg.times[idx];
            let (d, u) = (down[idx], up[idx]);
            let range = self.range;
            let names: Vec<(String, [u64; 2])> = agg.step_top.get(idx).map(|v| v.iter().take(4).map(|(k, v)| (s.app_name(k), *v)).collect()).unwrap_or_default();
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
        self.graph_controls(ui, s, plot, agg, fixed.is_some() && max > nice);
    }

    /// Plot overlays: the scale menu (top-left, with the clipped note) and the pause button with
    /// its chip (top-right). Drawn after the plot so they take the pointer first.
    fn graph_controls(&mut self, ui: &mut egui::Ui, s: &mut Shared, plot: Rect, agg: &Arc<Agg>, clipped: bool) {
        let bar = Rect::from_min_size(plot.min + Vec2::new(4.0, 4.0), Vec2::new((plot.width() - 8.0).max(0.0), 24.0));
        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let labels: Vec<String> = markers::SCALE_PRESETS.iter().map(|v| if *v > 0.0 { theme::rate(*v) } else { "Auto".to_string() }).collect();
        let items: Vec<&str> = labels.iter().map(String::as_str).collect();
        let slot = Self::scale_slot(&mut s.store.data.settings, self.range);
        let cur = usize::from(*slot).min(items.len() - 1);
        if let Some(i) = theme::menu(&mut left, "graph-scale", items[cur], &items, cur, &[], "") {
            if i != cur {
                *slot = i as u8;
                s.store.mark_dirty();
            }
        }
        if clipped {
            left.add_space(8.0);
            theme::label(&mut left, "▲ clipped", theme::semibold(11.5), c::warn()).on_hover_text("Traffic is above the chosen scale");
        }
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let paused = self.paused.is_some();
        let (g, tip) = if paused { (Glyph::Play, "Resume the graph") } else { (Glyph::Pause, "Pause the graph") };
        if theme::icon_button(&mut right, g, 24.0, tip).clicked() {
            self.paused = if paused { None } else { Some(agg.clone()) };
        }
        if paused {
            right.add_space(4.0);
            if Self::chip(&mut right, "Paused", c::warn(), "The graph is frozen; click to resume").clicked() {
                self.paused = None;
            }
        }
    }

    /// Clock markers for Important events in the lane under the plot; neighbours closer than
    /// 14 px merge, with a count badge. Hover lists up to 5 events.
    fn draw_markers(&self, ui: &mut egui::Ui, s: &Shared, lane: Rect, agg: &Agg, shift: f32) {
        let (Some(&t0), Some(&t1)) = (agg.times.first(), agg.times.last()) else { return };
        let step = agg.step.max(1);
        let sel = self.selected.clone().map_or(Sel::All, Sel::App);
        let data = &s.store.data;
        let picked = markers::pick_events(&data.events, &sel, |k| data.apps.get(k).and_then(|r| r.publisher.clone()));
        if picked.is_empty() {
            return;
        }
        // events sit on their step's start, the same x the series uses for that step
        let pts: Vec<(i64, u64)> = picked.iter().map(|e| (e.t - (e.t - t0).rem_euclid(step), e.id)).collect();
        let p = ui.painter_at(lane);
        let cy = lane.center().y;
        for cl in markers::clusters(&pts, t0, t1, lane.left() + shift, lane.width(), markers::MIN_GAP) {
            let r = Rect::from_center_size(Pos2::new(cl.x, cy), Vec2::splat(13.0));
            theme::glyph(&p, r, Glyph::Clock, c::accent());
            let n = cl.events.len();
            if n > 1 {
                let bc = Pos2::new(r.right() + 1.0, r.top() + 1.0);
                p.circle_filled(bc, 6.0, c::accent());
                p.text(bc, Align2::CENTER_CENTER, if n > 9 { "9+".to_string() } else { n.to_string() }, theme::bold(8.5), Color32::WHITE);
            }
            let Some(first) = cl.events.first() else { continue };
            let resp = ui.interact(r.expand(3.0).intersect(lane), ui.id().with(("marker", *first)), Sense::hover());
            if resp.hovered() {
                let range = self.range;
                let lines: Vec<(String, String)> = cl
                    .events
                    .iter()
                    .rev()
                    .take(5)
                    .filter_map(|id| picked.iter().find(|e| e.id == *id))
                    .map(|e| {
                        let when = if range == 0 { theme::local(e.t).format("%-I:%M:%S %p").to_string() } else if range <= 2 { theme::clock(e.t) } else { theme::date_time(e.t) };
                        let app = e.app.as_deref().map(|k| s.app_name(k)).unwrap_or_default();
                        (when, super::log::event_text(e, &app))
                    })
                    .collect();
                let more = n.saturating_sub(5);
                resp.on_hover_ui(|ui| {
                    ui.set_max_width(360.0);
                    for (when, msg) in lines {
                        theme::label(ui, when, theme::semibold(12.5), c::muted());
                        theme::label(ui, msg, theme::regular(13.0), c::text());
                    }
                    if more > 0 {
                        theme::label(ui, format!("and {more} more"), theme::regular(12.5), c::muted());
                    }
                });
            }
        }
    }

    /// Timeline scrubber: a faint mini chart of the whole stored history with a handle at the end
    /// of the visible window. Dragging pans back in time; the Now chip returns to live.
    fn scrubber(&mut self, ui: &mut egui::Ui, rect: Rect, agg: &Agg) {
        let bar = Rect::from_min_max(rect.min, Pos2::new(rect.right() - SCRUB_TAIL, rect.bottom()));
        let p = ui.painter_at(rect);
        p.rect_filled(bar, CornerRadius::same(4), c::panel());
        let window = agg.times.len();
        let max_offset = agg.max_offset;
        let h = &agg.history;
        if h.len() >= 2 && bar.width() > 4.0 {
            // one value per ~2 px (the peak of each bin), so long histories stay cheap to draw
            let bins = ((bar.width() / 2.0) as usize).clamp(2, h.len());
            let vals: Vec<f64> = (0..bins).map(|b| h[b * h.len() / bins..((b + 1) * h.len() / bins).max(b * h.len() / bins + 1)].iter().map(|x| x.1).fold(0.0, f64::max)).collect();
            let peak = vals.iter().cloned().fold(0.0, f64::max);
            theme::area(&p, bar.shrink2(Vec2::new(0.0, 3.0)), &vals, peak, c::faint());
            let fmt = |t: i64| match self.range {
                0 => theme::local(t).format("%-I:%M:%S").to_string(),
                1 | 2 => theme::clock(t),
                _ => theme::local(t).format("%-d %b").to_string(),
            };
            p.text(Pos2::new(bar.left() + 6.0, bar.center().y), Align2::LEFT_CENTER, fmt(h[0].0), theme::regular(10.5), c::muted());
            p.text(Pos2::new(bar.right() - 6.0, bar.center().y), Align2::RIGHT_CENTER, fmt(h[h.len() - 1].0), theme::regular(10.5), c::muted());
        }
        // the visible window and its end handle
        let off = self.end_offset.min(max_offset);
        let hx = markers::x_from_offset(off, bar, max_offset, window);
        let total = (max_offset + window).max(1) as f32;
        let wx = (hx - bar.width() * window as f32 / total).max(bar.left());
        p.rect_filled(Rect::from_min_max(Pos2::new(wx, bar.top()), Pos2::new(hx, bar.bottom())), CornerRadius::ZERO, theme::with_alpha(c::accent(), 28));
        p.line_segment([Pos2::new(hx, bar.top()), Pos2::new(hx, bar.bottom())], Stroke::new(2.0_f32, c::accent()));
        p.rect_filled(Rect::from_center_size(Pos2::new(hx, bar.center().y), Vec2::new(6.0, 14.0)), CornerRadius::same(3), c::accent());

        let resp = ui.interact(bar, ui.id().with("scrubber"), Sense::click_and_drag());
        if max_offset > 0 {
            let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text("Drag to look back in time");
            if resp.clicked() || resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.end_offset = markers::offset_from_x(pos.x, bar, max_offset, window);
                }
            }
        }
        let tail = Rect::from_min_max(Pos2::new(bar.right() + 8.0, rect.top()), rect.max);
        let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(tail).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if self.end_offset > 0 {
            if Self::chip(&mut tui, "Now", c::accent(), "Back to live").clicked() {
                self.end_offset = 0;
            }
        } else {
            theme::label(&mut tui, "Live", theme::semibold(12.0), c::muted());
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
        ui.painter().rect_filled(r, CornerRadius::ZERO, c::panel());
        ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, c::border()));

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
        let counts_w = if show_counts { 210.0 } else { 100.0 };
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

        // online status at the strip's right end, with the app and connection counts under it when there is room
        divider(r.right() - counts_w - 12.0);
        let conns: usize = s.apps_live.values().map(|a| a.in_conns + a.out_conns).sum();
        let active = s.apps_live.values().filter(|a| a.in_conns + a.out_conns > 0).count();
        let status = match s.internet {
            Some(true) => ("Online", c::good()),
            Some(false) => ("Offline", c::bad()),
            None => ("Checking…", c::muted()),
        };
        let rx = r.right() - 22.0;
        let sy = if show_counts { cy - 12.0 } else { cy };
        let sw = p.layout_no_wrap(status.0.into(), theme::semibold(14.0), c::text()).size().x;
        p.circle_filled(Pos2::new(rx - sw - 12.0, sy), 4.5, status.1);
        p.text(Pos2::new(rx, sy), Align2::RIGHT_CENTER, status.0, theme::semibold(14.0), c::text());
        if show_counts {
            p.text(Pos2::new(rx, cy + 12.0), Align2::RIGHT_CENTER, format!("{active} apps · {conns} connections"), theme::regular(13.0), c::muted());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::sort::SortSpec;

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

    fn row(n: &str, d: u64, u: u64) -> AppRow {
        (format!("{n}.exe"), n.to_string(), [d, u])
    }

    fn rows() -> Vec<AppRow> {
        vec![row("b", 10, 1), row("A", 5, 9), row("c", 10, 3)]
    }

    fn names(r: &[AppRow]) -> Vec<&str> {
        r.iter().map(|x| x.1.as_str()).collect()
    }

    #[test]
    fn traffic_rows_sort() {
        let mut r = rows();
        order_apps(&mut r, SortSpec { col: 1, desc: true });
        assert_eq!(names(&r), ["b", "c", "A"]);
        order_apps(&mut r, SortSpec { col: 0, desc: false });
        assert_eq!(names(&r), ["A", "b", "c"]);
        order_apps(&mut r, SortSpec { col: 2, desc: true });
        assert_eq!(names(&r), ["A", "c", "b"]);
    }

    #[test]
    fn traffic_natural_default_is_total() {
        let mut r = vec![row("a", 10, 1), row("b", 2, 20), row("c", 5, 5)];
        order_apps(&mut r, NATURAL);
        assert_eq!(names(&r), ["b", "a", "c"]);
    }

    #[test]
    fn traffic_stale_col_resets_to_natural() {
        assert_eq!(normalize(SortSpec { col: 7, desc: false }), NATURAL);
        assert_eq!(normalize(NATURAL), NATURAL);
        assert_eq!(normalize(SortSpec { col: 2, desc: false }), SortSpec { col: 2, desc: false });
    }

    #[test]
    fn traffic_connected_order() {
        let mk = |n: &str, c: usize| (format!("{n}.exe"), n.to_string(), c);
        let mut r = vec![mk("b", 2), mk("a", 2), mk("z", 5)];
        order_connected(&mut r, NATURAL);
        assert_eq!(r.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["z", "a", "b"]);
        order_connected(&mut r, SortSpec { col: 0, desc: false });
        assert_eq!(r.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["a", "b", "z"]);
    }
}
