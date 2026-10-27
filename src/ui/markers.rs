//! Graph event markers (Task 9), plus the graph's other pure helpers: scale presets, pause,
//! glide repaint and the timeline scrubber's x → offset mapping.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::Rect;

use super::traffic_data::{Agg, Group, Query, Sel, NO_PUBLISHER};
use crate::monitor::OTHER_KEY;
use crate::store::Event;

/// Markers closer than this (px) merge into one.
pub const MIN_GAP: f32 = 14.0;

/// Fixed graph maxima in bytes/s; index 0 is Auto.
pub const SCALE_PRESETS: [f64; 7] = [0.0, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9];

/// Newest sample older than this (s) stops the glide.
const STALE_S: f64 = 2.0;
/// Glide frame interval (~30 fps).
const GLIDE_FRAME: Duration = Duration::from_millis(33);

/// One marker: its x and the event ids it stands for, oldest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub x: f32,
    pub events: Vec<u64>,
}

/// Places `(t, id)` events on `x0..x0 + width` (t0 at x0, t1 at the right end), dropping those
/// outside `[t0, t1]`, and merges neighbours closer than `min_gap` into one cluster.
pub fn clusters(events: &[(i64, u64)], t0: i64, t1: i64, x0: f32, width: f32, min_gap: f32) -> Vec<Cluster> {
    let span = (t1 - t0) as f64;
    let x_of = |t: i64| if span > 0.0 { x0 + (width as f64 * (t - t0) as f64 / span) as f32 } else { x0 };
    let mut inside: Vec<(i64, u64)> = events.iter().copied().filter(|(t, _)| (t0..=t1).contains(t)).collect();
    inside.sort_unstable();
    let mut out: Vec<Cluster> = Vec::new();
    // (x of the cluster's first event, x of its last event)
    let mut ends: Vec<(f32, f32)> = Vec::new();
    for (t, id) in inside {
        let x = x_of(t);
        match (out.last_mut(), ends.last_mut()) {
            (Some(c), Some(e)) if x - e.0 < min_gap => {
                c.events.push(id);
                e.1 = x;
                c.x = (e.0 + e.1) / 2.0;
            }
            _ => {
                out.push(Cluster { x, events: vec![id] });
                ends.push((x, x));
            }
        }
    }
    out
}

/// The Important events the graph marks for `sel`: an app's own events, a publisher's apps'
/// events, or all of them for All traffic, Other traffic, a type or a country.
pub fn pick_events<'a>(events: impl IntoIterator<Item = &'a Event>, sel: &Sel, publisher_of: impl Fn(&str) -> Option<String>) -> Vec<&'a Event> {
    let important = events.into_iter().filter(|e| e.kind.important());
    match sel {
        Sel::App(k) | Sel::Group(Group::Apps, k) if k != OTHER_KEY => important.filter(|e| e.app.as_deref() == Some(k.as_str())).collect(),
        Sel::Group(Group::Publishers, p) => important
            .filter(|e| {
                let Some(app) = e.app.as_deref() else { return false };
                publisher_of(app).filter(|x| !x.trim().is_empty()).as_deref().unwrap_or(NO_PUBLISHER) == p
            })
            .collect(),
        _ => important.collect(),
    }
}

/// A saved scale index as a fixed maximum (bytes/s); `None` is Auto (also for unknown indexes).
pub fn scale_max(index: u8) -> Option<f64> {
    SCALE_PRESETS.get(usize::from(index)).copied().filter(|v| *v > 0.0)
}

/// The pause snapshot after a frame: any change of what's viewed resumes the graph.
pub fn next_pause(paused: Option<Arc<Agg>>, old: &Query, new: &Query) -> Option<Arc<Agg>> {
    if old == new { paused } else { None }
}

/// How soon the live glide wants the next frame: never while paused or once the newest sample
/// is stale (e.g. the window was hidden), otherwise at ~30 fps.
pub fn glide_repaint(paused: bool, last_sample_age_s: f64) -> Option<Duration> {
    // NaN fails the comparison too, so an unknown age never repaints
    if !paused && last_sample_age_s < STALE_S { Some(GLIDE_FRAME) } else { None }
}

/// Scrubber: the `end_offset` (steps back from now) for pointer x. The strip spans the whole
/// history (`max_offset + window` steps); the right edge is now, the left edge `max_offset`.
pub fn offset_from_x(x: f32, strip: Rect, max_offset: usize, window: usize) -> usize {
    if !x.is_finite() || strip.width() <= 0.0 {
        return 0;
    }
    let total = (max_offset + window) as f32;
    let back = ((strip.right() - x) / strip.width()).clamp(0.0, 1.0) * total;
    (back.round() as usize).min(max_offset)
}

/// Inverse of `offset_from_x`: the handle's x for `offset`.
pub fn x_from_offset(offset: usize, strip: Rect, max_offset: usize, window: usize) -> f32 {
    let total = (max_offset + window).max(1) as f32;
    strip.right() - strip.width() * offset.min(max_offset) as f32 / total
}

