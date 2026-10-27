# Vantage "Mesh Glass" UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle every Vantage page in the dark "mesh glass" look (plus a matching light mode), with the Manrope font, an icon rail, icon-first controls, a bottom usage strip on Traffic and light motion throughout.

**Architecture:** A runtime `Palette` (dark and light) replaces the `c::*` color constants; every page reads colors through `c::name()` accessors. New small modules hold the backdrop (gradient and network mesh), glass card drawing and motion helpers. `mod.rs` swaps the top tab bar for a left icon rail and page header. Each page is then restyled on top of those primitives without changing its data or behavior.

**Tech Stack:** Rust 2021, eframe/egui 0.31 (`ctx.animate_value_with_time`, `ctx.animate_bool_with_time`, `ViewportCommand::SetTheme`, `ctx.system_theme()`), serde, Manrope TTFs (SIL OFL).

**Spec:** `docs/superpowers/specs/2026-10-27-ui-redesign-design.md`. Mockups: `.superpowers/brainstorm/1917-1793059582/content/traffic-icons.html` (page) and `traffic-strip.html` (strip, but placed at the **bottom**).

## Global Constraints

- Don't edit `src/wfp.rs`, `src/system.rs` or `src/net.rs`. Only add the `appearance` setting to `src/store.rs`. Leave `src/monitor.rs` untouched. In `src/ui/firewall.rs`, change drawing code only, never block/allow logic or `Action`s.
- This is not a git repo. Task 1 snapshots `src/` to `backup/src-2026-10-27/`. In place of "commit", each checkpoint is: `cargo build` with no new warnings, then `cargo test` passing.
- The window keeps its 1100×680 minimum and its native title bar, colored to match the theme.
- Animations last 150–250ms with ease-out. Repaint only while an animation is running, with no continuous repaint loop. The mesh doesn't move.
- Every icon-only control needs a hover tooltip.
- Keep the shared `theme.rs` helpers' signatures the same. Adding new helpers is fine.
- Ask the user before downloading anything (the Manrope fonts).

## Review Focus

1. **Settings saved by the current version** have no `appearance` key. They must load as `Appearance::System` with every other setting kept, not reset to defaults. Test in Task 9.
2. **Window at its 1100×680 minimum, with the limited-mode banner showing.** The rail, cards and bottom strip must not overlap or clip. Below a width threshold, the strip drops the apps/connections section rather than squeezing it. Test in Task 6, manual check in Task 10.
3. **Extreme values.** 0 B, TB-scale totals, a cap that is exceeded (over 100%) and no cap at all must not draw the gauge outside its arc or overflow the number slots. Test in Task 6.
4. **Limited (non-admin) mode.** With no per-app data, the app list, strip and header still render sensibly: zeros, not blanks or panics. Manual check in Task 6.
5. **Hidden to tray and then reopened.** Repaints stop while the window is hidden. On reopen, nothing is stuck mid-animation and the page shows current data. Manual check in Task 10.

---

### Task 1: Snapshot and runtime palette

**Files:**
- Create: `src/ui/palette.rs`
- Modify: `src/ui/theme.rs` (module `c`, `setup`), `src/ui/mod.rs` (`mod palette;`), every `src/ui/*.rs` that uses `c::UPPER` (mechanical rename)

**Interfaces:**
- Produces:
  - `pub struct Palette` (Copy), with fields `bg, panel, raised, row_alt, hover, selected, border, text, muted, faint, accent, accent2, down, up, good, bad, warn, orange, bg_glow, bg_mid, bg_deep, glass_top, glass_bottom, glass_border, shadow, mesh_line, mesh_node, glow: Color32`.
  - `pub fn dark() -> Palette` and `pub fn light() -> Palette`.
  - `pub fn lerp(a: &Palette, b: &Palette, t: f32) -> Palette`, which applies `theme::lerp_color` to each field.
  - `pub fn current() -> Palette` and `pub fn set(p: Palette)`, backed by `static RwLock<Palette>` and initialised to `dark()`.
  - In `theme::c`: one lowercase accessor per field, e.g. `pub fn text() -> Color32 { palette::current().text }`. The `pub const` items are removed.

- [ ] **Step 1:** Copy `src/` to `backup/src-2026-10-27/`, then run `cargo build`. Expected: it builds. This is the baseline.
- [ ] **Step 2: Write the failing tests** in `palette.rs`'s `#[cfg(test)] mod tests`:
  - `lerp_endpoints`: `lerp(&dark(), &light(), 0.0).text == dark().text` and the same at `1.0` with `light().text`.
  - `set_then_current`: `set(light()); assert_eq!(current().bg, light().bg); set(dark());`
  - `text_contrasts_background`: for both palettes, the relative-luminance difference between `text` and `bg` is > 0.5.
