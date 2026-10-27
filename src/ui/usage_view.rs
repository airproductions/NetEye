//! Usage view: Hosts, Traffic type and Countries columns (Task 8).
//!
//! Chart mode 2. Three equal, full-height columns read the cached `Agg` (already narrowed by the
//! selection and the filters); nothing here aggregates. Each column sorts on its own, shows its
//! top 20 rows and folds the rest into a "+N more" row that expands the column when clicked.

use std::borrow::Cow;

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use super::csv_export::ExportRow;
use super::flags;
use super::sort::{self, SortSpec};
use super::theme::{self, c, Glyph, Icons};
use super::traffic_data::{Agg, Row, NO_HOST};
use super::Action;
use crate::monitor::Shared;

/// Rows shown per column before the "+N more" row.
const TOP: usize = 20;
const ROW_H: f32 = 34.0;
const TITLE_H: f32 = 30.0;
const HEAD_H: f32 = 24.0;
const FOOT_H: f32 = 30.0;
/// Space for the total at a row's right end.
const TOTAL_W: f32 = 76.0;
/// No column picked: total traffic, biggest first (the order `Agg` already has).
const NATURAL: SortSpec = SortSpec { col: u8::MAX, desc: true };
const DBIP_URL: &str = "https://db-ip.com";

/// (title, sort key, CSV column name) per column.
const COLS: [(&str, &str, &str); 3] = [("Hosts", "usage.hosts", "hosts"), ("Traffic type", "usage.types", "types"), ("Countries", "usage.countries", "countries")];

fn total(r: &Row) -> u64 {
    r.down.saturating_add(r.up)
}

fn column_rows(agg: &Agg, i: usize) -> &[Row] {
    match i {
        0 => &agg.hosts,
        1 => &agg.types,
        _ => &agg.countries,
    }
}

/// Columns: 0 name, 1 total. A saved column outside them falls back to NATURAL.
fn normalize(spec: SortSpec) -> SortSpec {
    if spec.col > 1 { NATURAL } else { spec }
}

/// The column's rows in display order. NATURAL is `Agg`'s own order, so it borrows.
fn ordered(rows: &[Row], spec: SortSpec) -> Cow<'_, [Row]> {
    if spec == NATURAL {
        return Cow::Borrowed(rows);
    }
    let mut v = rows.to_vec();
    sort::apply(&mut v, spec, |a, b, col| if col == 0 { sort::cmp_text(&a.label, &b.label) } else { total(a).cmp(&total(b)) }, |r| r.label.as_str());
    Cow::Owned(v)
}

/// The rows to draw and, when the column is collapsed and longer than `TOP`, the hidden tail as
/// (row count, bytes).
fn visible_rows(rows: &[Row], expanded: bool) -> (Vec<&Row>, Option<(usize, u64)>) {
    if expanded || rows.len() <= TOP {
        return (rows.iter().collect(), None);
    }
    let tail = &rows[TOP..];
    (rows[..TOP].iter().collect(), Some((tail.len(), tail.iter().map(total).fold(0, u64::saturating_add))))
}

fn spec_for(s: &Shared, i: usize) -> SortSpec {
    normalize(s.store.data.settings.sort_for(COLS[i].1, NATURAL))
}

/// The three columns as CSV rows, each column in its on-screen order and in full.
pub fn export_rows(agg: &Agg, s: &Shared) -> Vec<ExportRow> {
    let mut out = Vec::new();
    for i in 0..3 {
        for r in ordered(column_rows(agg, i), spec_for(s, i)).iter() {
            out.push(ExportRow { column: COLS[i].2, name: r.label.clone(), country_code: r.cc.clone().unwrap_or_default(), down: r.down, up: r.up });
        }
    }
    out
}

/// Which columns are expanded, and for which range and selection.
#[derive(Clone, Copy, Default)]
struct State {
    key: u64,
    expanded: [bool; 3],
}

fn state_id() -> egui::Id {
    egui::Id::new("usage_view.state")
}

/// Collapses every column when `key` (range and selection) differs from the last one seen.
pub fn sync(ctx: &egui::Context, key: u64) {
    ctx.data_mut(|d| {
        let st = d.get_temp_mut_or_default::<State>(state_id());
        if st.key != key {
            *st = State { key, expanded: [false; 3] };
        }
    });
}

pub fn show(ui: &mut Ui, agg: &Agg, s: &mut Shared, icons: &mut Icons, actions: &mut Vec<Action>, admin: bool) {
    let r = ui.max_rect();
    if !admin {
        limited(ui, r, actions);
        return;
    }
    let mut st = ui.ctx().data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    let w = r.width() / 3.0;
    for i in 0..3 {
        let x0 = r.left() + w * i as f32;
        let cr = Rect::from_min_max(Pos2::new(x0, r.top()), Pos2::new(x0 + w, r.bottom()));
        if i > 0 {
            ui.painter().line_segment([cr.left_top(), cr.left_bottom()], Stroke::new(1.0_f32, c::border()));
        }
        let inner = Rect::from_min_max(Pos2::new(cr.left() + if i == 0 { 0.0 } else { 12.0 }, cr.top()), Pos2::new(cr.right() - if i == 2 { 0.0 } else { 12.0 }, cr.bottom()));
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(("usage-col", i)));
        cui.set_clip_rect(inner.intersect(ui.clip_rect()));
        column(&mut cui, i, column_rows(agg, i), s, icons, &mut st.expanded[i]);
    }
    ui.ctx().data_mut(|d| d.insert_temp(state_id(), st));
}