/// Scrubber: the time span `(t0, t1)` the strip covers, `t1` exclusive at the right edge (the
/// live window's end) and `t0 = t1 - (max_offset + window) * step` at the left, so it matches the
/// step-linear `offset_from_x` / `x_from_offset`. `times` are the shown window's step starts,
/// already shifted back by `end_offset` (clamped to `max_offset`).
pub fn scrub_range(times: &[i64], step: i64, end_offset: usize, max_offset: usize) -> Option<(i64, i64)> {
    let last = *times.last()?;
    if step <= 0 {
        return None;
    }
    let off = end_offset.min(max_offset) as i64;
    let t1 = last + step + off * step;
    let t0 = t1 - (max_offset + times.len()) as i64 * step;
    Some((t0, t1))
}

/// Scrubber: the sparse `history` placed by time over `[t0, t1)` in `bins` equal slots, keeping
/// each slot's peak; slots with no stored bucket (the PC or app was off) are 0. Entries before
/// `t0` fall in the first slot, entries at or after `t1` are dropped.
pub fn history_bins(history: &[(i64, f64)], t0: i64, t1: i64, bins: usize) -> Vec<f64> {
    let mut out = vec![0.0; bins];
    if bins == 0 || t1 <= t0 {
        return out;
    }
    let span = (t1 - t0) as f64;
    for &(t, v) in history {
        if t >= t1 {
            continue;
        }
        let i = (((t - t0).max(0) as f64 / span) * bins as f64) as usize;
        let slot = &mut out[i.min(bins - 1)];
        *slot = slot.max(v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::traffic_data::{DirF, Net, Span};
    use crate::store::EventKind;
    use eframe::egui::{Pos2, Vec2};

    #[test]
    fn clusters_merge_close_and_drop_outside() {
        // 0..100 s over 0..1000 px: 10 px per second
        let ev = [(-5, 1), (0, 2), (1, 3), (3, 4), (50, 5), (100, 6), (101, 7)];
        let c = clusters(&ev, 0, 100, 0.0, 1000.0, MIN_GAP);
        assert_eq!(c.iter().map(|c| c.events.clone()).collect::<Vec<_>>(), vec![vec![2, 3], vec![4], vec![5], vec![6]]);
        assert!(c[0].x >= 0.0 && c[0].x <= 10.0);
        assert!((c[1].x - 30.0).abs() < 1e-3);
        assert!((c[2].x - 500.0).abs() < 1e-3);
        assert!((c[3].x - 1000.0).abs() < 1e-3);
        // unsorted input, x offset, and a degenerate window
        let c = clusters(&[(20, 9), (10, 8)], 0, 100, 50.0, 100.0, MIN_GAP);
        assert_eq!(c, vec![Cluster { x: 65.0, events: vec![8, 9] }]);
        assert!(clusters(&[(5, 1)], 5, 5, 0.0, 100.0, MIN_GAP).len() == 1);
        assert!(clusters(&[], 0, 100, 0.0, 100.0, MIN_GAP).is_empty());
    }

    fn ev(id: u64, kind: EventKind, app: Option<&str>) -> Event {
        Event { id, t: id as i64, kind, app: app.map(String::from), subject: String::new(), read: false }
    }

    fn ids(v: &[&Event]) -> Vec<u64> {
        v.iter().map(|e| e.id).collect()
    }

    #[test]
    fn pick_events_by_selection() {
        let events = vec![
            ev(1, EventKind::NewApp, Some("a")),
            ev(2, EventKind::Firewall, Some("a")),
            ev(3, EventKind::VersionChanged, Some("b")),
            ev(4, EventKind::InternetDown, None),
            ev(5, EventKind::DeviceLeft, None),
            ev(6, EventKind::NewApp, Some("c")),
        ];
        let publisher_of = |k: &str| match k {
            "a" | "b" => Some("P".to_string()),
            _ => None,
        };
        let pick = |sel: Sel| ids(&pick_events(&events, &sel, publisher_of));
        assert_eq!(pick(Sel::All), [1, 3, 4, 6]);
        assert_eq!(pick(Sel::App("a".into())), [1]);
        assert_eq!(pick(Sel::App(OTHER_KEY.into())), [1, 3, 4, 6]);
        assert_eq!(pick(Sel::Group(Group::Publishers, "P".into())), [1, 3]);
        assert_eq!(pick(Sel::Group(Group::Publishers, NO_PUBLISHER.into())), [6]);
        assert_eq!(pick(Sel::Group(Group::Types, "web".into())), [1, 3, 4, 6]);
        assert_eq!(pick(Sel::Group(Group::Countries, "IL".into())), [1, 3, 4, 6]);
        assert_eq!(pick(Sel::Group(Group::Apps, "c".into())), [6]);
    }

    #[test]
    fn scale_preset_lookup() {
        assert_eq!(scale_max(0), None);
        assert_eq!(scale_max(3), Some(1e6));
        assert_eq!(scale_max(6), Some(1e9));
        assert_eq!(scale_max(7), None);
        assert_eq!(scale_max(u8::MAX), None);
    }

    fn q(span: Span, sel: Sel) -> Query {
        Query { span, group: Group::Apps, net: Net::All, dir: DirF::Both, sel, admin: true, end_offset: 0 }
    }

    #[test]
    fn pause_cleared_on_query_change() {
        let snap = Some(Arc::new(Agg::default()));
        let base = q(Span::Preset(0), Sel::All);
        assert!(next_pause(snap.clone(), &base, &base.clone()).is_some(), "same query keeps the snapshot");
        assert!(next_pause(None, &base, &base).is_none());
        let changed = [
            q(Span::Preset(1), Sel::All),
            q(Span::Preset(0), Sel::App("a".into())),
            Query { group: Group::Types, ..base.clone() },
            Query { net: Net::Lan, ..base.clone() },
            Query { dir: DirF::In, ..base.clone() },
            Query { end_offset: 5, ..base.clone() },
        ];
        for new in changed {
            assert!(next_pause(snap.clone(), &base, &new).is_none(), "{new:?} should resume");
        }
    }

    #[test]
    fn glide_no_repaint_when_paused_or_stale() {
        assert_eq!(glide_repaint(true, 0.1), None);
        assert_eq!(glide_repaint(true, 10.0), None);
        // hidden to the tray and reopened: the newest sample is old, so no glide frames
        assert_eq!(glide_repaint(false, 2.5), None);
        assert_eq!(glide_repaint(false, 3600.0), None);
        assert_eq!(glide_repaint(false, 0.0), Some(Duration::from_millis(33)));
        assert_eq!(glide_repaint(false, 1.9), Some(Duration::from_millis(33)));
        assert_eq!(glide_repaint(false, f64::NAN), None);
    }

    #[test]
    fn offset_from_x_clamps() {
        let strip = Rect::from_min_size(Pos2::new(100.0, 0.0), Vec2::new(400.0, 26.0));
        assert_eq!(offset_from_x(500.0, strip, 120, 180), 0);
        assert_eq!(offset_from_x(100.0, strip, 120, 180), 120);
        assert_eq!(offset_from_x(900.0, strip, 120, 180), 0);
        assert_eq!(offset_from_x(-50.0, strip, 120, 180), 120);
        // halfway back through 300 steps is 150, clamped to max_offset
        assert_eq!(offset_from_x(300.0, strip, 120, 180), 120);
        // a quarter back is 75 steps
        assert_eq!(offset_from_x(400.0, strip, 120, 180), 75);
        assert_eq!(offset_from_x(300.0, strip, 0, 180), 0);
        assert_eq!(offset_from_x(f32::NAN, strip, 120, 180), 0);
        // the handle round-trips
        for off in [0, 1, 37, 120] {
            assert_eq!(offset_from_x(x_from_offset(off, strip, 120, 180), strip, 120, 180), off);
        }
    }
    #[test]
    fn scrubber_history_placed_by_time_with_gap() {
        // 10 s steps, a 3-step window, 7 steps of look-back: the strip covers 100 s
        let step = 10;
        let (t0, t1) = scrub_range(&[70, 80, 90], step, 0, 7).unwrap();
        assert_eq!((t0, t1), (0, 100));
        // the same strip while looking 2 steps back
        assert_eq!(scrub_range(&[50, 60, 70], step, 2, 7), Some((0, 100)));
        // end_offset beyond max_offset is clamped, as the aggregate does
        assert_eq!(scrub_range(&[0, 10, 20], step, 99, 7), Some((0, 100)));
        assert_eq!(scrub_range(&[], step, 0, 7), None);
        assert_eq!(scrub_range(&[1], 0, 0, 7), None);

        // the PC was off from 30 s to 70 s: no stored buckets there
        let h = [(0, 1.0), (10, 3.0), (20, 2.0), (70, 9.0), (80, 4.0), (90, 5.0), (100, 8.0)];
        let bins = history_bins(&h, t0, t1, 10);
        assert_eq!(bins, vec![1.0, 3.0, 2.0, 0.0, 0.0, 0.0, 0.0, 9.0, 4.0, 5.0]);
        // by index the spike at 70 s would sit in slot 3; by time it is in slot 7, which is where
        // the handle sits when the window ends right after it (offset 2: end at 80 s)
        let strip = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(100.0, 26.0));
        let hx = x_from_offset(2, strip, 7, 3);
        assert!((hx - 80.0).abs() < 1e-3);
        assert_eq!(offset_from_x(75.0, strip, 7, 3), 3);
        // peaks per slot, and old entries clamp into the first slot
        let bins = history_bins(&[(-5, 6.0), (0, 1.0), (50, 2.0), (55, 7.0)], 0, 100, 5);
        assert_eq!(bins, vec![6.0, 0.0, 7.0, 0.0, 0.0]);
        assert_eq!(history_bins(&h, 0, 0, 4), vec![0.0; 4]);
        assert!(history_bins(&h, 0, 100, 0).is_empty());
    }
}
