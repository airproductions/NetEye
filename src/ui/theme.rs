//! Palette, fonts, formatting helpers and the hand-drawn widgets shared by every page.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{
    self, epaint::Vertex, text::LayoutJob, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextWrapMode, TextureHandle, Ui, Vec2,
};

use super::motion;
use crate::monitor::Shared;

pub mod c {
    //! Colour accessors for the active palette (see `palette`).
    use eframe::egui::Color32;
    macro_rules! accessors {
        ($($f:ident),*) => { $(pub fn $f() -> Color32 { super::super::palette::current().$f })* };
    }
    accessors!(
        bg, raised, row_alt, hover, selected, border, text, muted, faint, accent, accent2, down, up, good, bad, warn, orange,
        bg_glow, bg_mid, bg_deep, glass_top, glass_bottom, glass_border, shadow, mesh_line, mesh_node, glow
    );
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Manrope (bundled, SIL OFL) with egui's defaults and Segoe UI Symbol behind it for glyph coverage.
fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 3] = [
        ("manrope", include_bytes!("../../assets/fonts/Manrope-Regular.ttf")),
        ("manrope-sb", include_bytes!("../../assets/fonts/Manrope-SemiBold.ttf")),
        ("manrope-b", include_bytes!("../../assets/fonts/Manrope-Bold.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let symbols = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf").ok();
    if let Some(b) = symbols.clone() {
        fonts.font_data.insert("symbols".into(), Arc::new(FontData::from_owned(b)));
    }
    let family = |first: &str| {
        let mut f = vec![first.to_string()];
        f.extend(fallback.iter().cloned());
        if symbols.is_some() {
            f.push("symbols".into());
        }
        f
    };
    fonts.families.insert(FontFamily::Proportional, family("manrope"));
    fonts.families.insert(FontFamily::Name("semibold".into()), family("manrope-sb"));
    fonts.families.insert(FontFamily::Name("bold".into()), family("manrope-b"));
    fonts
}

pub fn setup(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());

    // both egui styles (dark and light) get the same metrics, so switching theme keeps the layout
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.text_styles.insert(egui::TextStyle::Body, regular(14.0));
        style.text_styles.insert(egui::TextStyle::Button, regular(14.0));
        style.text_styles.insert(egui::TextStyle::Small, regular(12.0));
        style.text_styles.insert(egui::TextStyle::Heading, semibold(20.0));
    });
    apply_visuals(ctx, &super::palette::current());
}

/// Rebuilds egui's own widget visuals (popups, text edits, scrollbars) from a palette.
pub fn apply_visuals(ctx: &egui::Context, p: &super::palette::Palette) {
    let is_dark = p.bg.r() as u32 + p.bg.g() as u32 + p.bg.b() as u32 <= 384;
    let mut v = if is_dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    // popups sit above glass, so they need an opaque fill
    let solid = lerp_color(p.bg, p.bg_glow, 0.35);
    v.panel_fill = p.bg;
    v.window_fill = solid;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.extreme_bg_color = p.panel;
    v.faint_bg_color = p.row_alt;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.selected;
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.hyperlink_color = p.accent;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.inactive.weak_bg_fill = p.raised;
    v.widgets.inactive.bg_fill = p.raised;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = p.selected;
    v.widgets.active.bg_fill = p.selected;
    v.widgets.open.weak_bg_fill = p.hover;
    v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: p.shadow };
    ctx.set_visuals(v);
}

// ---------------------------------------------------------------------------------------------
// Formatting

pub fn bytes(b: u64) -> String {
    let b = b as f64;
    if b < 1000.0 {
        format!("{b:.0} B")
    } else if b < 1e6 {
        format!("{:.1} KB", b / 1e3)
    } else if b < 1e9 {
        format!("{:.1} MB", b / 1e6)
    } else if b < 1e12 {
        format!("{:.2} GB", b / 1e9)
    } else {
        format!("{:.2} TB", b / 1e12)
    }
}

pub fn rate(b: f64) -> String {
    if b < 1.0 {
        return "0 B/s".into();
    }
    if b < 1000.0 {
        format!("{b:.0} B/s")
    } else if b < 1e6 {
        format!("{:.0} KB/s", b / 1e3)
    } else if b < 1e9 {
        format!("{:.1} MB/s", b / 1e6)
    } else {
        format!("{:.2} GB/s", b / 1e9)
    }
}

pub fn local(t: i64) -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::from_timestamp(t, 0).unwrap_or_default().with_timezone(&chrono::Local)
}

pub fn clock(t: i64) -> String {
    local(t).format("%-I:%M %p").to_string()
}

pub fn date_time(t: i64) -> String {
    local(t).format("%-d %b %Y, %-I:%M %p").to_string()
}

// ---------------------------------------------------------------------------------------------
// Text

/// Draws single-line text truncated with an ellipsis to `max_w`. `pos` is the left-center point.
pub fn text(painter: &Painter, pos: Pos2, s: &str, font: FontId, color: Color32, max_w: f32) -> Rect {
    let mut job = LayoutJob::simple_singleline(s.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(1.0));
    let galley = painter.layout_job(job);
    let p = Pos2::new(pos.x, pos.y - galley.size().y / 2.0);
    let r = Rect::from_min_size(p, galley.size());
    painter.galley(p, galley, color);
    r
}