fn column(ui: &mut Ui, i: usize, rows: &[Row], s: &mut Shared, icons: &mut Icons, expanded: &mut bool) {
    let r = ui.max_rect();
    let (title, key, _) = COLS[i];
    let p = ui.painter();
    p.text(Pos2::new(r.left(), r.top() + TITLE_H / 2.0), Align2::LEFT_CENTER, title, theme::semibold(14.0), c::text());

    let hr = Rect::from_min_size(Pos2::new(r.left(), r.top() + TITLE_H), Vec2::new(r.width(), HEAD_H));
    let name_x = hr.left() + 4.0 + flags::SIZE.x + 8.0;
    let mut spec = spec_for(s, i);
    let hh = |x0: f32, x1: f32| Rect::from_min_max(Pos2::new(x0, hr.top()), Pos2::new(x1, hr.bottom()));
    let mut changed = false;
    changed |= sort::header(ui, hh(name_x, hr.right() - TOTAL_W - 10.0), "Name", 0, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
    changed |= sort::header(ui, hh(hr.right() - TOTAL_W, hr.right() - 4.0), "Total", 1, &mut spec, true, NATURAL, Align2::RIGHT_CENTER);
    if changed {
        s.store.data.settings.set_sort(key, spec);
        s.store.mark_dirty();
    }
    ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, c::border()));

    let foot = if i == 2 { FOOT_H } else { 0.0 };
    let body = Rect::from_min_max(Pos2::new(r.left(), hr.bottom() + 2.0), Pos2::new(r.right(), r.bottom() - foot));
    let rows = ordered(rows, spec);
    if rows.is_empty() {
        ui.painter().text(Pos2::new(body.center().x, body.top() + 40.0), Align2::CENTER_CENTER, "Nothing recorded in this period", theme::regular(13.0), c::muted());
    } else {
        let (vis, more) = visible_rows(&rows, *expanded);
        let max = rows.iter().map(total).max().unwrap_or(0).max(1);
        let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        bui.set_clip_rect(body.intersect(ui.clip_rect()));
        let n = vis.len() + usize::from(more.is_some());
        egui::ScrollArea::vertical().id_salt(("usage-scroll", i)).auto_shrink([false, false]).show_rows(&mut bui, ROW_H, n, |ui, range| {
            for j in range {
                match vis.get(j) {
                    Some(row) => draw_row(ui, i, row, max, s, icons),
                    None => {
                        if let Some((count, bytes)) = more {
                            if more_row(ui, count, bytes) {
                                *expanded = true;
                            }
                        }
                    }
                }
            }
        });
    }

    if i == 2 {
        let fr = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - foot), r.max);
        let mut fui = ui.new_child(egui::UiBuilder::new().max_rect(fr).layout(egui::Layout::left_to_right(egui::Align::Center)));
        fui.hyperlink_to(egui::RichText::new("IP Geolocation by DB-IP").font(theme::regular(12.0)), DBIP_URL).on_hover_text("https://db-ip.com · CC BY 4.0");
    }
}

