//! Palette, fonts, formatting helpers and the hand-drawn widgets shared by every page.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{
    self, epaint::Vertex, text::LayoutJob, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextWrapMode, TextureHandle, Ui, Vec2,
};

use crate::monitor::Shared;

pub mod c {
    use eframe::egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(0x14, 0x16, 0x1b);
    pub const PANEL: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x23);
    pub const RAISED: Color32 = Color32::from_rgb(0x21, 0x25, 0x2d);
    pub const ROW_ALT: Color32 = Color32::from_rgb(0x1d, 0x20, 0x27);
    pub const HOVER: Color32 = Color32::from_rgb(0x27, 0x2c, 0x36);
    pub const SELECTED: Color32 = Color32::from_rgb(0x23, 0x33, 0x52);
    pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2f, 0x39);
    pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xea, 0xf0);
    pub const MUTED: Color32 = Color32::from_rgb(0x8d, 0x95, 0xa5);
    pub const FAINT: Color32 = Color32::from_rgb(0x5c, 0x63, 0x70);
    pub const ACCENT: Color32 = Color32::from_rgb(0x5b, 0x8d, 0xff);
    pub const DOWN: Color32 = Color32::from_rgb(0x2d, 0xd4, 0xbf);
    pub const UP: Color32 = Color32::from_rgb(0xa7, 0x8b, 0xfa);
    pub const GOOD: Color32 = Color32::from_rgb(0x34, 0xd3, 0x99);
    pub const BAD: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
    pub const WARN: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
    pub const ORANGE: Color32 = Color32::from_rgb(0xfb, 0x92, 0x3c);
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let load = |f: &str| std::fs::read(format!("C:\\Windows\\Fonts\\{f}")).ok();
    if let Some(b) = load("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(b)));
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "segoe".into());
    }
    let semi = load("seguisb.ttf").map(|b| ("segoe-sb", b));
    let mut semi_family = fonts.families[&FontFamily::Proportional].clone();
    if let Some((name, b)) = semi {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(b)));
        semi_family.insert(0, name.into());
    }
    if let Some(b) = load("seguisym.ttf") {
        fonts.font_data.insert("symbols".into(), Arc::new(FontData::from_owned(b)));
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().push("symbols".into());
        semi_family.push("symbols".into());
    }
    fonts.families.insert(FontFamily::Name("semibold".into()), semi_family);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.text_styles.insert(egui::TextStyle::Body, regular(14.0));
    style.text_styles.insert(egui::TextStyle::Button, regular(14.0));
    style.text_styles.insert(egui::TextStyle::Small, regular(12.0));
    style.text_styles.insert(egui::TextStyle::Heading, semibold(20.0));
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = c::BG;
    v.window_fill = c::RAISED;
    v.window_stroke = Stroke::new(1.0_f32, c::BORDER);
    v.extreme_bg_color = c::PANEL;
    v.faint_bg_color = c::ROW_ALT;
    v.override_text_color = Some(c::TEXT);
    v.selection.bg_fill = c::SELECTED;
    v.selection.stroke = Stroke::new(1.0_f32, c::ACCENT);
    v.hyperlink_color = c::ACCENT;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, c::BORDER);
    v.widgets.inactive.weak_bg_fill = c::RAISED;
    v.widgets.inactive.bg_fill = c::RAISED;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = c::HOVER;
    v.widgets.hovered.bg_fill = c::HOVER;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = c::SELECTED;
    v.widgets.active.bg_fill = c::SELECTED;
    v.widgets.open.weak_bg_fill = c::HOVER;
    v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(120) };
    ctx.set_style(style);
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
    Log,
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
    Apps,
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
        Glyph::Log => {
            p.circle_stroke(pt(10.0, 10.0), 7.5 * u, s);
            line(&[(10.0, 5.5), (10.0, 10.0), (13.0, 12.0)]);
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
        Glyph::Apps => {
            for (x, y) in [(3.0, 3.0), (11.0, 3.0), (3.0, 11.0), (11.0, 11.0)] {
                p.rect_filled(Rect::from_min_max(pt(x, y), pt(x + 6.0, y + 6.0)), CornerRadius::same(1), color);
            }
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
    let col = if resp.hovered() { c::TEXT } else { c::MUTED };
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(6), c::HOVER);
    }
    glyph(ui.painter(), r.shrink(size * 0.22), g, col);
    resp.on_hover_text(tooltip)
}

/// Pill button with optional leading glyph.
pub fn pill(ui: &mut Ui, text_s: &str, g: Option<Glyph>, active: bool) -> Response {
    let font = semibold(13.5);
    let galley = ui.painter().layout_no_wrap(text_s.to_string(), font.clone(), c::TEXT);
    let w = galley.size().x + 28.0 + if g.is_some() { 22.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
    let fill = if active { c::SELECTED } else if resp.hovered() { c::HOVER } else { c::RAISED };
    ui.painter().rect_filled(r, CornerRadius::same(16), fill);
    let mut x = r.left() + 14.0;
    let col = if active { Color32::from_rgb(0x9d, 0xbc, 0xff) } else { c::TEXT };
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
    let fill = if resp.hovered() { Color32::from_rgb(0x6d, 0x9b, 0xff) } else { c::ACCENT };
    ui.painter().rect_filled(r, CornerRadius::same(8), fill);
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE);
    resp
}

