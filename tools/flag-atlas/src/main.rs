//! Offline generator for `assets/flags/atlas.png` and `assets/flags/index.txt`.
//!
//! Run from this directory, pointing it at an extracted lipis/flag-icons source archive:
//!
//! ```text
//! cargo run --release -- <flag-icons source dir> [output dir]
//! ```
//!
//! The output dir defaults to `../../assets/flags`.
//!
//! Which flags: every code that flag-icons' `country.json` marks `"iso": true` (the ISO 3166-1
//! alpha-2 codes), plus `xk`. Sub-regions (`gb-eng`, `es-ct`, ...) and organisations (`eu`,
//! `un`, ...) are left out.
//!
//! Layout: each `flags/4x3/<code>.svg` is rasterised at 32x24 px into a 34x26 cell. The flag sits
//! 1 px in from every edge of its cell, and its outermost pixels are repeated into that 1 px
//! border, so linear filtering at a flag's edge never picks up a neighbour. Cells are packed
//! alphabetically, row-major, `COLS` to a row.
//!
//! `index.txt`: the column count on the first line, then one lowercase code per cell, in order.

use std::path::{Path, PathBuf};

use resvg::{tiny_skia, usvg};

/// Flag size inside a cell, in pixels. Must match `ui::flags::CELL` in the app.
const CELL: [u32; 2] = [32, 24];
/// Border around each flag, in pixels. Must match `ui::flags::PAD` in the app.
const PAD: u32 = 1;
const COLS: u32 = 16;
/// Not in ISO 3166-1, but the user-assigned code commonly used for Kosovo.
const EXTRA: &[&str] = &["xk"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(src) = args.first().map(PathBuf::from) else {
        eprintln!("usage: flag-atlas <flag-icons source dir> [output dir]");
        std::process::exit(2);
    };
    let out = args.get(1).map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/flags"));

    let codes = codes(&src);
    let (cw, ch) = (CELL[0] + 2 * PAD, CELL[1] + 2 * PAD);
    let rows = (codes.len() as u32).div_ceil(COLS);
    let mut atlas = tiny_skia::Pixmap::new(COLS * cw, rows * ch).expect("atlas size");
    let aw = atlas.width();
    for (i, code) in codes.iter().enumerate() {
        let flag = rasterise(&src.join("flags").join("4x3").join(format!("{code}.svg")));
        let (x0, y0) = ((i as u32 % COLS) * cw, (i as u32 / COLS) * ch);
        let from = flag.pixels();
        let to = atlas.pixels_mut();
        for y in 0..ch {
            let sy = y.saturating_sub(PAD).min(CELL[1] - 1);
            for x in 0..cw {
                let sx = x.saturating_sub(PAD).min(CELL[0] - 1);
                to[((y0 + y) * aw + x0 + x) as usize] = from[(sy * CELL[0] + sx) as usize];
            }
        }
    }

    std::fs::create_dir_all(&out).expect("create output dir");
    atlas.save_png(out.join("atlas.png")).expect("write atlas.png");
    let mut index = format!("{COLS}\n");
    for code in &codes {
        index.push_str(code);
        index.push('\n');
    }
    std::fs::write(out.join("index.txt"), index).expect("write index.txt");

    let skipped: Vec<String> = std::fs::read_dir(src.join("flags").join("4x3"))
        .expect("list flags/4x3")
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".svg").map(str::to_string))
        .filter(|stem| !codes.contains(stem))
        .collect();
    println!("{} flags, {COLS} x {rows} cells, {} x {} px -> {}", codes.len(), atlas.width(), atlas.height(), out.display());
    println!("left out: {}", skipped.join(" "));
}

/// The codes to pack, sorted and lowercase.
fn codes(src: &Path) -> Vec<String> {
    let json = std::fs::read_to_string(src.join("country.json")).expect("read country.json");
    let list: serde_json::Value = serde_json::from_str(&json).expect("parse country.json");
    let mut codes: Vec<String> = list
        .as_array()
        .expect("country.json is an array")
        .iter()
        .filter(|c| c["iso"].as_bool() == Some(true))
        .map(|c| c["code"].as_str().expect("entry without a code").to_ascii_lowercase())
        .chain(EXTRA.iter().map(|s| s.to_string()))
        .collect();
    codes.sort();
    codes.dedup();
    for c in &codes {
        assert!(c.len() == 2 && c.bytes().all(|b| b.is_ascii_lowercase()), "not an alpha-2 code: {c}");
    }
    codes
}

/// One flag at exactly `CELL` pixels, stretched from its view box.
fn rasterise(svg: &Path) -> tiny_skia::Pixmap {
    let data = std::fs::read(svg).unwrap_or_else(|e| panic!("{}: {e}", svg.display()));
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).unwrap_or_else(|e| panic!("{}: {e}", svg.display()));
    let size = tree.size();
    let mut px = tiny_skia::Pixmap::new(CELL[0], CELL[1]).expect("cell size");
    let scale = tiny_skia::Transform::from_scale(CELL[0] as f32 / size.width(), CELL[1] as f32 / size.height());
    resvg::render(&tree, scale, &mut px.as_mut());
    px
}
