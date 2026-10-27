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
        muted: rgb(0x8d95a5),
        faint: rgb(0x5c6370),
        accent: rgb(0x5b8dff),
        accent2: rgb(0x5b8dff),
        down: rgb(0x2dd4bf),
        up: rgb(0xa78bfa),
        good: rgb(0x34d399),
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
        muted: rgb(0x5f6776),
        faint: rgb(0x9aa1ad),
        accent: rgb(0x3b6fe0),
        accent2: rgb(0x3b6fe0),
        down: rgb(0x0f9f8f),
        up: rgb(0x7c5cd6),
        good: rgb(0x16a34a),
        bad: rgb(0xdc2626),
        warn: rgb(0xb7791f),
        orange: rgb(0xc05621),
        shadow: Color32::from_black_alpha(30),
    }
}

static CURRENT: RwLock<Option<Palette>> = RwLock::new(None);

pub fn current() -> Palette {
    CURRENT.read().ok().and_then(|p| *p).unwrap_or_else(dark)
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

    #[test]
    fn text_contrasts_background() {
        for p in [dark(), light()] {
            assert!((luminance(p.text) - luminance(p.bg)).abs() > 0.5);
        }
    }
}