fn draw_row(ui: &mut Ui, col: usize, row: &Row, max: u64, s: &mut Shared, icons: &mut Icons) {
    let (rr, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    if resp.hovered() {
        ui.painter().rect_filled(rr, CornerRadius::same(4), c::hover());
    }
    let cy = rr.center().y - 3.0;
    let ir = Rect::from_center_size(Pos2::new(rr.left() + 4.0 + flags::SIZE.x / 2.0, cy), flags::SIZE);
    let globe = |ui: &Ui| theme::glyph(ui.painter(), Rect::from_center_size(ir.center(), Vec2::splat(16.0)), Glyph::Globe, c::muted());
    match (col, row.cc.as_deref()) {
        (0, _) if row.key == NO_HOST => globe(ui),
        // Traffic types get a lettered chip until the category glyphs land (Task 7's cat_glyph).
        (1, _) => flags::chip(ui.painter(), ir, &row.label),
        (_, Some(cc)) => flags::draw(ui, ir, cc),
        (_, None) => globe(ui),
    }

    let font = theme::regular(13.0);
    let right = rr.right() - 4.0;
    theme::text_right(ui.painter(), Pos2::new(right, cy), &theme::bytes(total(row)), font.clone(), c::text());
    let mut x = right - TOTAL_W - 4.0;
    if col == 0 {
        if row.more_apps > 0 {
            let t = theme::text_right(ui.painter(), Pos2::new(x, cy), &format!("+{}", row.more_apps), theme::regular(11.5), c::muted());
            x = t.left() - 4.0;
        }
        for k in row.apps.iter().rev() {
            let ar = Rect::from_min_size(Pos2::new(x - 16.0, cy - 8.0), Vec2::splat(16.0));
            let name = s.app_name(k);
            icons.draw(ui, s, k, ar, &name);
            x = ar.left() - 4.0;
        }
    }
    let name_x = ir.right() + 8.0;
    theme::text(ui.painter(), Pos2::new(name_x, cy), &row.label, theme::regular(13.5), c::text(), x - name_x - 6.0);

    // a thin bar split by direction, against the column's largest row
    let track = Rect::from_min_max(Pos2::new(name_x, rr.bottom() - 8.0), Pos2::new(right, rr.bottom() - 5.0));
    let p = ui.painter();
    p.rect_filled(track, CornerRadius::same(1), c::raised());
    let wd = track.width() * (row.down as f64 / max as f64) as f32;
    let wu = track.width() * (row.up as f64 / max as f64) as f32;
    if wd >= 0.5 {
        p.rect_filled(Rect::from_min_size(track.min, Vec2::new(wd, track.height())), CornerRadius::same(1), c::down());
    }
    if wu >= 0.5 {
        p.rect_filled(Rect::from_min_size(Pos2::new(track.left() + wd, track.top()), Vec2::new(wu, track.height())), CornerRadius::same(1), c::up());
    }
    resp.on_hover_text(format!("{}\n↓ {}   ↑ {}", row.label, theme::bytes(row.down), theme::bytes(row.up)));
}

/// The collapsed tail. Returns true when clicked.
fn more_row(ui: &mut Ui, count: usize, bytes: u64) -> bool {
    let (rr, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show every row");
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rr, CornerRadius::same(4), c::hover());
    }
    let cy = rr.center().y;
    let ir = Rect::from_center_size(Pos2::new(rr.left() + 4.0 + flags::SIZE.x / 2.0, cy), Vec2::splat(16.0));
    theme::glyph(p, ir, Glyph::Grid, c::accent());
    p.text(Pos2::new(rr.left() + 4.0 + flags::SIZE.x + 8.0, cy), Align2::LEFT_CENTER, format!("+{count} more"), theme::semibold(13.0), c::accent());
    theme::text_right(p, Pos2::new(rr.right() - 4.0, cy), &theme::bytes(bytes), theme::regular(13.0), c::muted());
    resp.clicked()
}

/// Limited mode: no flows, so there is nothing per host, type or country to show.
fn limited(ui: &mut Ui, r: Rect, actions: &mut Vec<Action>) {
    let cx = r.center();
    let p = ui.painter();
    theme::glyph(p, Rect::from_center_size(cx - Vec2::new(0.0, 58.0), Vec2::splat(28.0)), Glyph::Lock, c::warn());
    p.text(cx - Vec2::new(0.0, 20.0), Align2::CENTER_CENTER, "Per-app and per-host detail needs administrator mode", theme::semibold(15.0), c::text());
    p.text(cx + Vec2::new(0.0, 4.0), Align2::CENTER_CENTER, "Hosts, traffic types and countries come from connection records, which Windows only shares with administrators.", theme::regular(13.0), c::muted());
    let br = Rect::from_center_size(cx + Vec2::new(0.0, 46.0), Vec2::new(260.0, 40.0));
    let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::top_down(egui::Align::Center)));
    if theme::primary_button(&mut bui, "Restart as administrator").clicked() {
        actions.push(Action::RelaunchAdmin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: u64) -> Vec<Row> {
        (0..n).map(|i| Row { key: format!("k{i}"), label: format!("r{i}"), down: 100 - i, up: i, ..Default::default() }).collect()
    }

    #[test]
    fn visible_rows_collapses_tail() {
        let r = rows(25);
        let (vis, more) = visible_rows(&r, false);
        assert_eq!(vis.len(), 20);
        assert_eq!(vis[19].key, "k19");
        let tail: u64 = r[20..].iter().map(total).sum();
        assert_eq!(more, Some((5, tail)));
        let (vis, more) = visible_rows(&r, true);
        assert_eq!((vis.len(), more), (25, None));
        let short = rows(20);
        assert_eq!(visible_rows(&short, false).0.len(), 20);
        assert_eq!(visible_rows(&short, false).1, None);
    }

    #[test]
    fn usage_order_follows_spec() {
        let r = vec![
            Row { label: "b".into(), down: 5, ..Default::default() },
            Row { label: "A".into(), down: 9, ..Default::default() },
            Row { label: "c".into(), down: 1, ..Default::default() },
        ];
        let names = |v: &[Row]| v.iter().map(|x| x.label.clone()).collect::<Vec<_>>();
        assert_eq!(names(&ordered(&r, NATURAL)), ["b", "A", "c"]);
        assert_eq!(names(&ordered(&r, SortSpec { col: 0, desc: false })), ["A", "b", "c"]);
        assert_eq!(names(&ordered(&r, SortSpec { col: 1, desc: true })), ["A", "b", "c"]);
        assert_eq!(names(&ordered(&r, SortSpec { col: 1, desc: false })), ["c", "b", "A"]);
        assert_eq!(normalize(SortSpec { col: 5, desc: false }), NATURAL);
    }
}
