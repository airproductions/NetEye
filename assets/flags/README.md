# Country flags

`atlas.png` and `index.txt` are generated files. Vantage embeds them at build time (`src/ui/flags.rs`) and draws each flag at 16×12 logical pixels.

## Source

- Project: lipis/flag-icons, release tag `v7.6.0`, the 4x3 SVGs (`flags/4x3/*.svg`).
- Archive: https://github.com/lipis/flag-icons/archive/refs/tags/v7.6.0.tar.gz
- Archive SHA-256: `1adb180ded51b201003bf5f88776306421e955c9ad7336ce62b6eda4e8eeb604`
- Licence: MIT, see `LICENSE` in this folder (copied unchanged from the archive).
- Downloaded: 2026-10-27.

The flags were rasterised and repacked; the artwork itself was not modified.

## What is in the atlas

- The 249 codes that flag-icons' `country.json` marks as ISO 3166-1 alpha-2, plus `xk` (Kosovo): 250 flags.
- Left out: sub-regions (`gb-eng`, `gb-nir`, `gb-sct`, `gb-wls`, `es-ct`, `es-ga`, `es-pv`, `sh-ac`, `sh-hl`, `sh-ta`), organisations (`arab`, `asean`, `cefta`, `eac`, `eu`, `pc`, `un`), the reserved `cp`, `dg` and `ic`, and the placeholder `xx`. Vantage draws a two-letter chip for any code without a flag.

## Layout

- Each flag is 32×24 px inside a 34×26 px cell. The 1 px border around a flag repeats its edge pixels, so filtering never blends in a neighbouring flag.
- Cells are in alphabetical order, row-major, 16 to a row: 16 × 16 cells, 544 × 416 px.
- `index.txt`: the column count on the first line, then one lowercase code per cell, in the same order.
- `atlas.png` SHA-256: `4698f1c90cceae5e3964ba5236fb1c7f64de389b9634bcd4e7937d8b49b29287`

## Regenerating

The generator is `tools/flag-atlas`, a standalone Cargo project (resvg 0.49.0, pinned by its `Cargo.lock`). Extract the archive above, then:

```text
cd tools/flag-atlas
cargo run --release -- <path to the extracted flag-icons-7.6.0 folder>
```

It writes `atlas.png` and `index.txt` into this folder. The cell size and padding are shared with `src/ui/flags.rs` (`CELL`, `PAD`); change both together.