- [ ] **Step 3:** Run `cargo test palette`. Expected: compile FAIL (module missing).
- [ ] **Step 4: Implement the palette.** Values, as unmultiplied RGBA:
  - **dark:**
    - Backgrounds: `bg #0a1426`, `bg_glow #143a72`, `bg_mid #0a1426`, `bg_deep #05080f`.
    - Text: `text #e6eefc`, `muted #8a97b6`, `faint #5c6884`.
    - Accents and series: `accent #2f6bff`, `accent2 #4fd1ff`, `down #4fd1ff`, `up #a78bfa`.
    - Status: `good #5ee0a8`, `bad #f87171`, `warn #fbbf24`, `orange #fb923c`.
    - Glass: `glass_top (90,140,255,28)`, `glass_bottom (255,255,255,6)`, `glass_border (140,180,255,36)`, `shadow (0,0,0,90)`, `panel = glass_top`.
    - Surfaces: `raised (255,255,255,13)`, `row_alt (255,255,255,6)`, `hover (120,170,255,26)`, `selected (60,120,255,56)`, `border (140,180,255,36)`.
    - Mesh and glow: `mesh_line (61,139,255,70)`, `mesh_node (127,196,255,140)`, `glow (60,120,255,90)`.
  - **light:**
    - Backgrounds: `bg #eef2f7`, `bg_glow #c7d5ec`, `bg_mid #e9eef5`, `bg_deep #f3f5f9`.
    - Text: `text #17243d`, `muted #6b7890`, `faint #9aa5b8`.
    - Accents and series: `accent #2f6fd6`, `accent2 #3c7be0`, `down #2f6fd6`, `up #7c5cd6`.
    - Status: `good #1f8a5c`, `bad #d64545`, `warn #b7791f`, `orange #c05621`.
    - Glass: `glass_top (255,255,255,173)`, `glass_bottom (255,255,255,150)`, `glass_border (255,255,255,242)`, `shadow (40,70,120,20)`, `panel = glass_top`.
    - Surfaces: `raised #eaeff6`, `row_alt (23,36,61,6)`, `hover (36,64,122,15)`, `selected #e7eefb`, `border (23,36,61,20)`.
    - Mesh and glow: `mesh_line (90,120,168,40)`, `mesh_node (90,120,168,77)`, `glow (47,111,214,40)`.
- [ ] **Step 5: Rename the color constants.** Replace each `c::UPPER` with `c::lower()` across `src/ui/*.rs`, e.g. with `sed -E 's/c::(BG|PANEL|RAISED|ROW_ALT|HOVER|SELECTED|BORDER|TEXT|MUTED|FAINT|ACCENT|DOWN|UP|GOOD|BAD|WARN|ORANGE)\b/c::\L\1()/g'`. Leave `src/monitor.rs` and `src/net.rs` alone: their `c::` is unrelated. `theme::setup` reads the palette for the egui `Visuals` and gains `pub fn apply_visuals(ctx: &egui::Context, p: &Palette)` (light mode needs `Visuals::light()` as its base).
- [ ] **Step 6: Checkpoint.** Run `cargo build && cargo test`. Expected: PASS, and the app runs with the new dark colors (still flat).

### Task 2: Bundled Manrope font

**Files:**
- Create: `assets/fonts/Manrope-Regular.ttf`, `Manrope-SemiBold.ttf`, `Manrope-Bold.ttf`, `assets/fonts/OFL.txt`
- Modify: `src/ui/theme.rs` (`setup`, add `bold`)

**Interfaces:**
- Produces: `pub fn bold(size: f32) -> FontId` (family `"bold"`). `regular` and `semibold` keep their signatures.

