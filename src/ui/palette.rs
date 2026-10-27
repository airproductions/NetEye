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
    bg_glow, bg_mid, bg_deep, glass_top, glass_bottom, glass_border, shadow, mesh_line, mesh_node, glow,
);

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn dark() -> Palette {
    let glass_top = rgba(90, 140, 255, 10);
    Palette {
        bg: rgb(0x0a1426),
        bg_glow: rgb(0x143a72),
        bg_mid: rgb(0x0a1426),
        bg_deep: rgb(0x05080f),
        text: rgb(0xe6eefc),
        muted: rgb(0x8a97b6),
        faint: rgb(0x5c6884),
        accent: rgb(0x2f6bff),
        accent2: rgb(0x4fd1ff),
        down: rgb(0x4fd1ff),
        up: rgb(0xa78bfa),
        good: rgb(0x5ee0a8),
        bad: rgb(0xf87171),
        warn: rgb(0xfbbf24),
        orange: rgb(0xfb923c),
        glass_top,
        glass_bottom: rgba(255, 255, 255, 1),
        glass_border: rgba(140, 180, 255, 16),
        shadow: rgba(0, 0, 0, 90),
        panel: glass_top,
        raised: rgba(140, 170, 255, 4),
        row_alt: rgba(120, 160, 255, 1),
        hover: rgba(120, 170, 255, 8),
        selected: rgba(60, 120, 255, 30),
        border: rgba(140, 180, 255, 16),
        mesh_line: rgba(61, 139, 255, 14),
        mesh_node: rgba(127, 196, 255, 50),
        glow: rgba(60, 120, 255, 90),
    }
}

pub fn light() -> Palette {
    let glass_top = rgba(255, 255, 255, 173);
    Palette {
        bg: rgb(0xeef2f7),
        bg_glow: rgb(0xc7d5ec),
        bg_mid: rgb(0xe9eef5),
        bg_deep: rgb(0xf3f5f9),
        text: rgb(0x17243d),
        muted: rgb(0x6b7890),
        faint: rgb(0x9aa5b8),
        accent: rgb(0x2f6fd6),
        accent2: rgb(0x3c7be0),
        down: rgb(0x2f6fd6),
        up: rgb(0x7c5cd6),
        good: rgb(0x1f8a5c),
        bad: rgb(0xd64545),
        warn: rgb(0xb7791f),
        orange: rgb(0xc05621),
        glass_top,
        glass_bottom: rgba(255, 255, 255, 150),
        glass_border: rgba(255, 255, 255, 242),
        shadow: rgba(40, 70, 120, 20),
        panel: glass_top,
        raised: rgb(0xeaeff6),
        row_alt: rgba(23, 36, 61, 6),
        hover: rgba(36, 64, 122, 15),
        selected: rgb(0xe7eefb),
        border: rgba(23, 36, 61, 20),
        mesh_line: rgba(90, 120, 168, 22),
        mesh_node: rgba(90, 120, 168, 45),
        glow: rgba(47, 111, 214, 40),
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