/// iOS-style switch.
pub fn switch(ui: &mut Ui, on: bool) -> Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::click());
    let t = ui.ctx().animate_bool(resp.id, on);
    let bg = lerp_color(Color32::from_rgb(0x3a, 0x40, 0x4c), c::ACCENT, t);
    ui.painter().rect_filled(r, CornerRadius::same(11), bg);
    let x = egui::lerp(r.left() + 11.0..=r.right() - 11.0, t);
    ui.painter().circle_filled(Pos2::new(x, r.center().y), 8.0, Color32::WHITE);
    resp
}

/// Segmented control of glyph buttons.
pub fn segmented(ui: &mut Ui, items: &[(Glyph, &str)], selected: &mut usize) {
    let seg = Vec2::new(46.0, 32.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(seg.x * items.len() as f32 + 6.0, seg.y + 6.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(19), c::RAISED);
    for (i, (g, tip)) in items.iter().enumerate() {
        let sr = Rect::from_min_size(Pos2::new(r.left() + 3.0 + seg.x * i as f32, r.top() + 3.0), seg);
        let resp = ui.interact(sr, ui.id().with(("seg", i, tip)), Sense::click()).on_hover_text(*tip);
        if resp.clicked() {
            *selected = i;
        }
        let active = *selected == i;
        if active {
            ui.painter().rect_filled(sr, CornerRadius::same(16), c::ACCENT);
        } else if resp.hovered() {
            ui.painter().rect_filled(sr, CornerRadius::same(16), c::HOVER);
        }
        glyph(ui.painter(), Rect::from_center_size(sr.center(), Vec2::splat(18.0)), *g, if active { Color32::WHITE } else { c::MUTED });
    }
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
    let (r, _) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(17), c::RAISED);
    glyph(ui.painter(), Rect::from_center_size(Pos2::new(r.left() + 18.0, r.center().y), Vec2::splat(16.0)), Glyph::Search, c::MUTED);
    let inner = Rect::from_min_max(Pos2::new(r.left() + 34.0, r.top() + 2.0), Pos2::new(r.right() - 12.0, r.bottom() - 2.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.add(
        egui::TextEdit::singleline(value)
            .hint_text(egui::RichText::new(hint).color(c::FAINT))
            .frame(false)
            .desired_width(inner.width())
            .font(regular(14.0)),
    )
}

/// Allowed / Blocked status pill used by the firewall.
pub fn status_pill(ui: &mut Ui, rect: Rect, blocked: bool, enabled: bool, id: egui::Id) -> Response {
    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let (bg, fg, label) = if blocked {
        (Color32::from_rgb(0x4a, 0x1f, 0x24), c::BAD, "Blocked")
    } else {
        (Color32::from_rgb(0x17, 0x3b, 0x2f), c::GOOD, "Allowed")
    };
    let bg = if !enabled { Color32::from_rgb(0x25, 0x29, 0x31) } else if resp.hovered() { lerp_color(bg, fg, 0.18) } else { bg };
    let fg = if enabled { fg } else { c::FAINT };
    ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, semibold(12.5), fg);
    resp
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::default()
        .fill(c::PANEL)
        .stroke(Stroke::new(1.0_f32, c::BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(16))
        .show(ui, add);
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
    let top = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 80);
    let bottom = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 6);
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

/// Draws horizontal gridlines with a max label at the top right and a title at top left.
pub fn chart_frame(p: &Painter, rect: Rect, title: &str, max_label: &str) {
    let y = rect.top();
    p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, c::BORDER));
    let font = regular(13.0);
    let tr = p.text(Pos2::new(rect.left(), y), Align2::LEFT_CENTER, title, font.clone(), c::MUTED);
    p.rect_filled(tr.expand2(Vec2::new(6.0, 2.0)), CornerRadius::ZERO, c::BG);
    p.text(Pos2::new(rect.left(), y), Align2::LEFT_CENTER, title, font.clone(), c::MUTED);
    let mr = p.text(Pos2::new(rect.right(), y), Align2::RIGHT_CENTER, max_label, font.clone(), c::MUTED);
    p.rect_filled(mr.expand2(Vec2::new(6.0, 2.0)), CornerRadius::ZERO, c::BG);
    p.text(Pos2::new(rect.right(), y), Align2::RIGHT_CENTER, max_label, font, c::MUTED);
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
    area(p, rect, down, max, c::DOWN);
    area(p, rect, up, max, c::UP);
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
    p.text(center + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, msg, semibold(15.0), c::MUTED);
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