- [ ] **Step 1:** Ask the user to approve downloading the static Manrope TTFs and the OFL licence from the official repo (`github.com/sharanda/manrope`, `fonts/ttf/`). Don't use the variable font, because egui renders only its default instance. Download into a new, empty `assets/fonts/`.
- [ ] **Step 2: Write the failing test** `fonts_bundled` in theme tests: build the `FontDefinitions` through a new `fn font_definitions() -> FontDefinitions` (split out of `setup`), then assert `font_data` contains `"manrope"`, `"manrope-sb"` and `"manrope-b"`, and that the first entry of `families[Proportional]` is `"manrope"`.
- [ ] **Step 3:** Run `cargo test fonts_bundled`. Expected: FAIL.
- [ ] **Step 4: Implement.** Load the fonts with `include_bytes!("../../assets/fonts/…")`. Families:
  - Proportional is `[manrope, segoe-symbol fallback]`.
  - `"semibold"` is `[manrope-sb, …]`.
  - `"bold"` is `[manrope-b, …]`.
  - Keep loading `seguisym.ttf` from disk as the symbol fallback. Drop `segoeui.ttf` and `seguisb.ttf`.
- [ ] **Step 5: Checkpoint.** Run `cargo build && cargo test`, then open the app and check that the text renders in Manrope.

### Task 3: Backdrop, mesh and glass primitives

**Files:**
- Create: `src/ui/backdrop.rs`
- Modify: `src/ui/theme.rs` (`card`, add `glass`, `glass_frame`, `glow`), `src/ui/mod.rs` (paint the backdrop behind the central panel)

**Interfaces:**
- Produces:
  - `pub fn paint_backdrop(p: &Painter, rect: Rect)` draws the radial gradient (`bg_glow` at 65%/-10% of the rect, through `bg_mid` to `bg_deep`) as a vertex-colored `Mesh` fan. On top it draws the network mesh in `mesh_line`/`mesh_node`, concentrated in the lower 60% of the rect.
  - `pub fn mesh_graph(seed: u64, n: usize) -> (Vec<Pos2>, Vec<(usize, usize)>)` returns nodes in unit space (0..1) and edges joining each node to its 2–3 nearest neighbours. It is deterministic and cached per seed with `OnceLock`.
  - `theme::glass(p: &Painter, r: Rect, radius: u8)` draws, in order: a shadow (`egui::Shadow { offset: [0, 10], blur: 28, color: shadow }`), a diagonal gradient fill (`glass_top` to `glass_bottom`) and a 1px `glass_border` stroke.
  - `theme::glass_frame() -> egui::Frame` is the same look for Frame-based layouts: 14px radius, 14px margin.
  - `theme::glow(p: &Painter, r: Rect, radius: u8, t: f32)` draws a soft outer glow in `glow`, scaled by `t`.
  - `theme::card` keeps its signature but draws as glass.

- [ ] **Step 1: Write the failing tests** in `backdrop.rs`:
  - `mesh_is_deterministic`: two calls with the same seed return equal results.
  - `mesh_nodes_in_unit_square`.
  - `mesh_edges_valid`: there are no self-edges or duplicates (as unordered pairs), every index is less than `n`, and every node has at least one edge.
- [ ] **Step 2:** Run `cargo test backdrop`. Expected: FAIL.
- [ ] **Step 3: Implement.** Use a small xorshift PRNG (no new dependency). Use `n = 28`. Nodes get a vertical bias toward `y ∈ [0.4, 1.0]`. The backdrop is painted on `ctx.layer_painter(LayerId::background())` covering the full central rect. The `CentralPanel` frame fill becomes `Color32::TRANSPARENT`.
- [ ] **Step 4: Checkpoint.** Run `cargo build && cargo test`. Settings cards now render as glass over the mesh.

### Task 4: Motion helpers and animated widgets

**Files:**
- Create: `src/ui/motion.rs`
- Modify: `src/ui/theme.rs` (`icon_button`, `pill`, `primary_button`, `switch`, `segmented`, `search_box`, `status_pill`)

**Interfaces:**
- Produces:
  - `pub const FAST: f32 = 0.15;` and `pub const NORMAL: f32 = 0.25;`
  - `pub fn ease_out(t: f32) -> f32` is cubic: `1 - (1-t)^3`, clamped to 0..1.
  - `pub fn hover(ui: &Ui, id: Id, hovered: bool) -> f32` wraps `animate_bool_with_time(id, hovered, FAST)`.
  - `pub fn value(ctx: &Context, id: Id, target: f32) -> f32` wraps `animate_value_with_time(id, target, NORMAL)`.
  - `pub fn number(ctx: &Context, id: Id, target: f64) -> f64` eases a displayed number. It snaps immediately when the jump is more than 10× or the target is 0.
  - `pub fn num_slot(ui: &Ui, font: &FontId, widest: &str) -> f32` returns the width of `widest`, used for right-aligned fixed-width number slots.

