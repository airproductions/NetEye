//! Country flags and the two-letter chips drawn in their place.
//!
//! The flags come from one atlas PNG that `tools/flag-atlas` rasterised offline from
//! lipis/flag-icons (MIT); `assets/flags/README.md` records the source and layout.

use std::collections::HashMap;
use std::sync::OnceLock;

use eframe::egui::{self, Align2, Color32, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Ui};

use super::theme::{self, c};

/// A flag's size in the atlas, in pixels.
pub const CELL: [u32; 2] = [32, 24];
/// The border around each flag in the atlas, in pixels. It repeats the flag's edge pixels.
pub const PAD: u32 = 1;
/// The logical size every caller draws a flag or chip at.
pub const SIZE: egui::Vec2 = egui::vec2(16.0, 12.0);

const ATLAS: &[u8] = include_bytes!("../../assets/flags/atlas.png");
const INDEX: &str = include_str!("../../assets/flags/index.txt");
/// Distance between atlas cells: the flag plus its border on both sides.
const PITCH: [u32; 2] = [CELL[0] + 2 * PAD, CELL[1] + 2 * PAD];

/// `index.txt`, parsed: the grid size and each code's cell.
struct Index {
    cols: usize,
    rows: usize,
    cells: HashMap<String, usize>,
}

/// First line: the column count. Then one code per cell, row-major. A bad header gives no flags.
fn parse(text: &str) -> Index {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(cols) = lines.next().and_then(|l| l.parse::<usize>().ok()).filter(|&n| n > 0) else {
        return Index { cols: 1, rows: 0, cells: HashMap::new() };
    };
    let codes: Vec<String> = lines.map(str::to_ascii_lowercase).collect();
    let rows = codes.len().div_ceil(cols);
    Index { cols, rows, cells: codes.into_iter().enumerate().map(|(i, code)| (code, i)).collect() }
}

fn index() -> &'static Index {
    static INDEX_CELLS: OnceLock<Index> = OnceLock::new();
    INDEX_CELLS.get_or_init(|| parse(INDEX))
}

/// The atlas cell for `code`, matched case-insensitively.
fn lookup(index: &Index, code: &str) -> Option<usize> {
    index.cells.get(&code.to_ascii_lowercase()).copied()
}

/// The atlas texture. It is decoded and uploaded on first use, then cached in `ctx.data`. A PNG
/// that doesn't decode, or doesn't match the index, caches `None`, so it is only tried once.
fn atlas(ctx: &egui::Context) -> Option<TextureHandle> {
    let id = egui::Id::new("vantage.flags.atlas");
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return cached;
    }
    let idx = index();
    let want = (idx.cols * PITCH[0] as usize, idx.rows * PITCH[1] as usize);
    let tex = eframe::icon_data::from_png_bytes(ATLAS)
        .ok()
        .filter(|img| (img.width as usize, img.height as usize) == want)
        .map(|img| {
            let image = egui::ColorImage::from_rgba_unmultiplied([want.0, want.1], &img.rgba);
            ctx.load_texture("flags-atlas", image, egui::TextureOptions::LINEAR)
        });
    if tex.is_none() {
        crate::diag::log("flag atlas failed to load; drawing chips");
    }
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// The UV rect of atlas cell `index` in a `cols` x `rows` grid: the flag only, without its border.
pub fn uv(index: usize, cols: usize, rows: usize) -> Rect {
    let cols = cols.max(1);
    let (pw, ph) = (PITCH[0] as f32, PITCH[1] as f32);
    let (w, h) = (cols as f32 * pw, rows as f32 * ph);
    let (col, row) = ((index % cols) as f32, (index / cols) as f32);
    let min = Pos2::new((col * pw + PAD as f32) / w, (row * ph + PAD as f32) / h);
    Rect::from_min_size(min, egui::vec2(CELL[0] as f32 / w, CELL[1] as f32 / h))
}

/// Draws the flag for an ISO country code (any case) into `rect`, normally `SIZE`. `LAN`, `ZZ`,
/// a code without a flag, or an atlas that failed to load, gets a `chip` instead.
pub fn draw(ui: &Ui, rect: Rect, code: &str) {
    let idx = index();
    let p = ui.painter();
    match lookup(idx, code).and_then(|i| Some((i, atlas(ui.ctx())?))) {
        Some((i, tex)) => {
            p.image(tex.id(), rect, uv(i, idx.cols, idx.rows), Color32::WHITE);
            // A faint inner edge keeps white-edged flags (Japan, Israel, ...) distinct from a light background.
            p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0_f32, theme::with_alpha(c::text(), 28)), StrokeKind::Inside);
        }
        None => chip(p, rect, code),
    }
}

