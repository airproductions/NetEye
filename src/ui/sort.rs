//! Column sorting shared by the Traffic, Firewall, Scanner and Hardware lists.
#![allow(dead_code)] // Tasks 4-6 consume this module.

use std::cmp::Ordering;

use eframe::egui::{self, Align2, Pos2, Rect, Sense, Ui};

use super::motion;
use super::theme::{self, c};

pub use crate::store::SortSpec;

impl SortSpec {
    /// Clicking the active column flips the direction; another column becomes active with `default_desc`.
    pub fn click(&mut self, col: u8, default_desc: bool) {
        if self.col == col {
            self.desc = !self.desc;
        } else {
            *self = SortSpec { col, desc: default_desc };
        }
    }
}

/// Case-insensitive text order.
pub fn cmp_text(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

fn octets(s: &str) -> Option<[u8; 4]> {
    s.trim().parse::<std::net::Ipv4Addr>().ok().map(|ip| ip.octets())
}

/// Numeric IPv4 order; anything unparseable sorts after valid addresses.
pub fn cmp_ip(a: &str, b: &str) -> Ordering {
    match (octets(a), octets(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => cmp_text(a, b),
    }
}

/// Orders `Option`s with `None` last in the *displayed* order. Callers pass `spec.desc` straight through:
/// `apply` reverses the key's ordering when descending, so for `desc` the `None` result is flipped to
/// `Less` here and the reversal puts it back at the end.
pub fn cmp_opt<T: Ord>(a: Option<T>, b: Option<T>, desc: bool) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(&y),
        (None, None) => Ordering::Equal,
        (None, Some(_)) => if desc { Ordering::Less } else { Ordering::Greater },
        (Some(_), None) => if desc { Ordering::Greater } else { Ordering::Less },
    }
}

/// Stable sort by `key(a, b, spec.col)`, reversed when `spec.desc`, ties broken by name ascending.
pub fn apply<T>(rows: &mut [T], spec: SortSpec, key: impl Fn(&T, &T, u8) -> Ordering, name: impl Fn(&T) -> &str) {
    rows.sort_by(|a, b| {
        let o = key(a, b, spec.col);
        let o = if spec.desc { o.reverse() } else { o };
        o.then_with(|| cmp_text(name(a), name(b)))
    });
}

/// Clickable column header. Muted when idle, bright with a direction arrow when active. Returns true when clicked.
pub fn header(ui: &mut Ui, rect: Rect, label: &str, col: u8, spec: &mut SortSpec, default_desc: bool, align: Align2) -> bool {
    let id = ui.id().with(("sort", label, col));
    let resp = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let active = spec.col == col;
    let t = motion::hover(ui, id, resp.hovered());
    let base = if active { c::text() } else { c::muted() };
    let color = theme::lerp_color(base, c::text(), t);
    let text = if active { format!("{label} {}", if spec.desc { "\u{25BC}" } else { "\u{25B2}" }) } else { label.to_string() };
    let pos = match align {
        Align2::RIGHT_CENTER => Pos2::new(rect.right(), rect.center().y),
        Align2::CENTER_CENTER => rect.center(),
        _ => Pos2::new(rect.left(), rect.center().y),
    };
    ui.painter().text(pos, align, text, theme::semibold(12.0), color);
    let clicked = resp.clicked();
    if clicked {
        spec.click(col, default_desc);
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::*;

    #[test]
    fn text_is_case_insensitive() {
        assert_eq!(cmp_text("apple", "Banana"), Less);
    }

    #[test]
    fn ip_numeric() {
        assert_eq!(cmp_ip("192.168.1.9", "192.168.1.102"), Less);
        assert_eq!(cmp_ip("garbage", "10.0.0.1"), Greater);
        assert_eq!(cmp_ip("10.0.0.1", "garbage"), Less);
    }

    #[test]
    fn none_last_both_directions() {
        assert_eq!(cmp_opt(None, Some(1), false), Greater);
        // Descending: `apply` reverses the key, so cmp_opt pre-flips None to Less (see apply_desc_keeps_none_last).
        assert_eq!(cmp_opt(None, Some(1), true), Less);
        assert_eq!(cmp_opt(Some(1), None::<i32>, true), Greater);
    }

    #[test]
    fn click_flips_then_switches() {
        let mut s = SortSpec { col: 0, desc: false };
        s.click(0, true);
        assert!(s.desc);
        s.click(2, true);
        assert_eq!(s, SortSpec { col: 2, desc: true });
    }

    #[test]
    fn apply_empty_and_single() {
        let spec = SortSpec { col: 0, desc: true };
        let mut e: Vec<(i32, &str)> = vec![];
        apply(&mut e, spec, |a, b, _| a.0.cmp(&b.0), |r| r.1);
        assert!(e.is_empty());
        let mut one = vec![(1, "x")];
        apply(&mut one, spec, |a, b, _| a.0.cmp(&b.0), |r| r.1);
        assert_eq!(one, vec![(1, "x")]);
    }

    #[test]
    fn apply_all_equal_ties_by_name() {
        let mut rows = vec![(1, "carol"), (1, "Alice"), (1, "bob")];
        apply(&mut rows, SortSpec { col: 0, desc: true }, |a, b, _| a.0.cmp(&b.0), |r| r.1);
        assert_eq!(rows.iter().map(|r| r.1).collect::<Vec<_>>(), ["Alice", "bob", "carol"]);
    }

    #[test]
    fn apply_desc_keeps_none_last() {
        let mut rows = vec![(Some(2), "a"), (None, "b"), (Some(5), "c")];
        let spec = SortSpec { col: 0, desc: true };
        apply(&mut rows, spec, |a, b, _| cmp_opt(a.0, b.0, spec.desc), |r| r.1);
        assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), [Some(5), Some(2), None]);
        let spec = SortSpec { col: 0, desc: false };
        apply(&mut rows, spec, |a, b, _| cmp_opt(a.0, b.0, spec.desc), |r| r.1);
        assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), [Some(2), Some(5), None]);
    }

    #[test]
    fn apply_passes_col_to_key() {
        let mut rows = vec![(1, 9, "a"), (2, 1, "b")];
        apply(&mut rows, SortSpec { col: 1, desc: false }, |a, b, col| if col == 0 { a.0.cmp(&b.0) } else { a.1.cmp(&b.1) }, |r| r.2);
        assert_eq!(rows[0].2, "b");
    }
}