- [ ] **Step 1: Write the failing tests:**
  - `ease_out_bounds`: `ease_out(0.0)==0.0`, `ease_out(1.0)==1.0` and `ease_out(2.0)==1.0`.
  - `ease_out_monotonic` over 0..=100 steps.
  - `ease_out_front_loaded`: `ease_out(0.5) > 0.5`.
- [ ] **Step 2:** Run `cargo test motion`. Expected: FAIL.
- [ ] **Step 3: Implement the helpers, then animate the widgets:**
  - Hover fills fade with `hover()` and `lerp_color`.
  - `segmented`'s selected pill slides: its x position comes from `value(id, selected as f32)`. The active segment uses `accent` plus `glow`.
  - `switch` keeps its `animate_bool` but uses `FAST`.
  - `segmented` gains an optional label mode, `pub fn segmented_text(ui, items: &[&str], selected: &mut usize, tooltip: Option<&[&str]>)`, for short range codes.
- [ ] **Step 4: Checkpoint.** Run `cargo build && cargo test`. Hover fades and the segmented pill slides.

### Task 5: Shell: icon rail, page header, banner and page transition

**Files:**
- Modify: `src/ui/mod.rs` (`nav` is replaced by `rail` and `header`, plus the update layout), `src/ui/theme.rs` (new glyphs), `src/ui/settings.rs` (remove its own "Settings" title)

**Interfaces:**
- Consumes: `motion::*`, `theme::glass`, `theme::glow`, `backdrop::paint_backdrop`.
- Produces:
  - New `Glyph` variants `Bell, Radar, Wifi, WifiOff, Clock, Grid`, drawn on the existing 20×20 grid. Activity uses `Bell`, Scanner uses `Radar`, and Apps uses `Grid`.
  - `fn rail_slots(rect: Rect, n_top: usize) -> (Rect, Vec<Rect>, Rect)` returns the logo, the page slots and the settings slot. Slots are 40×40 with 10px gaps, starting 64px below the top. Settings is anchored 14px from the bottom.
  - `fn unread_label(n: usize) -> String` returns `"No new alerts"`, `"1 new alert"` or `"{n} new alerts"`.
  - `fn page_title(p: Page) -> &'static str` returns "Traffic", "Firewall", "Activity", "Scanner", "Hardware" or "Settings".

- [ ] **Step 1: Write the failing tests:**
  - `rail_slots_fit_min_height`: at a height of 680 − 32 (the title bar allowance), the settings slot doesn't overlap the last page slot.
  - `rail_slots_count`: there are 5 page slots.
  - `unread_label_plural`: 0, 1 and 7 give the three strings above.
- [ ] **Step 2:** Run `cargo test ui::`. Expected: FAIL.
- [ ] **Step 3: Implement the layout:**
  - `SidePanel::left("rail").exact_width(64)` with a transparent frame.
  - The rail draws its own faint glass strip.
  - The active highlight's y position is `motion::value(id, active_index)`, so the glow slides between icons.
  - The red dot shows when unread > 0, and the tooltip uses `unread_label`.
  - Every icon gets an `on_hover_text`.
- [ ] **Step 4: Implement the header.** It is a `TopBottomPanel::top("header").exact_height(56)` with a transparent frame:
  - `page_title` on the left, in `bold(20.0)`.
  - Three 28px status chips on the right, each with a tooltip:
    - online, offline or checking: `Wifi`/`WifiOff` in `good`/`bad`/`muted`;
    - firewall on or off: `Shield` in `good`/`faint`;
    - admin or limited: `Lock` in `accent`/`warn`.
- [ ] **Step 5: Implement the banner.** The limited-mode banner becomes a slim amber glass bar under the header: fill `warn` at alpha 28 and border `warn` at alpha 60. It keeps its text and button.
- [ ] **Step 6: Implement the page transition.** On a page change, store `page_changed: Instant`. For the first 0.25s, the central content uses `ui.set_opacity(ease_out(t))` and is offset by `8.0 * (1 - ease_out(t))` px upward-in. Call `ctx.request_repaint()` only while `t < 1`.
- [ ] **Step 7: Implement the toast.** `draw_toast` slides in from 16px right and below over `NORMAL` (offset `16 * (1 - ease_out(age / 0.25))`) and keeps the existing fade-out. Its frame becomes `glass_frame()` with an opaque `bg` underlay so the text stays readable.
- [ ] **Step 8: Checkpoint.** Run `cargo build && cargo test`. Click through all pages: each is reachable, tooltips show and the transition plays once.

### Task 6: Traffic page and bottom usage strip

**Files:**
- Modify: `src/ui/traffic.rs`

