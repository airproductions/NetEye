//! Light, short animations. egui requests repaints only while an animation is running.

use eframe::egui::{Context, FontId, Id, Ui};

pub const FAST: f32 = 0.15;
pub const NORMAL: f32 = 0.25;

/// Cubic ease-out, clamped to 0..1.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// 0..1 hover amount that fades in and out.
pub fn hover(ui: &Ui, id: Id, hovered: bool) -> f32 {
    ui.ctx().animate_bool_with_time(id.with("hover"), hovered, FAST)
}

/// A value that glides to `target`.
pub fn value(ctx: &Context, id: Id, target: f32) -> f32 {
    ctx.animate_value_with_time(id.with("value"), target, NORMAL)
}

/// Decides whether data-driven values ease or snap. Routine data ticks snap (one frame each);
/// for `NORMAL` seconds after the selection key changes (the user picked another range or app) they ease.
#[derive(Clone, Copy, Debug, Default)]
pub struct EaseGate {
    key: Option<u64>,
    since: f64,
}

impl EaseGate {
    /// `key` identifies the current selection; `now` is the ctx time in seconds.
    pub fn eases(&mut self, key: u64, now: f64) -> bool {
        match self.key {
            Some(k) if k == key => {}
            Some(_) => self.since = now,
            None => self.since = f64::NEG_INFINITY,
        }
        self.key = Some(key);
        now - self.since < NORMAL as f64
    }
}

/// A value easing from `from` to `to`, started at `start` (seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    from: f64,
    to: f64,
    start: f64,
}

impl Tween {
    fn fixed(v: f64, now: f64) -> Self {
        Tween { from: v, to: v, start: now }
    }

    /// The value at `now`.
    pub fn at(&self, now: f64) -> f64 {
        let t = ((now - self.start) / NORMAL as f64) as f32;
        self.from + (self.to - self.from) * ease_out(t) as f64
    }

    /// Still moving at `now`.
    pub fn running(&self, now: f64) -> bool {
        self.from != self.to && now - self.start < NORMAL as f64
    }

    /// Next state for `target`: snaps unless `ease`, keeps a running tween whose target is unchanged,
    /// otherwise starts a new one from the current value.
    pub fn next(prev: Option<Tween>, target: f64, now: f64, ease: bool) -> Tween {
        match prev {
            Some(tw) if ease && tw.to == target => tw,
            Some(tw) if ease => Tween { from: tw.at(now), to: target, start: now },
            _ => Tween::fixed(target, now),
        }
    }
}

/// A value that eases to `target` only when `ease` (see `EaseGate`), otherwise snaps with no extra frames.
pub fn gated(ctx: &Context, id: Id, target: f64, ease: bool) -> f64 {
    let now = ctx.input(|i| i.time);
    let id = id.with("tween");
    let prev: Option<Tween> = ctx.data(|d| d.get_temp(id));
    let tw = Tween::next(prev, target, now, ease);
    ctx.data_mut(|d| d.insert_temp(id, tw));
    if tw.running(now) {
        ctx.request_repaint();
        tw.at(now)
    } else {
        target
    }
}

/// Whether a displayed number snaps regardless of selection: a reset to zero or a jump over 10x.
fn number_snaps(prev: Option<f64>, target: f64) -> bool {
    target <= 0.0 || prev.is_none_or(|p| p <= 0.0 || target / p > 10.0 || p / target > 10.0)
}

/// A displayed number that eases to `target` when `ease`, snapping on routine ticks, large jumps or a reset to zero.
pub fn number(ctx: &Context, id: Id, target: f64, ease: bool) -> f64 {
    let prev: Option<f64> = ctx.data(|d| d.get_temp(id.with("target")));
    let snap = number_snaps(prev, target);
    ctx.data_mut(|d| d.insert_temp(id.with("target"), target));
    gated(ctx, id.with("number"), target, ease && !snap)
}

/// Width of `widest` in `font`, for right-aligned fixed-width number slots (no tabular digits in egui).
pub fn num_slot(ui: &Ui, font: &FontId, widest: &str) -> f32 {
    ui.painter().layout_no_wrap(widest.to_string(), font.clone(), eframe::egui::Color32::WHITE).size().x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_bounds() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert_eq!(ease_out(-1.0), 0.0);
    }

    #[test]
    fn ease_out_monotonic() {
        let mut last = 0.0;
        for i in 0..=100 {
            let v = ease_out(i as f32 / 100.0);
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn gate_first_frame_snaps() {
        let mut g = EaseGate::default();
        assert!(!g.eases(1, 100.0));
    }

    #[test]
    fn gate_data_tick_snaps() {
        let mut g = EaseGate::default();
        g.eases(1, 100.0);
        assert!(!g.eases(1, 101.0));
        assert!(!g.eases(1, 101.01));
    }

    #[test]
    fn gate_selection_change_eases_then_snaps() {
        let mut g = EaseGate::default();
        g.eases(1, 100.0);
        assert!(g.eases(2, 105.0));
        assert!(g.eases(2, 105.0 + NORMAL as f64 * 0.9));
        assert!(!g.eases(2, 105.0 + NORMAL as f64 + 0.01));
        assert!(g.eases(1, 110.0));
    }

    #[test]
    fn tween_snaps_without_ease() {
        let prev = Tween { from: 0.0, to: 10.0, start: 0.0 };
        let tw = Tween::next(Some(prev), 50.0, 0.05, false);
        assert_eq!(tw.at(0.05), 50.0);
        assert!(!tw.running(0.05));
        let first = Tween::next(None, 7.0, 1.0, true);
        assert_eq!(first.at(1.0), 7.0);
        assert!(!first.running(1.0));
    }

    #[test]
    fn tween_eases_from_current_value() {
        let prev = Tween::fixed(10.0, 0.0);
        let tw = Tween::next(Some(prev), 20.0, 5.0, true);
        assert_eq!(tw.at(5.0), 10.0);
        assert!(tw.running(5.0 + NORMAL as f64 / 2.0));
        assert_eq!(tw.at(5.0 + NORMAL as f64), 20.0);
        assert!(!tw.running(5.0 + NORMAL as f64));
    }

    #[test]
    fn tween_keeps_running_for_same_target() {
        let prev = Tween { from: 0.0, to: 10.0, start: 1.0 };
        assert_eq!(Tween::next(Some(prev), 10.0, 1.1, true), prev);
    }

    #[test]
    fn number_snap_rules() {
        assert!(number_snaps(None, 5.0));
        assert!(number_snaps(Some(5.0), 0.0));
        assert!(number_snaps(Some(1.0), 11.0));
        assert!(!number_snaps(Some(5.0), 6.0));
    }

    #[test]
    fn ease_out_front_loaded() {
        assert!(ease_out(0.5) > 0.5);
    }
}
