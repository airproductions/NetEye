//! Traffic Monitor: grouped usage list on the left, live graph / usage bars / hosts on the right.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Datelike;
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::app_popup;
use super::csv_export;
use super::flags;
use super::markers;
use super::motion;
use super::sort::{self, SortSpec};
use super::theme::{self, c, Glyph, Icons, MenuItem};
use super::traffic_data::{self as td, Agg, AggCache, DirF, Group, Net, Query, Row, Sel, Span};
use super::usage_view;
use super::Action;
use crate::monitor::{Shared, OTHER_KEY};
use crate::ports;
use crate::store::{self, Data, Settings, HOUR, MINUTE};

const ROW_H: f32 = 32.0;
const STRIP_H: f32 = 92.0;
const DAY: i64 = 24 * HOUR;
/// No column picked: total traffic, biggest first. No header shows as active.
const NATURAL: SortSpec = SortSpec { col: u8::MAX, desc: true };
const GROUP_ITEMS: [&str; 4] = ["Apps", "Publishers", "Traffic type", "Countries"];
const NET_ITEMS: [&str; 3] = ["WAN & LAN", "LAN", "WAN"];
const DIR_ITEMS: [&str; 3] = ["Incoming & Outgoing", "Incoming only", "Outgoing only"];
const ADMIN_TIP: &str = "Needs administrator mode";
const PRESET_LABELS: [&str; 5] = ["5 Minutes", "3 Hours", "24 Hours", "Last 7 Days", "Last 30 Days"];
const DASH: &str = "—";

/// A saved column outside the list's three columns (other than the sentinel) falls back to NATURAL.
fn normalize(spec: SortSpec) -> SortSpec {
    if spec.col > 2 { NATURAL } else { spec }
}

/// A sort on the number column the Direction filter hides shows the natural order instead.
fn shown_spec(spec: SortSpec, dir: DirF) -> SortSpec {
    match (spec.col, dir) {
        (1, DirF::Out) | (2, DirF::In) => NATURAL,
        _ => spec,
    }
}

/// (row key, display name, [down, up])
type AppRow = (String, String, [u64; 2]);
/// (app key, display name, connection count, current [down, up] speed)
type ConnRow = (String, String, usize, [u64; 2]);

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

/// "Connected now": name for column 0. In administrator mode the download and upload columns
/// sort by the speed they show; otherwise (and for NATURAL) by connection count.
fn order_connected(rows: &mut [ConnRow], spec: SortSpec, admin: bool) {
    sort::apply(
        rows,
        spec,
        |a, b, col| match col {
            0 => sort::cmp_text(&a.1, &b.1),
            1 if admin => a.3[0].cmp(&b.3[0]),
            2 if admin => a.3[1].cmp(&b.3[1]),
            _ => a.2.cmp(&b.2),
        },
        |r| r.1.as_str(),
    );
}

/// Group by, Network and Direction as shown: without administrator rights the admin-only saved
/// values show as Apps and WAN & LAN, and the saved values are left alone.
fn view_filters(set: &Settings, admin: bool) -> (Group, Net, DirF) {
    let (g, n) = td::effective(Group::from_u8(set.traffic_group), Net::from_u8(set.traffic_net), admin);
    (g, n, DirF::from_u8(set.traffic_dir))
}

/// Group by entries that need administrator mode (everything but Apps).
fn group_disabled(admin: bool) -> [bool; 4] {
    [false, !admin, !admin, !admin]
}

/// Network entries that need administrator mode (LAN only and WAN only).
fn net_disabled(admin: bool) -> [bool; 3] {
    [false, !admin, !admin]
}

/// Saved sort per Group by mode.
fn sort_key(g: Group) -> &'static str {
    match g {
        Group::Apps => "traffic",
        Group::Publishers => "traffic.publisher",
        Group::Types => "traffic.type",
        Group::Countries => "traffic.country",
    }
}

fn name_header(g: Group) -> &'static str {
    match g {
        Group::Apps => "App",
        Group::Publishers => "Publisher",
        Group::Types => "Type",
        Group::Countries => "Country",
    }
}

/// The selection a list row stands for.
fn row_sel(g: Group, key: &str) -> Sel {
    if g == Group::Apps { Sel::App(key.to_string()) } else { Sel::Group(g, key.to_string()) }
}

/// Whether a selection belongs to the list being shown.
fn sel_fits(sel: &Sel, g: Group) -> bool {
    match sel {
        Sel::All => true,
        Sel::App(_) => g == Group::Apps,
        Sel::Group(sg, _) => *sg == g && g != Group::Apps,
    }
}

/// Number columns as (column, header, side): both, or only the one the Direction filter keeps.
fn num_columns(dir: DirF) -> &'static [(u8, &'static str)] {
    match dir {
        DirF::Both => &[(1, "↓"), (2, "↑")],
        DirF::In => &[(1, "↓")],
        DirF::Out => &[(2, "↑")],
    }
}

/// Strip [↓, ↑]: true where the Direction filter excludes that side.
fn strip_dim(dir: DirF) -> [bool; 2] {
    [dir == DirF::Out, dir == DirF::In]
}

/// Strip [Internet, Local network]: true where the Network filter excludes that row.
fn net_dim(net: Net) -> [bool; 2] {
    [net == Net::Lan, net == Net::Wan]
}

/// Strip total and rate text for one side; an excluded side shows a dash.
fn strip_texts(bytes: u64, rate: f64, dimmed: bool) -> [String; 2] {
    if dimmed { [DASH.to_string(), DASH.to_string()] } else { [theme::bytes(bytes), theme::rate(rate)] }
}

/// "Connected now" number cells: the app's current speeds, or dashes without per-app bytes.
fn connected_cells(rate: [u64; 2], admin: bool) -> [String; 2] {
    if admin { [theme::rate(rate[0] as f64), theme::rate(rate[1] as f64)] } else { [DASH.to_string(), DASH.to_string()] }
}

/// Whether picking time-menu entry `i` (while `cur` is shown) sets a new span. Day, Week and
/// Month (5-7) always do, so picking the kind already shown snaps back to the current period.
fn span_pick_applies(i: usize, cur: usize) -> bool {
    i != cur || (5..=7).contains(&i)
}

/// Time menu button label.
fn span_label(span: Span) -> &'static str {
    match span {
        Span::Preset(p) => PRESET_LABELS[usize::from(p.min(4))],
        Span::Day(_) => "Day",
        Span::Week(_) => "Week",
        Span::Month(_) => "Month",
        Span::AllTime => "All Time",
        Span::Custom(..) => "Custom",
    }
}

/// Time menu entry of a span: presets 0-4, Day 5, Week 6, Month 7, All Time 8, Custom 9.
fn span_index(span: Span) -> usize {
    match span {
        Span::Preset(p) => usize::from(p.min(4)),
        Span::Day(_) => 5,
        Span::Week(_) => 6,
        Span::Month(_) => 7,
        Span::AllTime => 8,
        Span::Custom(..) => 9,
    }
}

/// The midnight before a given midnight (DST-safe: lands mid-day, then snaps).
fn day_back(midnight: i64) -> i64 {
    td::local_midnight(midnight - 12 * HOUR)
}