**Interfaces:**
- Consumes: `theme::glass`, `motion::number`, `motion::num_slot`, `theme::segmented`, `theme::segmented_text`, `Glyph::{Grid, Clock, Globe, Wifi}`.
- Produces (module-private, tested):
  - `const RANGE_CODES: [&str; 5] = ["5m", "3h", "24h", "7d", "30d"]`. The existing `RANGES` stay as the tooltips.
  - `struct Gauge { down: f32, up: f32, level: Level }` with `enum Level { Normal, Warn, Over }`.
  - `fn gauge(today: [u64; 2], cap_bytes: u64) -> Gauge`. With no cap (`cap_bytes == 0`), the arc is full and split by the download/upload ratio (`down + up == 1.0`, or both 0.0 when today is 0). With a cap, the arc fills against it: `down + up == min(total/cap, 1)`, with Warn above 0.8 and Over at 1.0 or more.
  - `fn strip_shows_counts(width: f32) -> bool` is `width >= 1180.0`.

- [ ] **Step 1: Write the failing tests:**
  - `gauge_no_cap_splits_ratio`: `gauge([300, 100], 0)` gives `down≈0.75`, `up≈0.25` and `Normal`.
  - `gauge_no_cap_zero`: `gauge([0, 0], 0)` gives `0, 0` and `Normal`.
  - `gauge_cap_partial`: `gauge([400, 100], 1000)` gives `down≈0.4`, `up≈0.1` and `Normal`.
  - `gauge_cap_warn`: a total of 850 against a cap of 1000 gives `Warn`.
  - `gauge_cap_exceeded_clamps`: `gauge([3_000, 1_000], 1_000)` has `down + up == 1.0` and `Over`.
  - `gauge_huge_values`: `gauge([u64::MAX / 2, u64::MAX / 2], 0)` doesn't panic and gives fractions in 0..=1.
  - `strip_drops_counts_when_narrow`: `!strip_shows_counts(1036.0)` (the 1100 minimum minus the 64 rail) and `strip_shows_counts(1300.0)`.
- [ ] **Step 2:** Run `cargo test traffic`. Expected: FAIL.
- [ ] **Step 3: Implement the layout.** Use a 16px outer gutter and 10px gaps:
  - the app list as a glass card on the left (width `min(400, 34%)`);
  - the chart as a glass card on the right;
  - the strip as a glass card along the bottom, 96px high.
  - The app list header is icon-only: `Grid` plus `Search`, with tooltips. Rows keep the real app icons, and the selected row uses `selected`.
  - The chart header shows `segmented` with Live graph/Usage/Hosts (icons only) on the left. On the right are a `Clock` glyph and `segmented_text(RANGE_CODES)` with `RANGES` as tooltips.
- [ ] **Step 4: Implement the strip.** Order:
  - Download: the range total in `bold(17)` with the rate below, then a `down` arrow.
  - The gauge, as a 120×70 semicircle. The `down` arc is followed by the `up` arc. In the capped Warn or Over state, the whole fill uses `warn` or `bad`. Today's total sits in the centre.
  - Upload: an `up` arrow, then the total with the rate below.
  - A divider.
  - Two rows, Globe/"Internet" and Wifi/"Local network" (both as tooltips): each has its total and a bar split into `down` (gradient `accent` to `accent2` in dark) and `up`.
  - A divider, then `apps · connections` when `strip_shows_counts`.
  - Online status moves to the header (Task 5), so drop it here.
  - The numbers use `motion::number` in `num_slot` widths, sized from `"999.9 MB/s"` and `"999.99 GB"`.
- [ ] **Step 5: Implement the chart glide.** When a new live sample arrives, draw the series shifted right by one sample width × `(1 - ease_out(t))` over `NORMAL`, clipped to the chart rect. Use the existing `theme::area`, colored `down`/`up`.
- [ ] **Step 6: Checkpoint.** Run `cargo build && cargo test`. Then manually check Review Focus 2 and 4: run without admin at 1100×680 and confirm nothing clips and zeros render.

### Task 7: Activity, Scanner and Hardware pages

**Files:**
- Modify: `src/ui/log.rs`, `src/ui/scanner.rs`, `src/ui/hardware.rs`

**Interfaces:**
- Consumes: `theme::glass`, `glass_frame`, `motion::hover`, `segmented_text`, the new glyphs.