pub fn text_right(painter: &Painter, pos: Pos2, s: &str, font: FontId, color: Color32) -> Rect {
    painter.text(pos, Align2::RIGHT_CENTER, s, font, color)
}

pub fn label(ui: &mut Ui, s: impl Into<String>, font: FontId, color: Color32) -> Response {
    ui.add(egui::Label::new(egui::RichText::new(s.into()).font(font).color(color)).wrap_mode(TextWrapMode::Truncate))
}

// ---------------------------------------------------------------------------------------------
// Glyphs: small vector icons, drawn rather than shipped as assets.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Traffic,
    Shield,
    Bell,
    Radar,
    Wifi,
    WifiOff,
    Clock,
    Grid,
    Scanner,
    Chip,
    Gear,
    Search,
    Down,
    Up,
    Router,
    Computer,
    Phone,
    Tv,
    Device,
    Plug,
    Info,
    Chevron,
    ChevronDown,
    Graph,
    Bars,
    Globe,
    Refresh,
    Check,
    Lock,
}

pub fn glyph(p: &Painter, r: Rect, g: Glyph, color: Color32) {
    let s = Stroke::new(1.6_f32, color);
    let c = r.center();
    let w = r.width().min(r.height());
    let u = w / 20.0; // design grid: 20x20
    let pt = |x: f32, y: f32| Pos2::new(c.x + (x - 10.0) * u, c.y + (y - 10.0) * u);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|&(x, y)| pt(x, y)).collect(), s));
    };
    match g {
        Glyph::Traffic => {
            line(&[(2.0, 17.0), (6.0, 6.0), (9.0, 12.0), (12.0, 3.0), (15.0, 13.0), (18.0, 17.0)]);
            line(&[(2.0, 17.0), (18.0, 17.0)]);
        }
        Glyph::Shield => {
            line(&[(10.0, 2.0), (17.0, 5.0), (17.0, 10.0), (14.5, 15.0), (10.0, 18.0), (5.5, 15.0), (3.0, 10.0), (3.0, 5.0), (10.0, 2.0)]);
            line(&[(6.8, 10.0), (9.0, 12.4), (13.4, 7.6)]);
        }
        Glyph::Bell => {
            line(&[(4.5, 14.0), (5.5, 12.5), (5.5, 9.0)]);
            p.add(Shape::line(arc(pt(10.0, 9.0), 4.5 * u, 3.1416, 6.2832), s));
            line(&[(14.5, 9.0), (14.5, 12.5), (15.5, 14.0), (4.5, 14.0)]);
            p.add(Shape::line(arc(pt(10.0, 14.0), 1.8 * u, 0.0, 3.1416), s));
        }
        Glyph::Radar => {
            p.circle_stroke(pt(10.0, 10.0), 7.5 * u, s);
            p.circle_stroke(pt(10.0, 10.0), 3.8 * u, s);
            line(&[(10.0, 10.0), (15.3, 4.7)]);
            p.circle_filled(pt(10.0, 10.0), 1.1 * u, color);
        }
        Glyph::Wifi => {
            for r in [3.0, 6.0, 9.0] {
                p.add(Shape::line(arc(pt(10.0, 15.0), r * u, -2.35, -0.79), s));
            }
            p.circle_filled(pt(10.0, 15.0), 1.2 * u, color);
        }
        Glyph::WifiOff => {
            for r in [3.0, 6.0, 9.0] {
                p.add(Shape::line(arc(pt(10.0, 15.0), r * u, -2.35, -0.79), s));
            }
            p.circle_filled(pt(10.0, 15.0), 1.2 * u, color);
            line(&[(3.0, 3.0), (17.0, 17.0)]);
        }
        Glyph::Clock => {
            p.circle_stroke(pt(10.0, 10.0), 7.5 * u, s);
            line(&[(10.0, 5.5), (10.0, 10.0), (13.0, 12.0)]);
        }
        Glyph::Grid => {
            for (x, y) in [(3.0, 3.0), (11.0, 3.0), (3.0, 11.0), (11.0, 11.0)] {
                p.rect_stroke(Rect::from_min_max(pt(x, y), pt(x + 6.0, y + 6.0)), CornerRadius::same(1), s, StrokeKind::Middle);
            }
        }
        Glyph::Scanner => {
            p.circle_stroke(pt(5.0, 6.0), 2.5 * u, s);
            p.circle_stroke(pt(15.0, 8.0), 2.5 * u, s);
            p.circle_stroke(pt(8.0, 15.5), 2.5 * u, s);
            line(&[(7.4, 6.5), (12.6, 7.5)]);
            line(&[(6.0, 8.3), (7.3, 13.1)]);
            line(&[(13.2, 9.9), (10.0, 13.8)]);
        }
        Glyph::Chip => {
            p.rect_stroke(Rect::from_min_max(pt(5.0, 5.0), pt(15.0, 15.0)), CornerRadius::same(2), s, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(pt(8.0, 8.0), pt(12.0, 12.0)), CornerRadius::ZERO, s, StrokeKind::Middle);
            for i in [7.5, 10.0, 12.5] {
                line(&[(i, 2.0), (i, 5.0)]);
                line(&[(i, 15.0), (i, 18.0)]);
                line(&[(2.0, i), (5.0, i)]);
                line(&[(15.0, i), (18.0, i)]);
            }
        }
        Glyph::Gear => {
            p.circle_stroke(pt(10.0, 10.0), 2.6 * u, s);
            let mut pts = Vec::new();
            for i in 0..=48 {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                let rr = if (i / 3) % 2 == 0 { 7.6 } else { 5.8 };
                pts.push(Pos2::new(c.x + a.cos() * rr * u, c.y + a.sin() * rr * u));
            }
            p.add(Shape::line(pts, s));
        }
        Glyph::Search => {
            p.circle_stroke(pt(8.5, 8.5), 5.5 * u, s);
            line(&[(12.5, 12.5), (17.0, 17.0)]);
        }
        Glyph::Down => {
            line(&[(10.0, 3.0), (10.0, 17.0)]);
            line(&[(5.0, 12.0), (10.0, 17.0), (15.0, 12.0)]);
        }
        Glyph::Up => {
            line(&[(10.0, 17.0), (10.0, 3.0)]);
            line(&[(5.0, 8.0), (10.0, 3.0), (15.0, 8.0)]);
        }
        Glyph::Router => {
            p.rect_stroke(Rect::from_min_max(pt(3.0, 12.0), pt(17.0, 17.0)), CornerRadius::same(2), s, StrokeKind::Middle);
            p.circle_filled(pt(6.0, 14.5), 0.9 * u, color);
            line(&[(13.0, 12.0), (13.0, 7.0)]);
            p.add(Shape::line(arc(pt(13.0, 7.0), 3.0 * u, -2.4, -0.7), s));
            p.add(Shape::line(arc(pt(13.0, 7.0), 5.5 * u, -2.4, -0.7), s));
        }
        Glyph::Computer => {
            p.rect_stroke(Rect::from_min_max(pt(2.5, 3.5), pt(17.5, 13.5)), CornerRadius::same(2), s, StrokeKind::Middle);
            line(&[(10.0, 13.5), (10.0, 16.5)]);
            line(&[(6.0, 16.5), (14.0, 16.5)]);
        }
        Glyph::Phone => {
            p.rect_stroke(Rect::from_min_max(pt(6.0, 2.5), pt(14.0, 17.5)), CornerRadius::same(3), s, StrokeKind::Middle);
            line(&[(9.0, 15.0), (11.0, 15.0)]);
        }
        Glyph::Tv => {
            p.rect_stroke(Rect::from_min_max(pt(2.5, 4.0), pt(17.5, 14.5)), CornerRadius::same(2), s, StrokeKind::Middle);
            p.add(Shape::line(arc(pt(4.0, 14.5), 3.0 * u, -1.57, 0.0), s));
            line(&[(7.0, 17.0), (13.0, 17.0)]);
        }
        Glyph::Device => {
            p.circle_stroke(pt(10.0, 10.0), 7.0 * u, s);
            p.circle_filled(pt(10.0, 10.0), 2.0 * u, color);
        }
        Glyph::Plug => {
            line(&[(4.0, 16.0), (7.5, 12.5)]);
            line(&[(16.0, 4.0), (12.5, 7.5)]);
            line(&[(7.0, 9.0), (11.0, 13.0)]);
            line(&[(9.0, 7.0), (13.0, 11.0)]);
            line(&[(7.0, 9.0), (9.0, 7.0)]);
            line(&[(11.0, 13.0), (13.0, 11.0)]);
        }
        Glyph::Info => {
            line(&[(10.0, 9.0), (10.0, 15.0)]);
            p.circle_filled(pt(10.0, 5.8), 1.2 * u, color);
        }
        Glyph::Chevron => line(&[(8.0, 5.0), (13.0, 10.0), (8.0, 15.0)]),
        Glyph::ChevronDown => line(&[(5.0, 8.0), (10.0, 13.0), (15.0, 8.0)]),
        Glyph::Graph => line(&[(2.0, 12.0), (6.0, 12.0), (8.0, 6.0), (11.0, 15.0), (13.0, 9.0), (14.5, 12.0), (18.0, 12.0)]),
        Glyph::Bars => {
            for (x, top) in [(4.0, 10.0), (8.0, 5.0), (12.0, 8.0), (16.0, 3.0)] {
                p.rect_filled(Rect::from_min_max(pt(x - 1.3, top), pt(x + 1.3, 17.0)), CornerRadius::same(1), color);
            }
        }
        Glyph::Globe => {
            p.circle_stroke(pt(10.0, 10.0), 7.5 * u, s);
            line(&[(2.5, 10.0), (17.5, 10.0)]);
            p.add(Shape::ellipse_stroke(pt(10.0, 10.0), Vec2::new(3.4 * u, 7.5 * u), s));
        }
        Glyph::Refresh => {
            p.add(Shape::line(arc(pt(10.0, 10.0), 6.5 * u, -0.6, 4.4), s));
            line(&[(15.5, 2.8), (15.5, 6.5), (11.8, 6.5)]);
        }
        Glyph::Check => line(&[(4.5, 10.5), (8.3, 14.3), (15.5, 6.0)]),
        Glyph::Lock => {
            p.rect_stroke(Rect::from_min_max(pt(4.5, 9.0), pt(15.5, 17.5)), CornerRadius::same(2), s, StrokeKind::Middle);
            p.add(Shape::line(arc(pt(10.0, 9.0), 3.6 * u, -3.1416, 0.0), s));
        }
    }
}