/// The midnight after a given midnight.
fn day_fwd(midnight: i64) -> i64 {
    td::local_midnight(midnight + 36 * HOUR)
}

/// Local midnight of the oldest stored bucket, if any.
fn oldest_midnight(data: &Data) -> Option<i64> {
    [&data.minutes, &data.hours].iter().filter_map(|l| l.front().map(|b| b.t)).min().map(td::local_midnight)
}

/// The span a time menu entry opens: calendar periods start at the current one; Custom starts
/// as the last 7 days, clipped to the stored history.
fn span_for_index(i: usize, now: i64, data: &Data) -> Span {
    let today = td::local_midnight(now);
    let date = theme::local(now).date_naive();
    let back = |days: i64| td::local_midnight(today - days * DAY + 12 * HOUR);
    match i {
        0..=4 => Span::Preset(i as u8),
        5 => Span::Day(today),
        6 => Span::Week(back(i64::from(date.weekday().num_days_from_monday()))),
        7 => Span::Month(back(i64::from(date.day0()))),
        8 => Span::AllTime,
        _ => {
            let from = oldest_midnight(data).map_or(back(6), |o| back(6).max(o).min(today));
            Span::Custom(from, day_fwd(today))
        }
    }
}

/// One Custom stepper click: moves From (`to_side` false) or To by a day. From stays on or after
/// the oldest stored day and before To; To (exclusive, shown as the day before it) stays after
/// From and no later than tomorrow's midnight.
fn custom_step(from: i64, to: i64, to_side: bool, dir: i8, oldest: i64, today: i64) -> (i64, i64) {
    if !to_side {
        let f = if dir < 0 { day_back(from) } else { day_fwd(from) };
        if f >= oldest && f < to { (f, to) } else { (from, to) }
    } else {
        let t = if dir < 0 { day_back(to) } else { day_fwd(to) };
        if t > from && t <= day_fwd(today) { (from, t) } else { (from, to) }
    }
}

/// Stepper text for a Day, Week or Month span.
fn period_label(span: Span) -> String {
    match span {
        Span::Day(s) => theme::local(s).format("%a %-d %b").to_string(),
        Span::Week(s) => format!("Week of {}", theme::local(s).format("%-d %b")),
        Span::Month(s) => theme::local(s).format("%B %Y").to_string(),
        _ => String::new(),
    }
}

/// Time axis label for a sample at `t` in a window of `len` seconds.
fn tick_label(step: i64, len: i64, t: i64) -> String {
    if step < MINUTE {
        theme::local(t).format("%-I:%M:%S").to_string()
    } else if step < HOUR {
        theme::clock(t)
    } else if len <= 8 * DAY {
        theme::local(t).format("%a %-d").to_string()
    } else {
        theme::local(t).format("%-d %b").to_string()
    }
}

/// Graph tooltip heading for the step at `t`.
fn tip_when(step: i64, t: i64) -> String {
    if step < MINUTE {
        theme::local(t).format("%-I:%M:%S %p").to_string()
    } else if step < HOUR {
        theme::clock(t)
    } else {
        theme::local(t).format("%a %-d %b, %-I %p").to_string()
    }
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

/// Identifies what the strip is showing; a change means the user picked another span, row or filter.
fn selection_key(span: Span, sel: &Sel, filters: (Group, Net, DirF)) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (span, sel, filters).hash(&mut h);
    h.finish()
}

/// Below this page width the strip drops the apps/connections section instead of squeezing it.
fn strip_shows_counts(width: f32) -> bool {
    width >= 1180.0
}

/// Icon for a list row (or the chart title): app icon, publisher's largest app, type glyph or flag.
#[allow(clippy::too_many_arguments)]
fn row_icon(ui: &egui::Ui, s: &mut Shared, icons: &mut Icons, group: Group, key: &str, label: &str, icon_key: Option<&str>, ir: Rect) {
    if key == OTHER_KEY && matches!(group, Group::Apps | Group::Publishers) {
        return theme::glyph(ui.painter(), ir, Glyph::Globe, c::muted());
    }
    match group {
        Group::Apps => icons.draw(ui, s, key, ir, label),
        Group::Publishers => match icon_key {
            Some(k) => icons.draw(ui, s, k, ir, label),
            None => theme::fallback_icon(ui.painter(), ir, label),
        },
        Group::Types => theme::glyph(ui.painter(), ir, theme::cat_glyph(ports::category(key)), c::accent()),
        Group::Countries if key == ports::UNCLASSIFIED => theme::glyph(ui.painter(), ir, Glyph::Globe, c::muted()),
        Group::Countries => flags::draw(ui, Rect::from_center_size(ir.center(), flags::SIZE), key),
    }
}

/// `◀ label ▶` stepper; returns -1 or 1 when an enabled arrow is clicked.
fn stepper(ui: &mut egui::Ui, id: &str, label: &str, can_prev: bool, can_next: bool) -> Option<i8> {
    const AW: f32 = 24.0;
    let font = theme::semibold(12.5);
    let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), c::text()).size().x;
    let (r, _) = ui.allocate_exact_size(Vec2::new(tw + 2.0 * AW + 8.0, 28.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(8), c::raised());
    let mut out = None;
    let arrows = [(-1i8, Rect::from_min_size(r.min, Vec2::new(AW, r.height())), can_prev, "Earlier"), (1, Rect::from_min_size(Pos2::new(r.right() - AW, r.top()), Vec2::new(AW, r.height())), can_next, "Later")];
    for (dir, ar, can, tip) in arrows {
        let resp = ui.interact(ar, ui.id().with((id, dir)), if can { Sense::click() } else { Sense::hover() });
        let t = motion::hover(ui, resp.id, can && resp.hovered());
        if t > 0.0 {
            ui.painter().rect_filled(ar, CornerRadius::same(8), theme::lerp_color(Color32::TRANSPARENT, c::hover(), t));
        }
        let col = if can { theme::lerp_color(c::muted(), c::text(), t) } else { c::faint() };
        let (cx, cy) = (ar.center().x, ar.center().y);
        let tip_x = cx + 3.5 * f32::from(dir);
        let back_x = cx - 3.5 * f32::from(dir);
        ui.painter().add(egui::Shape::convex_polygon(vec![Pos2::new(tip_x, cy), Pos2::new(back_x, cy - 4.5), Pos2::new(back_x, cy + 4.5)], col, Stroke::NONE));
        let resp = if can { resp.on_hover_text(tip) } else { resp };
        if can && resp.clicked() {
            out = Some(dir);
        }
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, c::text());
    out
}