- [ ] **Step 1: Restyle the toolbars.** Each page's full-width `PANEL` toolbar (`log.rs:37`, `scanner.rs:44`, `hardware.rs:41`) becomes a glass card inside the 16px gutter. Toolbar text labels become icons with tooltips where an icon is clear:
  - Log filter and period: keep the dropdowns, but prefix them with `Bell`/`Clock` glyphs.
  - Scanner's "Scan" button keeps its text, since it's the primary action, and gains the `Radar` glyph.
  - Search stays as it is.
- [ ] **Step 2: Restyle the lists and tables.** They sit in glass cards. Row hover uses `motion::hover`, and alternate rows use `row_alt`. Headers use `muted` `semibold(12)`. Hardware's chart frames (`c::bg()` label knockouts at `theme.rs:505-508`) switch to drawing the label over a `glass_top` pill, because there is no longer a solid background behind them.
- [ ] **Step 3: Checkpoint.** Run `cargo build && cargo test`. Visually check each page in dark, with no leftover opaque `PANEL` bands.

### Task 8: Firewall page (drawing only)

**Files:**
- Modify: `src/ui/firewall.rs`

- [ ] **Step 1:** Read `firewall.rs` in full and list every line that pushes an `Action` or mutates `s`. Those lines must be byte-identical after this task.
- [ ] **Step 2: Restyle the page.** Do the same as in Task 7: a glass toolbar (`firewall.rs:62` header band), the table in a glass card, hover fades, icon-first toolbar controls with tooltips. `status_pill` gets a light-mode-safe fill: `good`/`bad` at alpha 36 instead of the hard-coded dark RGB in `theme.rs`.
- [ ] **Step 3: Checkpoint.** Run `cargo build && cargo test`. Diff `firewall.rs` against `backup/src-2026-10-27/ui/firewall.rs` and confirm the Step 1 lines are unchanged. Block and unblock one test app manually and confirm it still works.

### Task 9: Settings, Appearance setting, light mode and title bar

**Files:**
- Modify: `src/store.rs` (`Settings`), `src/ui/settings.rs`, `src/ui/mod.rs`, `src/ui/theme.rs`

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)] pub enum Appearance { #[default] System, Dark, Light }`, stored as the field `appearance` in `Settings`. `Settings` is already `#[serde(default)]`.
  - `pub fn resolve(a: Appearance, system: Option<egui::Theme>) -> egui::Theme`. `System` falls back to Dark when the system theme is `None`.

- [ ] **Step 1: Write the failing tests:**
  - `old_settings_json_loads`: deserialize `{"notifications":false,"daily_cap_mb":500}` into `Settings` and assert `appearance == System`, `notifications == false` and `daily_cap_mb == 500` (Review Focus 1).
  - `resolve_cases`: cover all three variants × `Some(Light)`, `Some(Dark)` and `None`.
- [ ] **Step 2:** Run `cargo test settings`. Expected: FAIL.
- [ ] **Step 3: Implement the enum and `resolve`.** Then, each frame in `mod.rs`:
  - Compute the target theme.
  - When it changes, record `theme_changed: Instant`, send `ViewportCommand::SetTheme(SystemTheme::Dark/Light)` (this sets the native title bar color) and re-apply the visuals.
  - For 0.25s, `palette::set(lerp(from, to, ease_out(t)))`, repainting only during the fade.
  - Apply the target theme once at startup, with no fade.
- [ ] **Step 4: Restyle Settings.** Remove its own title, since the header shows it. Its cards are glass. Add an "Appearance" card at the top with `segmented_text(["System", "Dark", "Light"])`, which marks the store dirty on change.
- [ ] **Step 5: Checkpoint.** Run `cargo build && cargo test`. Switch through all three options. Every page must be readable in light, with no dark-only colors left. Find leftovers with `grep -n "Color32::from_rgb" src/ui/*.rs` and move any page-level hard-coded colors onto the palette.

### Task 10: Final verification

- [ ] **Step 1:** Run `cargo build --release && cargo test`. Expected: both succeed, with no new warnings compared to the baseline.
- [ ] **Step 2: Check CPU (Review Focus 5).** With the window open and idle on Traffic, CPU in Task Manager should be about the same as the baseline build (it repaints about 1/s on monitor ticks). Hide the window to the tray and confirm the CPU drops to the background-monitor level. Reopen it and confirm nothing is mid-animation and the data is current.
- [ ] **Step 3: Check the minimum size (Review Focus 2).** At 1100×680, in limited mode, in both themes, every page has no clipping or overlap.
- [ ] **Step 4:** Walk every page in dark and light, and hover every icon-only control to confirm each has a tooltip.