fn arc(center: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    (0..=24).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / 24.0;
        Pos2::new(center.x + a.cos() * r, center.y + a.sin() * r)
    }).collect()
}

// ---------------------------------------------------------------------------------------------
// Widgets

pub fn icon_button(ui: &mut Ui, g: Glyph, size: f32, tooltip: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let t = motion::hover(ui, resp.id, resp.hovered());
    if t > 0.0 {
        ui.painter().rect_filled(r, CornerRadius::same(8), lerp_color(Color32::TRANSPARENT, c::hover(), t));
    }
    glyph(ui.painter(), r.shrink(size * 0.22), g, lerp_color(c::muted(), c::text(), t));
    resp.on_hover_text(tooltip)
}

/// Pill button with optional leading glyph.
pub fn pill(ui: &mut Ui, text_s: &str, g: Option<Glyph>, active: bool) -> Response {
    let font = semibold(13.5);
    let galley = ui.painter().layout_no_wrap(text_s.to_string(), font.clone(), c::text());
    let w = galley.size().x + 28.0 + if g.is_some() { 22.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
    let t = motion::hover(ui, resp.id, resp.hovered());
    let fill = if active { c::selected() } else { lerp_color(c::raised(), c::hover(), t) };
    ui.painter().rect_filled(r, CornerRadius::same(16), fill);
    let mut x = r.left() + 14.0;
    let col = if active { c::accent2() } else { c::text() };
    if let Some(g) = g {
        glyph(ui.painter(), Rect::from_center_size(Pos2::new(x + 8.0, r.center().y), Vec2::splat(16.0)), g, col);
        x += 22.0;
    }
    ui.painter().galley(Pos2::new(x, r.center().y - galley.size().y / 2.0), galley, col);
    resp
}

pub fn primary_button(ui: &mut Ui, text_s: &str) -> Response {
    let font = semibold(13.5);
    let galley = ui.painter().layout_no_wrap(text_s.to_string(), font, Color32::WHITE);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 28.0, 32.0), Sense::click());
    let t = motion::hover(ui, resp.id, resp.hovered());
    glow(ui.painter(), r, 10, t);
    ui.painter().rect_filled(r, CornerRadius::same(10), lerp_color(c::accent(), c::accent2(), t * 0.35));
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE);
    resp
}

