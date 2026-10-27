//! Runtime colour palette. Pages read colours through `theme::c::*()` so dark and light can swap live.

use std::sync::RwLock;

use eframe::egui::Color32;

use super::theme::lerp_color;

macro_rules! palette {
    ($($f:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Palette {
            $(pub $f: Color32,)*
        }

        pub fn lerp(a: &Palette, b: &Palette, t: f32) -> Palette {
            Palette { $($f: lerp_color(a.$f, b.$f, t),)* }
        }
    };
}

palette!(
    bg, panel, raised, row_alt, hover, selected, border, text, muted, faint, accent, accent2, down, up, good, bad, warn, orange,
    shadow,
);

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub fn dark() -> Palette {
    Palette {
        bg: rgb(0x14161b),
        panel: rgb(0x1a1d23),
        raised: rgb(0x21252d),
        row_alt: rgb(0x1d2027),
        hover: rgb(0x272c36),
        selected: rgb(0x233352),
        border: rgb(0x2a2f39),
        text: rgb(0xe7eaf0),
        muted: rgb(0x9aa2b1),
        faint: rgb(0x7a8291),
        accent: rgb(0x5b8dff),
        accent2: rgb(0x5b8dff),
        down: rgb(0x2dd4bf),
        up: rgb(0xd08bfa),
        good: rgb(0x4ade80),
        bad: rgb(0xf87171),
        warn: rgb(0xfbbf24),
        orange: rgb(0xfb923c),
        shadow: Color32::from_black_alpha(60),
    }
}

pub fn light() -> Palette {
    Palette {
        bg: rgb(0xf3f4f7),
        panel: rgb(0xffffff),
        raised: rgb(0xeceef2),
        row_alt: rgb(0xf7f8fa),
        hover: rgb(0xe6e9ef),
        selected: rgb(0xdde7fb),
        border: rgb(0xdcdfe5),
        text: rgb(0x1b1f27),
        muted: rgb(0x4f5664),
        faint: rgb(0x656c7a),
        accent: rgb(0x3b6fe0),
        accent2: rgb(0x3b6fe0),
        down: rgb(0x0f9f8f),
        up: rgb(0x9b4fd6),
        good: rgb(0x16a34a),
        bad: rgb(0xdc2626),
        warn: rgb(0xb7791f),
        orange: rgb(0xc05621),
        shadow: Color32::from_black_alpha(30),
    }
}

static CURRENT: RwLock<Option<Palette>> = RwLock::new(None);
static GRAPH: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Graph colour preset (Settings → Graph colours); 0 keeps the theme's teal and orchid.
pub fn set_graph_colors(i: u8) {
    GRAPH.store(i, std::sync::atomic::Ordering::Relaxed);
}

/// (download, upload) for a preset on a dark or light theme. Preset 1 uses the Okabe–Ito
/// sky blue and orange, which stay distinct for the common kinds of colour blindness.
pub fn graph_pair(i: u8, light: bool) -> Option<(Color32, Color32)> {
    match (i, light) {
        (1, false) => Some((rgb(0x56b4e9), rgb(0xe69f00))),
        (1, true) => Some((rgb(0x0072b2), rgb(0xd55e00))),
        (2, false) => Some((rgb(0x22d3ee), rgb(0xf472b6))),
        (2, true) => Some((rgb(0x0891b2), rgb(0xdb2777))),
        _ => None,
    }
}

pub fn current() -> Palette {
    let mut p = CURRENT.read().ok().and_then(|p| *p).unwrap_or_else(dark);
    let light = p.bg.r() > 128;
    if let Some((down, up)) = graph_pair(GRAPH.load(std::sync::atomic::Ordering::Relaxed), light) {
        p.down = down;
        p.up = up;
    }
    p
}

pub fn set(p: Palette) {
    if let Ok(mut cur) = CURRENT.write() {
        *cur = Some(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    fn luminance(c: Color32) -> f32 {
        let ch = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    #[test]
    fn dark_is_classic_graphite() {
        let p = dark();
        assert_eq!(p.bg, rgb(0x14161b));
        assert_eq!(p.panel, rgb(0x1a1d23));
        assert_eq!(p.accent, rgb(0x5b8dff));
        assert_eq!(p.down, rgb(0x2dd4bf));
    }

    #[test]
    fn light_is_graphite_light() {
        let p = light();
        assert_eq!(p.bg, rgb(0xf3f4f7));
        assert_eq!(p.panel, rgb(0xffffff));
        assert_eq!(p.accent, rgb(0x3b6fe0));
    }

    #[test]
    fn lerp_endpoints() {
        assert_eq!(lerp(&dark(), &light(), 0.0).text, dark().text);
        assert_eq!(lerp(&dark(), &light(), 1.0).text, light().text);
    }

    #[test]
    fn set_then_current() {
        set(light());
        assert_eq!(current().bg, light().bg);
        set(dark());
        assert_eq!(current().bg, dark().bg);
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    fn hue(c: Color32) -> f32 {
        let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = (max - min).max(1e-6);
        let h = if max == r { ((g - b) / d).rem_euclid(6.0) } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
        h * 60.0
    }

    fn hue_gap(a: Color32, b: Color32) -> f32 {
        let d = (hue(a) - hue(b)).abs();
        d.min(360.0 - d)
    }

    #[test]
    fn small_text_meets_aa_on_every_surface() {
        for p in [dark(), light()] {
            for surface in [p.bg, p.panel, p.raised] {
                assert!(contrast(p.muted, surface) >= 4.5, "muted {:?} on {:?}", p.muted, surface);
                assert!(contrast(p.faint, surface) >= 3.9, "faint {:?} on {:?}", p.faint, surface);
            }
        }
    }

    #[test]
    fn data_hues_stay_apart_from_accent_and_status() {
        // download / upload must not read as the interactive accent or as a status colour
        for p in [dark(), light()] {
            assert!(hue_gap(p.up, p.accent) >= 40.0);
            assert!(hue_gap(p.down, p.accent) >= 40.0);
            assert!(hue_gap(p.down, p.up) >= 90.0);
            assert!(hue_gap(p.down, p.good) >= 25.0);
        }
    }

    #[test]
    fn graph_presets_keep_data_hues_apart() {
        for i in 1..=2 {
            for light in [false, true] {
                let (d, u) = graph_pair(i, light).unwrap();
                assert!(hue_gap(d, u) >= 90.0, "preset {i}");
            }
        }
        assert!(graph_pair(0, false).is_none());
    }

    #[test]
    fn text_contrasts_background() {
        for p in [dark(), light()] {
            assert!((luminance(p.text) - luminance(p.bg)).abs() > 0.5);
        }
    }
}