/// The saved graph scale for a span: each preset has its own; calendar and longer spans share
/// the slot of the preset with the same step (Day → 24 Hours, Week → Week, longer → Month).
fn scale_slot(st: &mut Settings, span: Span) -> &mut u8 {
    let ix = match span {
        Span::Preset(i) => i,
        Span::Day(_) => 2,
        Span::Week(_) => 3,
        _ => 4,
    };
    match ix {
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

pub struct TrafficPage {
    span: Span,
    mode: usize,
    sel: Sel,
    /// Steps back from now (0 = live). Changing the span resets it.
    end_offset: usize,
    search: String,
    show_search: bool,
    /// (newest sample time, ctx time it arrived) for the 5-minute glide; `None` off the live view.
    glide: Option<(i64, f64)>,
    /// Strip numbers, gauge and bars ease only right after a selection change; data ticks snap.
    ease: motion::EaseGate,
    cache: AggCache,
    /// Paused graph: the snapshot drawn instead of the live aggregate.
    paused: Option<Arc<Agg>>,
    /// Last frame's query: any change of what's viewed resumes a paused graph.
    last_query: Option<Query>,
}

/// Marker lane under the plot.
const LANE_H: f32 = 18.0;
/// Timeline scrubber strip under the graph and Bars views.
const SCRUB_H: f32 = 26.0;
/// Scrubber: room on its right for the Live label / Now chip.
const SCRUB_TAIL: f32 = 60.0;

impl Default for TrafficPage {
    fn default() -> Self {
        Self {
            span: Span::Preset(0),
            mode: 0,
            sel: Sel::All,
            end_offset: 0,
            search: String::new(),
            show_search: false,
            glide: None,
            ease: Default::default(),
            cache: Default::default(),
            paused: None,
            last_query: None,
        }
    }
}

impl TrafficPage {
    /// Writes a new Group by; a change resets the selection to All traffic. Returns true when the
    /// setting changed (the caller marks the store dirty).
    fn pick_group(&mut self, set: &mut Settings, g: Group) -> bool {
        let v = g.to_u8();
        if set.traffic_group == v {
            return false;
        }
        set.traffic_group = v;
        self.sel = Sel::All;
        true
    }

    fn set_span(&mut self, span: Span) {
        self.span = span;
        self.end_offset = 0;
        self.paused = None;
    }

    /// The cached aggregate for the current span, filters, selection and scrubber offset: the one
    /// query the list, strip, graph, Usage and Bars all read. Any change resumes a paused graph.
    fn agg(&mut self, s: &Shared, filters: (Group, Net, DirF)) -> Arc<Agg> {
        let (group, net, dir) = filters;
        let q = Query { span: self.span, group, net, dir, sel: self.sel.clone(), admin: s.admin, end_offset: self.end_offset };
        if let Some(old) = &self.last_query {
            self.paused = markers::next_pause(self.paused.take(), old, &q);
        }
        self.last_query = Some(q.clone());
        let now = s.live.back().map(|l| l.t).unwrap_or_else(store::now);
        let key = td::cache_key(&q, &s.store.data, &s.live, now);
        self.cache.get(key, || td::aggregate(&s.store.data, &s.live, &q, now))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, actions: &mut Vec<Action>) {
        let filters = view_filters(&s.store.data.settings, s.admin);
        if !sel_fits(&self.sel, filters.0) {
            self.sel = Sel::All;
        }
        // decided with the selection the aggregate is built from, so the key and the data change in the same frame
        let ease = self.ease.eases(selection_key(self.span, &self.sel, filters), ui.input(|i| i.time));
        let agg = self.agg(s, filters);
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
        self.app_list(&mut lui, s, icons, &agg, filters, actions);
        let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right.shrink2(Vec2::new(24.0, 14.0))));
        self.chart_area(&mut rui, s, icons, &agg, filters, actions);
        let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(strip));
        self.strip(&mut fui, s, &agg, filters, full.width(), ease, actions);
    }

    /// Group by, Network and Direction menus at the top of the list.
    fn filter_menus(&mut self, ui: &mut egui::Ui, s: &mut Shared, filters: (Group, Net, DirF)) {
        let (group, net, dir) = filters;
        let admin = s.admin;
        let set = &mut s.store.data.settings;
        let mut dirty = false;
        // Picking the entry already shown writes nothing, so a saved admin-only value that limited
        // mode shows as Apps / WAN & LAN survives.
        let gi = usize::from(group.to_u8());
        if let Some(i) = theme::menu(ui, "traffic-group", GROUP_ITEMS[gi], &GROUP_ITEMS, gi, &group_disabled(admin), ADMIN_TIP).filter(|i| *i != gi) {
            dirty |= self.pick_group(set, Group::from_u8(i as u8));
        }
        let ni = usize::from(net.to_u8());
        if let Some(i) = theme::menu(ui, "traffic-net", NET_ITEMS[ni], &NET_ITEMS, ni, &net_disabled(admin), ADMIN_TIP).filter(|i| *i != ni) {
            if set.traffic_net != i as u8 {
                set.traffic_net = i as u8;
                dirty = true;
            }
        }
        let di = usize::from(dir.to_u8());
        if let Some(i) = theme::menu(ui, "traffic-dir", "⇅", &DIR_ITEMS, di, &[false; 3], "") {
            if set.traffic_dir != i as u8 {
                set.traffic_dir = i as u8;
                dirty = true;
            }
        }
        if dirty {
            s.store.mark_dirty();
        }
    }

    fn app_list(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, agg: &Agg, filters: (Group, Net, DirF), actions: &mut Vec<Action>) {
        let (group, _, dir) = filters;
        let r = ui.max_rect();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            if self.show_search {
                let resp = theme::search_box(ui, &mut self.search, r.width() - 80.0, "Search");
                resp.request_focus();
                if theme::icon_button(ui, Glyph::Search, 30.0, "Close search").clicked() {
                    self.show_search = false;
                    self.search.clear();
                }
            } else {
                ui.spacing_mut().item_spacing.x = 6.0;
                self.filter_menus(ui, s, filters);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                    if theme::icon_button(ui, Glyph::Search, 30.0, "Search the list").clicked() {
                        self.show_search = true;
                    }
                });
            }
        });
        ui.add_space(8.0);

        // number columns: ↓ then ↑, or only the one the Direction filter keeps, at the right edge
        let (cd, cu) = (r.right() - 88.0, r.right() - 14.0);
        let cols = num_columns(dir);
        let col_x = |i: usize| if cols.len() == 2 && i == 0 { cd } else { cu };
        let first_x = col_x(0);
        let text_w = first_x - r.left() - 120.0;
        let (hr, _) = ui.allocate_exact_size(Vec2::new(r.width(), 28.0), Sense::hover());
        let key = sort_key(group);
        let mut spec = shown_spec(normalize(s.store.data.settings.sort_for(key, NATURAL)), dir);
        let hh = |x0: f32, x1: f32| Rect::from_min_max(Pos2::new(x0, hr.top()), Pos2::new(x1, hr.bottom()));
        let mut changed = sort::header(ui, hh(r.left() + 46.0, first_x - 100.0), name_header(group), 0, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
        for (i, (col, label)) in cols.iter().enumerate() {
            let x = col_x(i);
            changed |= sort::header(ui, hh(x - 70.0, x), label, *col, &mut spec, true, NATURAL, Align2::RIGHT_CENTER);
        }
        if changed {
            s.store.data.settings.set_sort(key, spec);
            s.store.mark_dirty();
        }
        ui.painter().line_segment([Pos2::new(r.left(), hr.bottom()), Pos2::new(r.right(), hr.bottom())], Stroke::new(1.0_f32, c::border()));

        let by_key: HashMap<&str, &Row> = agg.rows.iter().map(|r| (r.key.as_str(), r)).collect();
        let q = self.search.to_lowercase();
        // limited mode: `__other` is every byte, the same as the pinned row
        let mut rows: Vec<AppRow> = agg
            .rows
            .iter()
            .filter(|r| s.admin || r.key != OTHER_KEY)
            .map(|r| (r.key.clone(), if group == Group::Apps { s.app_name(&r.key) } else { r.label.clone() }, [r.down, r.up]))
            .filter(|(_, n, _)| q.is_empty() || n.to_lowercase().contains(&q))
            .collect();
        order_apps(&mut rows, spec);
        // Apps mode, both modes: every app with live connections now and its current speed
        // (dashes in limited mode, which has no per-app bytes). Range rows are totals, so an app
        // may appear in both.
        let mut connected: Vec<ConnRow> = Vec::new();
        if group == Group::Apps {
            for (k, l) in &s.apps_live {
                let n = l.in_conns + l.out_conns;
                let name = s.app_name(k);
                if n > 0 && k != OTHER_KEY && (q.is_empty() || name.to_lowercase().contains(&q)) {
                    connected.push((k.clone(), name, n, l.rate));
                }
            }
            order_connected(&mut connected, spec, s.admin);
        }
        // the pinned row is every list row added up, whatever is selected
        let all = agg.rows.iter().fold([0u64, 0u64], |a, r| [a[0] + r.down, a[1] + r.up]);

        let extra = if connected.is_empty() { 0 } else { connected.len() + 1 };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len() + 1 + extra, |ui, range| {
            for i in range {
                if i > rows.len() {
                    let j = i - rows.len() - 1;
                    let (row, resp) = ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::hover());
                    if j == 0 {
                        ui.painter().text(Pos2::new(row.left() + 16.0, row.center().y + 4.0), Align2::LEFT_CENTER, "CONNECTED NOW", theme::semibold(11.5), c::muted());
                        continue;
                    }
                    let (k, name, n, rate) = &connected[j - 1];
                    if resp.hovered() {
                        ui.painter().rect_filled(row, CornerRadius::ZERO, c::hover());
                    }
                    let ir = Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(18.0));
                    icons.draw(ui, s, k, ir, name);
                    let p = ui.painter();
                    theme::text(p, Pos2::new(row.left() + 46.0, row.center().y), name, theme::regular(14.0), c::text(), text_w);
                    let cells = connected_cells(*rate, s.admin);
                    for (ci, (col, _)) in cols.iter().enumerate() {
                        theme::text_right(p, Pos2::new(col_x(ci), row.center().y), &cells[usize::from(*col) - 1], theme::regular(13.0), c::muted());
                    }
                    app_popup::link(ui, ui.id().with((k, "app-link")), ir, Pos2::new(row.left() + 46.0, row.center().y), name, theme::regular(14.0), text_w, k, actions);
                    resp.on_hover_text(format!("{n} connection{}", if *n == 1 { "" } else { "s" }));
                    continue;
                }
                let (key, name, v) = if i == 0 { (None, "All traffic".to_string(), all) } else {
                    let (k, n, v) = &rows[i - 1];
                    (Some(k.clone()), n.clone(), *v)
                };
                let (row, resp) = ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::click());
                let this = key.as_deref().map_or(Sel::All, |k| row_sel(group, k));
                let p = ui.painter();
                if self.sel == this {
                    p.rect_filled(row, CornerRadius::ZERO, c::selected());
                } else if resp.hovered() {
                    p.rect_filled(row, CornerRadius::ZERO, c::hover());
                } else if i % 2 == 1 {
                    p.rect_filled(row, CornerRadius::ZERO, c::row_alt());
                }
                let ir = Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(18.0));
                match &key {
                    None => theme::glyph(p, ir, Glyph::Grid, c::accent()),
                    Some(k) => {
                        let icon_key = by_key.get(k.as_str()).and_then(|r| r.icon_key.as_deref());
                        row_icon(ui, s, icons, group, k, &name, icon_key, ir);
                    }
                }
                let p = ui.painter();
                theme::text(p, Pos2::new(row.left() + 46.0, row.center().y), &name, if i == 0 { theme::semibold(14.0) } else { theme::regular(14.0) }, c::text(), text_w);
                for (ci, (col, _)) in cols.iter().enumerate() {
                    theme::text_right(p, Pos2::new(col_x(ci), row.center().y), &theme::bytes(v[usize::from(*col) - 1]), theme::regular(13.5), c::text());
                }
                // on app rows the name and icon also open the app popup; they select the row first
                if let Some(k) = key.as_deref().filter(|k| group == Group::Apps && *k != OTHER_KEY) {
                    let font = theme::regular(14.0);
                    if app_popup::link(ui, ui.id().with((k, "app-link")), ir, Pos2::new(row.left() + 46.0, row.center().y), &name, font, text_w, k, actions) {
                        self.sel = this.clone();
                    }
                }
                if resp.clicked() {
                    self.sel = this;
                }
            }
        });
    }

    /// Time menu, plus the period or Custom steppers to its left (the parent lays out right to left).
    fn time_controls(&mut self, ui: &mut egui::Ui, data: &Data, now: i64) {
        let cal = |label| MenuItem { icon: Some(Glyph::Calendar), ..MenuItem::new(label) };
        let items = [
            MenuItem::new(PRESET_LABELS[0]),
            MenuItem::new(PRESET_LABELS[1]),
            MenuItem::new(PRESET_LABELS[2]),
            MenuItem::new(PRESET_LABELS[3]),
            MenuItem::new(PRESET_LABELS[4]),
            MenuItem { divider_before: true, ..cal("Day") },
            cal("Week"),
            cal("Month"),
            MenuItem::new("All Time"),
            MenuItem { divider_before: true, ..MenuItem::new("Custom") },
        ];
        let cur = span_index(self.span);
        if let Some(i) = theme::menu_items(ui, "traffic-span", span_label(self.span), &items, cur, "") {
            if span_pick_applies(i, cur) {
                self.set_span(span_for_index(i, now, data));
            }
        }
        match self.span {
            Span::Day(_) | Span::Week(_) | Span::Month(_) => {
                let (prev, next) = (td::span_step(self.span, -1, data), td::span_step(self.span, 1, data));
                if let Some(d) = stepper(ui, "period", &period_label(self.span), prev != self.span, next != self.span) {
                    self.set_span(if d < 0 { prev } else { next });
                }
            }
            Span::Custom(from, to) => {
                let today = td::local_midnight(now);
                let oldest = oldest_midnight(data).unwrap_or(today).min(from);
                let can = |to_side: bool, d: i8| custom_step(from, to, to_side, d, oldest, today) != (from, to);
                let day = |t: i64| theme::local(t).format("%-d %b").to_string();
                let mut moved = None;
                if let Some(d) = stepper(ui, "custom-to", &day(day_back(to)), can(true, -1), can(true, 1)) {
                    moved = Some(custom_step(from, to, true, d, oldest, today));
                }
                theme::label(ui, "To", theme::regular(13.0), c::muted());
                if let Some(d) = stepper(ui, "custom-from", &day(from), can(false, -1), can(false, 1)) {
                    moved = Some(custom_step(from, to, false, d, oldest, today));
                }
                theme::label(ui, "From", theme::regular(13.0), c::muted());
                if let Some((f, t)) = moved {
                    self.set_span(Span::Custom(f, t));
                }
            }
            _ => {}
        }
    }

    fn chart_area(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, agg: &Arc<Agg>, filters: (Group, Net, DirF), actions: &mut Vec<Action>) {
        let (group, _, dir) = filters;
        let r = ui.max_rect();
        // header: mode switch on the left, time menu (and steppers) on the right
        let head = Rect::from_min_size(r.min, Vec2::new(r.width(), 36.0));
        let mut hl = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
        theme::segmented(&mut hl, &[(Glyph::Graph, "Graph"), (Glyph::Columns, "Usage"), (Glyph::Bars, "Bars")], &mut self.mode);
        let mut hr = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let now = s.live.back().map(|l| l.t).unwrap_or_else(store::now);
        self.time_controls(&mut hr, &s.store.data, now);
        // export: what the Usage view shows, from the same aggregate; limited mode has no detail to export
        if s.admin {
            if theme::icon_button(&mut hr, Glyph::Export, 28.0, "Export as CSV").clicked() {
                actions.push(self.export(agg, s, filters));
            }
        } else {
            let (er, eresp) = hr.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
            theme::glyph(hr.painter(), er.shrink(28.0 * 0.22), Glyph::Export, c::faint());
            eresp.on_hover_text("Needs administrator mode");
        }

        // title row: what the chart and strip are showing
        let title = Rect::from_min_size(Pos2::new(r.left(), r.top() + 44.0), Vec2::new(r.width(), 28.0));
        let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(title).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let shown = match &self.sel {
            Sel::All => None,
            Sel::App(k) => Some((Group::Apps, k.clone(), s.app_name(k), None)),
            Sel::Group(g, k) => {
                let row = agg.rows.iter().find(|r| r.key == *k);
                Some((*g, k.clone(), row.map_or_else(|| k.clone(), |r| r.label.clone()), row.and_then(|r| r.icon_key.clone())))
            }
        };
        match shown {
            Some((g, k, name, icon_key)) => {
                let (ir, _) = tui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                row_icon(&tui, s, icons, g, &k, &name, icon_key.as_deref(), ir);
                theme::label(&mut tui, name, theme::semibold(16.0), c::text());
                if theme::pill(&mut tui, "Show all traffic", None, false).clicked() {
                    self.sel = Sel::All;
                }
            }
            None => {
                theme::label(&mut tui, "All traffic", theme::semibold(16.0), c::text());
            }
        }

        // the timeline scrubber sits under the Graph and Bars views, for the preset spans only
        let scrub = self.mode != 1 && matches!(self.span, Span::Preset(_));
        let bottom = if scrub { r.bottom() - SCRUB_H - 30.0 } else { r.bottom() - 28.0 };
        let chart = Rect::from_min_max(Pos2::new(r.left(), r.top() + 90.0), Pos2::new(r.right(), bottom));
        match self.mode {
            1 => {
                usage_view::sync(ui.ctx(), selection_key(self.span, &self.sel, filters));
                let admin = s.admin;
                let area = Rect::from_min_max(Pos2::new(r.left(), r.top() + 84.0), r.max);
                let mut uui = ui.new_child(egui::UiBuilder::new().max_rect(area));
                usage_view::show(&mut uui, agg, s, icons, actions, admin);
            }
            2 => self.bars(ui, chart, agg, dir),
            _ => {
                let view = self.paused.clone().unwrap_or_else(|| agg.clone());
                self.graph(ui, s, chart, &view, group, dir);
            }
        }
        if scrub {
            self.scrubber(ui, Rect::from_min_max(Pos2::new(r.left(), r.bottom() - SCRUB_H), r.max), agg);
        }
    }

    fn time_axis(&self, p: &egui::Painter, chart: Rect, agg: &Agg) {
        let n = agg.times.len();
        if n < 2 {
            return;
        }
        let len = n as i64 * agg.step;
        let ticks = ((chart.width() / 130.0) as usize).clamp(2, 12);
        for i in 0..=ticks {
            let idx = (n - 1) * i / ticks;
            let x = chart.left() + chart.width() * idx as f32 / (n - 1) as f32;
            let label = tick_label(agg.step, len, agg.times[idx]);
            p.line_segment([Pos2::new(x, chart.bottom()), Pos2::new(x, chart.bottom() + 4.0)], Stroke::new(1.0_f32, c::border()));
            let align = if i == 0 { Align2::LEFT_TOP } else if i == ticks { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
            p.text(Pos2::new(x, chart.bottom() + 8.0), align, label, theme::regular(12.0), c::muted());
        }
    }

    fn graph(&mut self, ui: &mut egui::Ui, s: &mut Shared, chart: Rect, agg: &Arc<Agg>, group: Group, dir: DirF) {
        let paused = self.paused.is_some();
        let step = agg.step.max(1) as f64;
        // the Direction filter drops the excluded series
        let show = [dir != DirF::Out, dir != DirF::In];
        let down: Vec<f64> = agg.down.iter().map(|v| v / step).collect();
        let up: Vec<f64> = agg.up.iter().map(|v| v / step).collect();
        let max = [&down, &up].iter().zip(show).filter(|(_, on)| *on).flat_map(|(v, _)| v.iter()).cloned().fold(0.0, f64::max);
        // the marker lane sits under the plot; the time axis goes under the lane
        let plot = Rect::from_min_max(chart.min, Pos2::new(chart.right(), chart.bottom() - LANE_H));
        let lane = Rect::from_min_max(Pos2::new(chart.left(), plot.bottom()), chart.max);
        let p = ui.painter_at(chart.expand(30.0));
        let fixed = markers::scale_max(*scale_slot(&mut s.store.data.settings, self.span));
        // not while paused or looking back
        let live = self.span == Span::Preset(0) && self.end_offset == 0 && !paused;
        if !live {
            self.glide = None;
        }
        if max <= 0.0 {
            theme::empty_state(&p, plot.center(), "No traffic in this period. Try another time range.");
            self.time_axis(&p, chart, agg);
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
        // a new live sample slides the series in from the left over NORMAL; not once the newest
        // sample is stale (e.g. after hiding to the tray)
        let mut shift = 0.0;
        if live && agg.times.len() > 1 {
            let newest = *agg.times.last().unwrap();
            let now = ui.input(|i| i.time);
            match self.glide {
                Some((t, _)) if t == newest => {}
                Some((t, _)) if (1..=5).contains(&(newest - t)) => self.glide = Some((newest, now)),
                _ => self.glide = Some((newest, f64::NEG_INFINITY)),
            }
            if let Some((_, started)) = self.glide {
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
        }
        let sp = ui.painter_at(plot);
        let moved = plot.translate(Vec2::new(shift, 0.0));
        if show[0] {
            theme::area(&sp, moved, &down, nice, c::down());
        }
        if show[1] {
            theme::area(&sp, moved, &up, nice, c::up());
        }
        self.time_axis(&p, chart, agg);
        self.draw_markers(ui, s, lane, agg, shift);

        let resp = ui.interact(plot, ui.id().with("graph"), Sense::hover());
        if let Some(pos) = resp.hover_pos().filter(|_| agg.times.len() >= 2) {
            let n = agg.times.len();
            let idx = (((pos.x - plot.left()) / plot.width()) * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32) as usize;
            let x = plot.left() + plot.width() * idx as f32 / (n - 1) as f32;
            p.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0_f32, c::muted()));
            for ((vals, col), on) in [(&down, c::down()), (&up, c::up())].into_iter().zip(show) {
                if on {
                    let y = plot.bottom() - (vals[idx] / nice).clamp(0.0, 1.0) as f32 * plot.height();
                    p.circle(Pos2::new(x, y), 4.0, col, Stroke::new(2.0_f32, c::bg()));
                }
            }
            let t = agg.times[idx];
            let (d, u) = (down[idx], up[idx]);
            let step = agg.step;
            // top contributors of the step, named the way the list names them
            let names: Vec<(String, u64)> = agg.step_top.get(idx).map_or(&[][..], |v| v.as_slice()).iter().map(|(k, v)| {
                let name = if group == Group::Apps { s.app_name(k) } else { agg.rows.iter().find(|r| r.key == *k).map_or_else(|| k.clone(), |r| r.label.clone()) };
                (name, v[0] + v[1])
            }).collect();
            let filtered = self.sel != Sel::All;
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("tip"), |ui| {
                theme::label(ui, tip_when(step, t), theme::semibold(13.0), c::text());
                ui.horizontal(|ui| {
                    if show[0] {
                        theme::label(ui, format!("↓ {}", theme::rate(d)), theme::regular(13.0), c::down());
                    }
                    if show[1] {
                        theme::label(ui, format!("↑ {}", theme::rate(u)), theme::regular(13.0), c::up());
                    }
                });
                if !filtered && !names.is_empty() {
                    ui.separator();
                    for (n, v) in names {
                        ui.horizontal(|ui| {
                            theme::label(ui, n, theme::regular(12.5), c::text());
                            theme::label(ui, theme::bytes(v), theme::regular(12.5), c::muted());
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
        let slot = scale_slot(&mut s.store.data.settings, self.span);
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
            if chip(&mut right, "Paused", c::warn(), "The graph is frozen; click to resume").clicked() {
                self.paused = None;
            }
        }
    }

    /// Clock markers for Important events in the lane under the plot; neighbours closer than
    /// 14 px merge, with a count badge. Hover lists up to 5 events.
    fn draw_markers(&self, ui: &mut egui::Ui, s: &Shared, lane: Rect, agg: &Agg, shift: f32) {
        let (Some(&t0), Some(&t1)) = (agg.times.first(), agg.times.last()) else { return };
        let step = agg.step.max(1);
        let data = &s.store.data;
        let picked = markers::pick_events(&data.events, &self.sel, |k| data.apps.get(k).and_then(|r| r.publisher.clone()));
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
                let lines: Vec<(String, String)> = cl
                    .events
                    .iter()
                    .rev()
                    .take(5)
                    .filter_map(|id| picked.iter().find(|e| e.id == *id))
                    .map(|e| {
                        let when = if agg.step < HOUR { tip_when(agg.step, e.t) } else { theme::date_time(e.t) };
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
    /// of the visible window. Dragging pans the whole page back in time; the Now chip returns to live.
    fn scrubber(&mut self, ui: &mut egui::Ui, rect: Rect, agg: &Agg) {
        let bar = Rect::from_min_max(rect.min, Pos2::new(rect.right() - SCRUB_TAIL, rect.bottom()));
        let p = ui.painter_at(rect);
        p.rect_filled(bar, CornerRadius::same(4), c::panel());
        let window = agg.times.len();
        let max_offset = agg.max_offset;
        let h = &agg.history;
        let range = markers::scrub_range(&agg.times, agg.step, self.end_offset, max_offset);
        if let (true, Some((t0, t1))) = (h.len() >= 2 && bar.width() > 4.0, range) {
            // placed by time (gaps stay empty) so it lines up with the handle; one peak per ~2 px
            let bins = ((bar.width() / 2.0) as usize).max(2);
            let vals = markers::history_bins(h, t0, t1, bins);
            let peak = vals.iter().cloned().fold(0.0, f64::max);
            theme::area(&p, bar.shrink2(Vec2::new(0.0, 3.0)), &vals, peak, c::faint());
            let fmt = |t: i64| tick_label(agg.step, t1 - t0, t);
            p.text(Pos2::new(bar.left() + 6.0, bar.center().y), Align2::LEFT_CENTER, fmt(t0), theme::regular(10.5), c::muted());
            p.text(Pos2::new(bar.right() - 6.0, bar.center().y), Align2::RIGHT_CENTER, fmt(t1 - agg.step), theme::regular(10.5), c::muted());
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
            if chip(&mut tui, "Now", c::accent(), "Back to live").clicked() {
                self.end_offset = 0;
            }
        } else {
            theme::label(&mut tui, "Live", theme::semibold(12.0), c::muted());
        }
    }

    fn bars(&self, ui: &mut egui::Ui, chart: Rect, agg: &Agg, dir: DirF) {
        // Group into calendar-friendly buckets: 10s, 5min (3h), 1h (24h), 1 day
        let group = if agg.step < MINUTE {
            10
        } else if agg.step < HOUR {
            if agg.times.len() <= 180 { 5 } else { 60 }
        } else {
            24
        };
        let show = [dir != DirF::Out, dir != DirF::In];
        let mut groups: Vec<(i64, f64, f64)> = Vec::new();
        for (i, chunk) in agg.down.chunks(group).enumerate() {
            let up: f64 = agg.up[i * group..(i * group + chunk.len())].iter().sum();
            groups.push((agg.times[i * group], chunk.iter().sum(), up));
        }
        let max = groups.iter().map(|g| g.1.max(g.2)).fold(0.0, f64::max);
        let p = ui.painter_at(chart.expand(30.0));
        if max <= 0.0 {
            theme::empty_state(&p, chart.center(), "No traffic in this period. Try another time range.");
            return;
        }
        let len = agg.times.len() as i64 * agg.step;
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
                p.text(Pos2::new(cx, chart.bottom() + 8.0), Align2::CENTER_TOP, tick_label(agg.step, len, *t), theme::regular(12.0), c::muted());
            }
            if hover == Some(i) {
                let (d, u, t) = (*d, *u, *t);
                let text = match show {
                    [true, false] => format!("↓ {}", theme::bytes(d as u64)),
                    [false, true] => format!("↑ {}", theme::bytes(u as u64)),
                    _ => format!("↓ {}   ↑ {}", theme::bytes(d as u64), theme::bytes(u as u64)),
                };
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("btip"), |ui| {
                    theme::label(ui, theme::date_time(t), theme::semibold(13.0), c::text());
                    theme::label(ui, text, theme::regular(13.0), c::text());
                });
            }
        }
        p.line_segment([chart.left_bottom(), chart.right_bottom()], Stroke::new(1.0_f32, c::border()));
    }

    /// Bottom usage strip: download total, today gauge, upload total, then the Internet/LAN split.
    #[allow(clippy::too_many_arguments)]
    fn strip(&self, ui: &mut egui::Ui, s: &Shared, agg: &Agg, filters: (Group, Net, DirF), page_w: f32, ease: bool, actions: &mut Vec<Action>) {
        let (_, net, dir) = filters;
        let r = ui.max_rect();
        let ctx = ui.ctx().clone();
        let id = ui.id().with("strip");
        let cy = r.center().y;
        ui.painter().rect_filled(r, CornerRadius::ZERO, c::panel());
        ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, c::border()));

        // totals and live rate follow the selection and both filters
        // the current speed is read fresh each frame (one live sample), outside the cached aggregate
        let cur = self.last_query.as_ref().map_or([0, 0], |q| td::live_rate(&s.store.data, &s.live, q));
        let total = agg.total;
        let dim = strip_dim(dir);
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
        let col = |p: &egui::Painter, x: f32, right: bool, bytes_v: f64, rate_v: f64, key: &str, dimmed: bool| {
            let (al, ax) = if right { (Align2::RIGHT_CENTER, x + slot_w) } else { (Align2::LEFT_CENTER, x) };
            let [b, rt] = if dimmed { strip_texts(0, 0.0, true) } else { strip_texts(num(&format!("{key}b"), bytes_v) as u64, num(&format!("{key}r"), rate_v), false) };
            p.text(Pos2::new(ax, cy - 9.0), al, b, big.clone(), if dimmed { c::faint() } else { c::text() });
            p.text(Pos2::new(ax, cy + 12.0), al, rt, small.clone(), if dimmed { c::faint() } else { c::muted() });
        };

        let p = ui.painter().clone();
        let mut x = r.left() + 22.0;
        col(&p, x, true, total[0] as f64, cur[0] as f64, "d", dim[0]);
        x += slot_w + 8.0;
        let arrow = |x: f32, g: Glyph, color: Color32| {
            theme::glyph(&p, Rect::from_center_size(Pos2::new(x + 8.0, cy), Vec2::splat(16.0)), g, color);
        };
        arrow(x, Glyph::Down, if dim[0] { c::faint() } else { c::down() });
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
        // data-alert bell above the gauge total, registered after the gauge so it gets the hover
        let br = Rect::from_center_size(Pos2::new(gc.x, gc.y - 37.0), Vec2::splat(16.0));
        let bresp = ui.interact(br.expand(2.0), id.with("bell"), Sense::click());
        let bt = motion::hover(ui, bresp.id, bresp.hovered());
        if bt > 0.0 {
            p.rect_filled(br.expand(2.0), CornerRadius::same(5), theme::lerp_color(Color32::TRANSPARENT, c::hover(), bt));
        }
        theme::glyph(&p, br.shrink(1.0), Glyph::BellPlus, theme::lerp_color(c::muted(), c::text(), bt));
        if bresp.on_hover_text("Set a data alert").clicked() {
            actions.push(Action::OpenSettings);
        }
        x = gr.right() + 14.0;

        arrow(x, Glyph::Up, if dim[1] { c::faint() } else { c::up() });
        x += 16.0 + 8.0;
        col(&p, x, false, total[1] as f64, cur[1] as f64, "u", dim[1]);
        x += slot_w + 22.0;

        let divider = |x: f32| p.line_segment([Pos2::new(x, r.top() + 16.0), Pos2::new(x, r.bottom() - 16.0)], Stroke::new(1.0_f32, c::border()));
        divider(x);
        x += 22.0;

        // Internet / Local network rows: they ignore the Network filter, which only dims a row
        let show_counts = strip_shows_counts(page_w);
        let counts_w = if show_counts { 210.0 } else { 100.0 };
        let (wan, lan) = (agg.wan, agg.lan);
        let ndim = net_dim(net);
        let rows_right = r.right() - 22.0 - counts_w;
        let tot_font = theme::semibold(13.5);
        let tot_w = motion::num_slot(ui, &tot_font, "999.99 GB");
        let bar_x = x + 22.0 + tot_w + 10.0;
        let bar_w = (rows_right - bar_x).clamp(40.0, 260.0);
        let sum = |v: [u64; 2]| v[0] as f64 + v[1] as f64;
        let maxv = sum(wan).max(sum(lan)).max(1.0);
        for (i, (gl, tip, v)) in [(Glyph::Globe, "Internet", wan), (Glyph::Wifi, "Local network", lan)].into_iter().enumerate() {
            let mut p = p.clone();
            if ndim[i] {
                p.multiply_opacity(0.35);
            }
            let y = cy + if i == 0 { -14.0 } else { 14.0 };
            let ir = Rect::from_center_size(Pos2::new(x + 8.0, y), Vec2::splat(16.0));
            theme::glyph(&p, ir, gl, c::muted());
            let tip = if ndim[i] { format!("{tip} (not counted under the Network filter)") } else { tip.to_string() };
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

impl TrafficPage {
    /// Snapshot of the Usage view for the export button; the span, selection and filters name the file.
    fn export(&self, agg: &Agg, s: &Shared, filters: (Group, Net, DirF)) -> Action {
        let (_, net, dir) = filters;
        let net = match net {
            Net::All => "",
            Net::Lan => "LAN",
            Net::Wan => "WAN",
        };
        let dir = match dir {
            DirF::Both => "",
            DirF::In => "download",
            DirF::Out => "upload",
        };
        let sel = match &self.sel {
            Sel::All => String::new(),
            Sel::App(k) => s.app_name(k),
            Sel::Group(_, k) => agg.rows.iter().find(|r| r.key == *k).map_or_else(|| k.clone(), |r| r.label.clone()),
        };
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
        Action::ExportUsage(usage_view::export_rows(agg, s), csv_export::file_name(span_label(self.span), &sel, net, dir, &stamp))
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
    fn selection_key_tracks_span_selection_and_filters() {
        let f = (Group::Apps, Net::All, DirF::Both);
        let base = selection_key(Span::Preset(0), &Sel::All, f);
        assert_eq!(base, selection_key(Span::Preset(0), &Sel::All, f));
        assert_ne!(base, selection_key(Span::Preset(1), &Sel::All, f));
        assert_ne!(base, selection_key(Span::Preset(0), &Sel::App("chrome.exe".into()), f));
        assert_ne!(selection_key(Span::Preset(0), &Sel::App("chrome.exe".into()), f), selection_key(Span::Preset(0), &Sel::App("firefox.exe".into()), f));
        assert_ne!(base, selection_key(Span::Preset(0), &Sel::All, (Group::Apps, Net::All, DirF::In)));
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
        let mk = |n: &str, c: usize| (format!("{n}.exe"), n.to_string(), c, [0, 0]);
        let mut r = vec![mk("b", 2), mk("a", 2), mk("z", 5)];
        order_connected(&mut r, NATURAL, false);
        assert_eq!(r.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["z", "a", "b"]);
        order_connected(&mut r, SortSpec { col: 0, desc: false }, false);
        assert_eq!(r.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["a", "b", "z"]);
    }

    #[test]
    fn traffic_connected_admin_sorts_by_speed() {
        // many connections but slow vs. few connections but fast
        let mut r = vec![("a.exe".to_string(), "a".to_string(), 9, [10, 900]), ("b.exe".to_string(), "b".to_string(), 1, [500, 5])];
        let order = |r: &[ConnRow]| r.iter().map(|x| x.1.clone()).collect::<Vec<_>>();
        order_connected(&mut r, SortSpec { col: 1, desc: true }, true);
        assert_eq!(order(&r), ["b", "a"]);
        order_connected(&mut r, SortSpec { col: 2, desc: true }, true);
        assert_eq!(order(&r), ["a", "b"]);
        order_connected(&mut r, SortSpec { col: 1, desc: false }, true);
        assert_eq!(order(&r), ["a", "b"]);
        // limited mode shows dashes, so the number columns keep sorting by connection count
        order_connected(&mut r, SortSpec { col: 1, desc: true }, false);
        assert_eq!(order(&r), ["a", "b"]);
    }

    #[test]
    fn group_change_resets_selection() {
        let mut page = TrafficPage::default();
        let mut set = store::Settings::default();
        page.sel = Sel::App("chrome.exe".into());
        // picking the group already shown changes nothing
        assert!(!page.pick_group(&mut set, Group::Apps));
        assert_eq!(page.sel, Sel::App("chrome.exe".into()));
        assert!(page.pick_group(&mut set, Group::Countries));
        assert_eq!(set.traffic_group, Group::Countries.to_u8());
        assert_eq!(page.sel, Sel::All);
        page.sel = Sel::Group(Group::Countries, "IL".into());
        assert!(page.pick_group(&mut set, Group::Types));
        assert_eq!(page.sel, Sel::All);
    }

    #[test]
    fn admin_only_items() {
        assert_eq!(group_disabled(true), [false; 4]);
        assert_eq!(group_disabled(false), [false, true, true, true]);
        assert_eq!(net_disabled(true), [false; 3]);
        assert_eq!(net_disabled(false), [false, true, true]);
        // a saved admin-only value shows as Apps / WAN & LAN, and the saved value stays
        let mut set = store::Settings::default();
        set.traffic_group = 2;
        set.traffic_net = 1;
        set.traffic_dir = 2;
        assert_eq!(view_filters(&set, false), (Group::Apps, Net::All, DirF::Out));
        assert_eq!((set.traffic_group, set.traffic_net), (2, 1));
        assert_eq!(view_filters(&set, true), (Group::Types, Net::Lan, DirF::Out));
    }

    #[test]
    fn sort_list_key_per_mode() {
        assert_eq!(sort_key(Group::Apps), "traffic");
        assert_eq!(sort_key(Group::Publishers), "traffic.publisher");
        assert_eq!(sort_key(Group::Types), "traffic.type");
        assert_eq!(sort_key(Group::Countries), "traffic.country");
    }

    #[test]
    fn hidden_column_sort_falls_back_to_natural() {
        let up = SortSpec { col: 2, desc: true };
        assert_eq!(shown_spec(up, DirF::In), NATURAL);
        assert_eq!(shown_spec(up, DirF::Out), up);
        assert_eq!(shown_spec(SortSpec { col: 1, desc: true }, DirF::Out), NATURAL);
        assert_eq!(shown_spec(SortSpec { col: 0, desc: false }, DirF::In), SortSpec { col: 0, desc: false });
    }

    #[test]
    fn strip_cells_dim_excluded() {
        assert_eq!(strip_dim(DirF::In), [false, true]);
        assert_eq!(strip_dim(DirF::Out), [true, false]);
        assert_eq!(strip_dim(DirF::Both), [false, false]);
        assert_eq!(strip_texts(1_500, 2_000.0, true), ["—".to_string(), "—".to_string()]);
        assert_eq!(strip_texts(1_500, 2_000.0, false), [theme::bytes(1_500), theme::rate(2_000.0)]);
        // [Internet, Local network]
        assert_eq!(net_dim(Net::Lan), [true, false]);
        assert_eq!(net_dim(Net::Wan), [false, true]);
        assert_eq!(net_dim(Net::All), [false, false]);
    }

    #[test]
    fn cat_glyph_covers_all() {
        use crate::ports::Cat;
        let all = [Cat::Web, Cat::Names, Cat::Files, Cat::Mail, Cat::Remote, Cat::Media, Cat::Games, Cat::System, Cat::Other];
        let g: Vec<Glyph> = all.iter().map(|c| theme::cat_glyph(*c)).collect();
        for (i, a) in g.iter().enumerate() {
            for b in &g[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_eq!(theme::cat_glyph(crate::ports::category(crate::ports::UNCLASSIFIED)), Glyph::TypeOther);
    }

    #[test]
    fn span_label() {
        assert_eq!(super::span_label(Span::Preset(2)), "24 Hours");
        assert_eq!(super::span_label(Span::Preset(3)), "Last 7 Days");
        assert_eq!(super::span_label(Span::Day(1_790_000_000)), "Day");
        assert_eq!(super::span_label(Span::Week(1_790_000_000)), "Week");
        assert_eq!(super::span_label(Span::Month(1_790_000_000)), "Month");
        assert_eq!(super::span_label(Span::AllTime), "All Time");
        assert_eq!(super::span_label(Span::Custom(0, 1)), "Custom");
    }

    #[test]
    fn span_menu_round_trip() {
        let d = store::Data::default();
        let now = 1_790_000_000;
        for i in 0..10 {
            assert_eq!(span_index(span_for_index(i, now, &d)), i);
        }
        // calendar periods start at the current one
        let today = super::super::traffic_data::local_midnight(now);
        assert_eq!(span_for_index(5, now, &d), Span::Day(today));
        let Span::Week(w) = span_for_index(6, now, &d) else { panic!() };
        assert_eq!(theme::local(w).format("%a").to_string(), "Mon");
        assert!(w <= today && today - w < 7 * DAY);
        let Span::Month(m) = span_for_index(7, now, &d) else { panic!() };
        assert_eq!(theme::local(m).format("%-d").to_string(), "1");
    }

    #[test]
    fn custom_steppers_clamp() {
        let today = super::super::traffic_data::local_midnight(1_790_000_000);
        let oldest = day_back(day_back(today));
        let (from, to) = (day_back(today), day_fwd(today));
        // From can't pass the oldest stored day or reach To
        assert_eq!(custom_step(from, to, false, -1, oldest, today), (oldest, to));
        assert_eq!(custom_step(oldest, to, false, -1, oldest, today), (oldest, to));
        assert_eq!(custom_step(from, to, false, 1, oldest, today), (today, to));
        assert_eq!(custom_step(today, to, false, 1, oldest, today), (today, to));
        // To can't go past today or before From
        assert_eq!(custom_step(from, to, true, 1, oldest, today), (from, to));
        assert_eq!(custom_step(from, to, true, -1, oldest, today), (from, today));
        assert_eq!(custom_step(from, today, true, -1, oldest, today), (from, today));
    }

    #[test]
    fn span_pick_snaps_calendar_kinds_to_now() {
        for i in 5..=7 {
            assert!(span_pick_applies(i, i));
        }
        for i in [0, 4, 8, 9] {
            assert!(!span_pick_applies(i, i));
        }
        assert!(span_pick_applies(1, 0));
    }

    #[test]
    fn connected_cells_show_rates() {
        assert_eq!(connected_cells([2_000, 500], true), [theme::rate(2_000.0), theme::rate(500.0)]);
        assert_eq!(connected_cells([2_000, 500], false), ["—".to_string(), "—".to_string()]);
    }
}