/// iOS-style switch.
pub fn switch(ui: &mut Ui, on: bool) -> Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, on, motion::FAST);
    let bg = lerp_color(c::raised(), c::accent(), t);
    glow(ui.painter(), r, 11, t * 0.6);
    ui.painter().rect_filled(r, CornerRadius::same(11), bg);
    ui.painter().rect_stroke(r, CornerRadius::same(11), Stroke::new(1.0_f32, lerp_color(c::border(), Color32::TRANSPARENT, t)), StrokeKind::Inside);
    let x = egui::lerp(r.left() + 11.0..=r.right() - 11.0, t);
    ui.painter().circle_filled(Pos2::new(x, r.center().y), 8.0, Color32::WHITE);
    resp
}

/// Shared track for segmented controls: draws the trough and a selection pill that slides between items.
fn segment_track(ui: &mut Ui, widths: &[f32], height: f32, selected: &mut usize, tips: &[&str], mut draw: impl FnMut(&Painter, Rect, usize, bool, f32)) {
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let (r, resp) = ui.allocate_exact_size(Vec2::new(total, height + 6.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, CornerRadius::same(12), c::raised());
    let mut rects = Vec::with_capacity(widths.len());
    let mut x = r.left() + 3.0;
    for w in widths {
        rects.push(Rect::from_min_size(Pos2::new(x, r.top() + 3.0), Vec2::new(*w, height)));
        x += w;
    }
    let mut hovered = Vec::with_capacity(rects.len());
    for (i, sr) in rects.iter().enumerate() {
        let item = ui.interact(*sr, resp.id.with(i), Sense::click());
        let item = match tips.get(i) {
            Some(t) if !t.is_empty() => item.on_hover_text(*t),
            _ => item,
        };
        if item.clicked() {
            *selected = i;
        }
        hovered.push(motion::hover(ui, item.id, item.hovered()));
    }
    // the pill glides from the previous selection to the new one
    let pos = motion::value(ui.ctx(), resp.id, *selected as f32);
    let (i0, frac) = (pos.floor() as usize, pos.fract());
    let a = rects[i0.min(rects.len() - 1)];
    let b = rects[(i0 + 1).min(rects.len() - 1)];
    let pill = Rect::from_min_max(a.min.lerp(b.min, frac), a.max.lerp(b.max, frac));
    glow(&p, pill, 9, 0.7);
    p.rect_filled(pill, CornerRadius::same(9), c::accent());
    for (i, sr) in rects.iter().enumerate() {
        let active = *selected == i;
        if !active && hovered[i] > 0.0 {
            p.rect_filled(*sr, CornerRadius::same(9), lerp_color(Color32::TRANSPARENT, c::hover(), hovered[i]));
        }
        draw(&p, *sr, i, active, hovered[i]);
    }
}

/// Segmented control of glyph buttons.
pub fn segmented(ui: &mut Ui, items: &[(Glyph, &str)], selected: &mut usize) {
    let widths = vec![40.0; items.len()];
    let tips: Vec<&str> = items.iter().map(|(_, t)| *t).collect();
    segment_track(ui, &widths, 30.0, selected, &tips, |p, r, i, active, h| {
        let col = if active { Color32::WHITE } else { lerp_color(c::muted(), c::text(), h) };
        glyph(p, Rect::from_center_size(r.center(), Vec2::splat(17.0)), items[i].0, col);
    });
}

/// Segmented control of short text labels, with optional per-item tooltips.
pub fn segmented_text(ui: &mut Ui, items: &[&str], selected: &mut usize, tooltip: Option<&[&str]>) {
    let font = semibold(12.5);
    let widths: Vec<f32> = items.iter().map(|s| ui.painter().layout_no_wrap(s.to_string(), font.clone(), c::text()).size().x + 22.0).collect();
    let tips = tooltip.unwrap_or(&[]);
    segment_track(ui, &widths, 28.0, selected, tips, |p, r, i, active, h| {
        let col = if active { Color32::WHITE } else { lerp_color(c::muted(), c::text(), h) };
        p.text(r.center(), Align2::CENTER_CENTER, items[i], font.clone(), col);
    });
}

/// Rounded dropdown. Returns true when the selection changed.
pub fn dropdown(ui: &mut Ui, id: &str, options: &[&str], selected: &mut usize) -> bool {
    let before = *selected;
    egui::ComboBox::from_id_salt(id)
        .selected_text(egui::RichText::new(options[*selected]).font(semibold(13.5)))
        .width(0.0)
        .show_ui(ui, |ui| {
            for (i, o) in options.iter().enumerate() {
                ui.selectable_value(selected, i, *o);
            }
        });
    before != *selected
}

pub fn search_box(ui: &mut Ui, value: &mut String, width: f32, hint: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::hover());
    let t = motion::hover(ui, resp.id, resp.hovered());
    ui.painter().rect_filled(r, CornerRadius::same(17), lerp_color(c::raised(), c::hover(), t));
    ui.painter().rect_stroke(r, CornerRadius::same(17), Stroke::new(1.0_f32, c::border()), StrokeKind::Inside);
    glyph(ui.painter(), Rect::from_center_size(Pos2::new(r.left() + 18.0, r.center().y), Vec2::splat(16.0)), Glyph::Search, c::muted());
    let inner = Rect::from_min_max(Pos2::new(r.left() + 34.0, r.top() + 2.0), Pos2::new(r.right() - 12.0, r.bottom() - 2.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.add(
        egui::TextEdit::singleline(value)
            .hint_text(egui::RichText::new(hint).color(c::faint()))
            .frame(false)
            .desired_width(inner.width())
            .font(regular(14.0)),
    )
}

/// Allowed / Blocked status pill used by the firewall.
pub fn status_pill(ui: &mut Ui, rect: Rect, blocked: bool, enabled: bool, id: egui::Id) -> Response {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let (bg, fg, label) = if blocked {
        (c::bad().gamma_multiply(36.0 / 255.0), c::bad(), "Blocked")
    } else {
        (c::good().gamma_multiply(36.0 / 255.0), c::good(), "Allowed")
    };
    let t = motion::hover(ui, id, resp.hovered());
    let bg = if !enabled { c::raised() } else { lerp_color(bg, fg, 0.18 * t) };
    let fg = if enabled { fg } else { c::faint() };
    ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, semibold(12.5), fg);
    resp
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let slot = ui.painter().add(Shape::Noop);
    let resp = egui::Frame::default().corner_radius(CornerRadius::same(14)).inner_margin(egui::Margin::same(16)).show(ui, add);
    ui.painter().set(slot, glass_shape(resp.response.rect, 14));
}

// ---------------------------------------------------------------------------------------------
// Glass

/// Outline of a rounded rectangle, clockwise from the top-left arc.
fn rounded_outline(r: Rect, radius: f32) -> Vec<Pos2> {
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0);
    let corners = [
        (Pos2::new(r.left() + rad, r.top() + rad), std::f32::consts::PI),
        (Pos2::new(r.right() - rad, r.top() + rad), std::f32::consts::PI * 1.5),
        (Pos2::new(r.right() - rad, r.bottom() - rad), 0.0),
        (Pos2::new(r.left() + rad, r.bottom() - rad), std::f32::consts::PI * 0.5),
    ];
    let mut pts = Vec::with_capacity(4 * 7);
    for (c0, a0) in corners {
        for i in 0..=6 {
            let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / 6.0;
            pts.push(c0 + Vec2::new(a.cos(), a.sin()) * rad);
        }
    }
    pts
}

