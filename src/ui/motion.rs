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

/// A displayed number that eases to `target`, snapping on large jumps or a reset to zero.
pub fn number(ctx: &Context, id: Id, target: f64) -> f64 {
    let prev: Option<f64> = ctx.data(|d| d.get_temp(id.with("target")));
    let snap = target <= 0.0 || prev.is_none_or(|p| p <= 0.0 || target / p > 10.0 || p / target > 10.0);
    ctx.data_mut(|d| d.insert_temp(id.with("target"), target));
    ctx.animate_value_with_time(id.with("number"), target as f32, if snap { 0.0 } else { NORMAL }) as f64
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
    fn ease_out_front_loaded() {
        assert!(ease_out(0.5) > 0.5);
    }
}