/// A rounded tile with the code's first two letters, or a small network diagram for `LAN`.
pub fn chip(p: &Painter, rect: Rect, code: &str) {
    p.rect_filled(rect, CornerRadius::same(3), c::raised());
    if code.eq_ignore_ascii_case("lan") {
        return network(p, rect, c::muted());
    }
    let mut letters: String = code.chars().filter(char::is_ascii_alphanumeric).take(2).collect();
    letters.make_ascii_uppercase();
    if letters.is_empty() {
        letters.push('?');
    }
    p.text(rect.center(), Align2::CENTER_CENTER, letters, theme::semibold(rect.height() * 0.66), c::muted());
}

/// One node above two, joined by a bus: the local-network mark, drawn on a 12-unit-high grid.
fn network(p: &Painter, rect: Rect, color: Color32) {
    let u = rect.height().min(rect.width() * 0.75) / 12.0;
    let at = |x: f32, y: f32| rect.center() + egui::vec2(x * u, y * u);
    let s = Stroke::new(u, color);
    p.line_segment([at(0.0, -2.5), at(0.0, 0.0)], s);
    p.line_segment([at(-3.5, 0.0), at(3.5, 0.0)], s);
    p.line_segment([at(-3.5, 0.0), at(-3.5, 2.5)], s);
    p.line_segment([at(3.5, 0.0), at(3.5, 2.5)], s);
    for (x, y) in [(0.0, -2.8), (-3.5, 2.8), (3.5, 2.8)] {
        p.rect_filled(Rect::from_center_size(at(x, y), egui::vec2(2.6 * u, 2.2 * u)), CornerRadius::ZERO, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_inner_rect() {
        let r = uv(0, 16, 17);
        assert_eq!(r.min, Pos2::new(1.0 / 544.0, 1.0 / 442.0));
        assert_eq!(r.max, Pos2::new(1.0 / 544.0 + 32.0 / 544.0, 1.0 / 442.0 + 24.0 / 442.0));
        let last = uv(16 * 17 - 1, 16, 17);
        assert_eq!(last.min, Pos2::new((15.0 * 34.0 + 1.0) / 544.0, (16.0 * 26.0 + 1.0) / 442.0));
        assert!((last.max.x - 543.0 / 544.0).abs() < 1e-6 && (last.max.y - 441.0 / 442.0).abs() < 1e-6, "{last:?}");
    }

    #[test]
    fn index_parses() {
        let idx = index();
        assert!(idx.cells.len() >= 245, "{} codes", idx.cells.len());
        for code in ["il", "it", "us", "xk"] {
            assert!(idx.cells.contains_key(code), "{code} missing");
        }
        assert!(idx.cells.keys().all(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_lowercase())), "sub-region or malformed code");
        assert_eq!(idx.rows, idx.cells.len().div_ceil(idx.cols));
    }

    #[test]
    fn atlas_decodes() {
        let idx = index();
        let img = eframe::icon_data::from_png_bytes(ATLAS).expect("atlas.png decodes");
        assert!(idx.rows > 0);
        assert_eq!((img.width as usize, img.height as usize), (idx.cols * 34, idx.rows * 26));
    }

    #[test]
    fn draw_lowercases() {
        let idx = index();
        assert!(lookup(idx, "IL").is_some());
        assert_eq!(lookup(idx, "IL"), lookup(idx, "il"));
        assert_eq!(lookup(idx, "ZZ"), None);
        assert_eq!(lookup(idx, "LAN"), None);
    }

    #[test]
    fn atlas_loads_once() {
        let ctx = egui::Context::default();
        let a = atlas(&ctx).expect("atlas texture");
        let b = atlas(&ctx).expect("cached atlas texture");
        assert_eq!(a.id(), b.id());
        let idx = index();
        assert_eq!(a.size(), [idx.cols * 34, idx.rows * 26]);
    }

    /// Runs one frame that draws `code`; returns how many atlas meshes and which texts it painted.
    fn paint(code: &str) -> (usize, Vec<String>) {
        let ctx = egui::Context::default();
        super::super::theme::setup(&ctx);
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), SIZE);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| draw(ui, rect, code));
        });
        let atlas_id = atlas(&ctx).map(|t| t.id());
        let (mut meshes, mut texts) = (0, Vec::new());
        for s in out.shapes {
            match s.shape {
                egui::Shape::Mesh(m) if Some(m.texture_id) == atlas_id => meshes += 1,
                egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                _ => {}
            }
        }
        (meshes, texts)
    }

    #[test]
    fn draw_paints_flag_or_chip() {
        assert_eq!(paint("IL"), (1, vec![]));
        assert_eq!(paint("zz"), (0, vec!["ZZ".to_string()]));
        assert_eq!(paint("LAN"), (0, vec![]));
    }
}