/// Shadow, diagonal translucent gradient and hairline border: the "glass" card look.
pub fn glass_shape(r: Rect, radius: u8) -> Shape {
    let shadow = egui::Shadow { offset: [0, 10], blur: 28, spread: 0, color: c::shadow() };
    let outline = rounded_outline(r, radius as f32);
    let (top, bottom) = (c::glass_top(), c::glass_bottom());
    let diag = |q: Pos2| (((q.x - r.left()) / r.width().max(1.0)) + ((q.y - r.top()) / r.height().max(1.0))) / 2.0;
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex { pos: r.center(), uv: egui::epaint::WHITE_UV, color: lerp_color(top, bottom, 0.5) });
    for q in &outline {
        mesh.vertices.push(Vertex { pos: *q, uv: egui::epaint::WHITE_UV, color: lerp_color(top, bottom, diag(*q).clamp(0.0, 1.0)) });
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    let mut border = outline;
    border.push(border[0]);
    Shape::Vec(vec![
        shadow.as_shape(r, CornerRadius::same(radius)).into(),
        Shape::mesh(mesh),
        Shape::line(border, Stroke::new(1.0_f32, c::glass_border())),
    ])
}

pub fn glass(p: &Painter, r: Rect, radius: u8) {
    p.add(glass_shape(r, radius));
}

/// Frame-based glass for layouts that need a `Frame` (flat fill, no gradient).
pub fn glass_frame() -> egui::Frame {
    egui::Frame::default()
        .fill(c::glass_top())
        .stroke(Stroke::new(1.0_f32, c::glass_border()))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(14))
        .shadow(egui::Shadow { offset: [0, 10], blur: 28, spread: 0, color: c::shadow() })
}

/// Soft outer glow around an active element; `t` scales its strength (0..1).
pub fn glow(p: &Painter, r: Rect, radius: u8, t: f32) {
    if t <= 0.0 {
        return;
    }
    let col = c::glow().gamma_multiply(t.min(1.0));
    p.add(egui::Shadow { offset: [0, 0], blur: 18, spread: 0, color: col }.as_shape(r, CornerRadius::same(radius)));
}

// ---------------------------------------------------------------------------------------------
// Charts

/// Filled area series with a vertical gradient. `values` are evenly spaced across `rect`.
pub fn area(p: &Painter, rect: Rect, values: &[f64], max: f64, color: Color32) {
    if values.len() < 2 || max <= 0.0 {
        return;
    }
    let n = values.len() - 1;
    let pts: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.left() + rect.width() * i as f32 / n as f32;
            let y = rect.bottom() - (v / max).clamp(0.0, 1.0) as f32 * rect.height();
            Pos2::new(x, y)
        })
        .collect();
    let top = with_alpha(color, 80);
    let bottom = with_alpha(color, 6);
    let mut mesh = Mesh::default();
    for (i, pnt) in pts.iter().enumerate() {
        let t = ((rect.bottom() - pnt.y) / rect.height()).clamp(0.0, 1.0);
        mesh.vertices.push(Vertex { pos: *pnt, uv: egui::epaint::WHITE_UV, color: lerp_color(bottom, top, 0.35 + 0.65 * t) });
        mesh.vertices.push(Vertex { pos: Pos2::new(pnt.x, rect.bottom()), uv: egui::epaint::WHITE_UV, color: bottom });
        if i > 0 {
            let b = (i as u32 - 1) * 2;
            mesh.add_triangle(b, b + 1, b + 2);
            mesh.add_triangle(b + 1, b + 3, b + 2);
        }
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(pts, Stroke::new(1.6_f32, color)));
}

/// The parts of `left..right` not covered by any of `holes` (each `(start, end)`), in order.
pub fn gaps(left: f32, right: f32, holes: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut holes = holes.to_vec();
    holes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut x = left;
    for (a, b) in holes {
        if a > x {
            out.push((x, a.min(right)));
        }
        x = x.max(b);
        if x >= right {
            break;
        }
    }
    if x < right {
        out.push((x, right));
    }
    out.retain(|(a, b)| b > a);
    out
}

/// `col` with its alpha replaced by `a`, keeping the unmultiplied colour (`Color32` channels are premultiplied).
pub fn with_alpha(col: Color32, a: u8) -> Color32 {
    let [r, g, b, _] = col.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Draws horizontal gridlines with a max label at the top right and a title at top left.
pub fn chart_frame(p: &Painter, rect: Rect, title: &str, max_label: &str) {
    const PAD: f32 = 8.0;
    let y = rect.top();
    let font = regular(13.0);
    let tg = p.layout_no_wrap(title.to_string(), font.clone(), c::muted());
    let tr = Align2::LEFT_CENTER.anchor_size(Pos2::new(rect.left(), y), tg.size());
    let mg = p.layout_no_wrap(max_label.to_string(), font, c::muted());
    let mr = Align2::RIGHT_CENTER.anchor_size(Pos2::new(rect.right(), y), mg.size());
    // the gridline stops short of each label, since there is no solid background to knock it out with
    let holes = [(tr.left() - PAD, tr.right() + PAD), (mr.left() - PAD, mr.right() + PAD)];
    for (a, b) in gaps(rect.left(), rect.right(), &holes) {
        p.line_segment([Pos2::new(a, y), Pos2::new(b, y)], Stroke::new(1.0_f32, c::border()));
    }
    p.galley(tr.min, tg, c::muted());
    p.galley(mr.min, mg, c::muted());
}

/// Rounds a maximum up to a "nice" axis value.
pub fn nice_max(v: f64) -> f64 {
    if v <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(v.log10().floor());
    for m in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if v <= m * mag {
            return m * mag;
        }
    }
    10.0 * mag
}

/// Tiny sparkline used in table rows.
pub fn spark(p: &Painter, rect: Rect, down: &[f64], up: &[f64]) {
    let max = down.iter().chain(up.iter()).cloned().fold(0.0, f64::max);
    if max <= 0.0 {
        return;
    }
    area(p, rect, down, max, c::down());
    area(p, rect, up, max, c::up());
}

pub fn empty_state(p: &Painter, center: Pos2, msg: &str) {
    let col = Color32::from_rgb(0x4a, 0x50, 0x5c);
    // a small landscape: sun, hills and a signal mast
    p.circle_filled(center + Vec2::new(-40.0, -70.0), 11.0, col);
    let ground = center.y - 20.0;
    p.line_segment([Pos2::new(center.x - 90.0, ground), Pos2::new(center.x + 90.0, ground)], Stroke::new(2.0_f32, col));
    p.add(Shape::convex_polygon(
        vec![Pos2::new(center.x - 70.0, ground), Pos2::new(center.x - 45.0, ground - 22.0), Pos2::new(center.x - 20.0, ground)],
        col,
        Stroke::NONE,
    ));
    let mx = center.x + 35.0;
    p.line_segment([Pos2::new(mx, ground), Pos2::new(mx, ground - 55.0)], Stroke::new(3.0_f32, col));
    p.line_segment([Pos2::new(mx - 12.0, ground), Pos2::new(mx, ground - 30.0)], Stroke::new(2.0_f32, col));
    p.line_segment([Pos2::new(mx + 12.0, ground), Pos2::new(mx, ground - 30.0)], Stroke::new(2.0_f32, col));
    p.add(Shape::line(arc(Pos2::new(mx, ground - 55.0), 10.0, -2.6, -0.5), Stroke::new(2.0_f32, col)));
    p.add(Shape::line(arc(Pos2::new(mx, ground - 55.0), 18.0, -2.6, -0.5), Stroke::new(2.0_f32, col)));
    p.text(center + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, msg, semibold(15.0), c::muted());
}

// ---------------------------------------------------------------------------------------------
// App icons

#[derive(Default)]
pub struct Icons {
    textures: HashMap<String, Option<TextureHandle>>,
}

impl Icons {
    pub fn draw(&mut self, ui: &Ui, s: &mut Shared, key: &str, rect: Rect, name: &str) {
        if !self.textures.contains_key(key) {
            match s.icons.get(key) {
                Some(Some((w, h, px))) => {
                    let img = egui::ColorImage::from_rgba_unmultiplied([*w, *h], px);
                    let tex = ui.ctx().load_texture(format!("icon:{key}"), img, egui::TextureOptions::LINEAR);
                    self.textures.insert(key.to_string(), Some(tex));
                }
                Some(None) => {
                    self.textures.insert(key.to_string(), None);
                }
                None => {
                    s.icon_requests.insert(key.to_string());
                }
            }
        }
        match self.textures.get(key) {
            Some(Some(t)) => {
                ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            _ => fallback_icon(ui.painter(), rect, name),
        }
    }
}

pub fn fallback_icon(p: &Painter, rect: Rect, name: &str) {
    let hue = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) % 360;
    let col: Color32 = egui::ecolor::Hsva::new(hue as f32 / 360.0, 0.45, 0.55, 1.0).into();
    p.rect_filled(rect, CornerRadius::same(4), col);
    let letter = name.chars().find(|c| c.is_alphanumeric()).unwrap_or('?').to_ascii_uppercase();
    p.text(rect.center(), Align2::CENTER_CENTER, letter, semibold(rect.height() * 0.62), Color32::WHITE);
}

/// Procedural logo, used for the window and tray icons.
pub fn logo_rgba(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let r = s * 0.22;
            // rounded-square mask
            let dx = (fx - s / 2.0).abs() - (s / 2.0 - r);
            let dy = (fy - s / 2.0).abs() - (s / 2.0 - r);
            let d = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r;
            let a = (0.5 - d).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let t = (fx + fy) / (2.0 * s);
            let (r0, g0, b0) = (0x3d as f32, 0x7b as f32, 0xff as f32);
            let (r1, g1, b1) = (0x14 as f32, 0xc8 as f32, 0xb4 as f32);
            let mut col = [r0 + (r1 - r0) * t, g0 + (g1 - g0) * t, b0 + (b1 - b0) * t];
            // white "V" made of two strokes with a pulse dot
            let seg = |ax: f32, ay: f32, bx: f32, by: f32| {
                let (px_, py_) = (fx / s, fy / s);
                let (vx, vy) = (bx - ax, by - ay);
                let h = (((px_ - ax) * vx + (py_ - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
                ((px_ - ax - vx * h).powi(2) + (py_ - ay - vy * h).powi(2)).sqrt() * s
            };
            let dv = seg(0.27, 0.30, 0.50, 0.74).min(seg(0.50, 0.74, 0.73, 0.30));
            let w = s * 0.055;
            let white = (w - dv + 0.5).clamp(0.0, 1.0);
            for ch in &mut col {
                *ch = *ch + (255.0 - *ch) * white;
            }
            let i = ((y * size + x) * 4) as usize;
            px[i] = col[0] as u8;
            px[i + 1] = col[1] as u8;
            px[i + 2] = col[2] as u8;
            px[i + 3] = (a * 255.0) as u8;
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_color_keeps_translucent_endpoints() {
        let a = Color32::from_rgba_unmultiplied(90, 140, 255, 28);
        let b = Color32::from_rgba_unmultiplied(255, 255, 255, 173);
        assert_eq!(lerp_color(a, b, 0.0), a);
        assert_eq!(lerp_color(a, b, 1.0), b);
    }

    #[test]
    fn lerp_color_from_transparent_midpoint() {
        let c = Color32::from_rgba_unmultiplied(120, 170, 255, 26);
        let m = lerp_color(Color32::TRANSPARENT, c, 0.5);
        assert!((m.a() as i32 - c.a() as i32 / 2).abs() <= 1, "alpha {}", m.a());
        assert!((m.r() as i32 - c.r() as i32 / 2).abs() <= 1, "red {}", m.r());
    }

    #[test]
    fn with_alpha_on_translucent_keeps_unmultiplied_colour() {
        // the old `from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)` fed premultiplied channels back in and darkened the colour
        let c = Color32::from_rgba_unmultiplied(60, 120, 255, 90);
        let w = with_alpha(c, 45);
        assert_eq!(w.a(), 45);
        let (got, want) = (w.to_srgba_unmultiplied(), c.to_srgba_unmultiplied());
        for i in 0..3 {
            assert!((got[i] as i32 - want[i] as i32).abs() <= 3, "{got:?} vs {want:?}");
        }
    }

    #[test]
    fn gamma_multiply_scales_translucent_alpha() {
        // the replacement used for glow(), the rail strip and the mesh halo: egui blends premultiplied
        // channels, so a weaker copy of a colour keeps each channel's ratio to alpha
        let c = Color32::from_rgba_unmultiplied(60, 120, 255, 90);
        let ratios = |x: Color32| [x.r(), x.g(), x.b()].map(|v| v as f32 / x.a() as f32);
        let off = |x: Color32| ratios(x).iter().zip(ratios(c)).map(|(a, b)| ((a - b) / b).abs()).fold(0.0, f32::max);
        let h = c.gamma_multiply(0.25);
        assert_eq!(h.a(), (c.a() as f32 * 0.25 + 0.5) as u8);
        assert!(off(h) < 0.1, "{h:?} vs {c:?}");
        let old = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), c.a() / 4);
        assert!(off(old) > 0.25, "old pattern shifts the colour: {old:?} vs {c:?}");
    }

    #[test]
    fn gaps_without_holes() {
        assert_eq!(gaps(0.0, 100.0, &[]), vec![(0.0, 100.0)]);
    }

    #[test]
    fn gaps_skip_holes() {
        assert_eq!(gaps(0.0, 100.0, &[(20.0, 30.0)]), vec![(0.0, 20.0), (30.0, 100.0)]);
        assert_eq!(gaps(0.0, 100.0, &[(80.0, 110.0), (-5.0, 10.0)]), vec![(10.0, 80.0)]);
    }

    #[test]
    fn gaps_merge_overlapping_holes() {
        assert_eq!(gaps(0.0, 100.0, &[(25.0, 40.0), (10.0, 30.0)]), vec![(0.0, 10.0), (40.0, 100.0)]);
    }

    #[test]
    fn gaps_fully_covered() {
        assert!(gaps(0.0, 100.0, &[(-1.0, 60.0), (50.0, 101.0)]).is_empty());
        assert!(gaps(0.0, 100.0, &[(0.0, 100.0)]).is_empty());
    }

    #[test]
    fn fonts_bundled() {
        let f = font_definitions();
        for key in ["manrope", "manrope-sb", "manrope-b"] {
            assert!(f.font_data.contains_key(key), "missing {key}");
        }
        assert_eq!(f.families[&FontFamily::Proportional][0], "manrope");
        assert_eq!(f.families[&FontFamily::Name("bold".into())][0], "manrope-b");
    }
}
