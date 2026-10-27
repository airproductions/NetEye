# Vantage "Classic Graphite" Rebuild: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore Vantage's pre-redesign layout and flat graphite look. Keep the redesign's non-layout improvements and every feature from `2da002e`, and add sortable lists plus an app details popup.

**Architecture:**
- The runtime `Palette` stays and gets flat graphite values. The backdrop and glass drawing are removed, and the old top tab bar comes back in `mod.rs`.
- A new `src/ui/sort.rs` owns the pure sort comparators and the clickable header widget. Each list's sort state persists in `Settings.sorts`.
- A new `src/ui/app_popup.rs` owns the popup. Pages open it by pushing a new `Action::OpenApp`.

**Tech Stack:**
- Rust, eframe/egui 0.31 (vendored eframe patch), serde.
- New dependency: `sha2 = "0.10"`, for the VirusTotal hash.

**Spec:** `docs/superpowers/specs/2026-10-27-classic-graphite-design.md`. Layout reference: `git show 2373ff6:src/ui/<file>.rs`. Mockup: `.superpowers/brainstorm/1917-1793059582/content/classic-layout.html`, option A.

## Global Constraints

- **Branch.** Work on a branch `classic-graphite` cut from `glasswire-feature-parity` (`2da002e`). Don't merge it into `main`.
- **Keep every `2da002e` feature working:**
  - firewall mode dropdown and profiles;
  - Ask-to-Connect bar and `Action::Answer`;
  - security monitors and their settings;
  - Activity filters and sort;
  - data plan;
  - device rename;
  - run at startup;
  - speed units;
  - snooze.
- **Off-limits files.** Don't change logic in `wfp.rs`, `system.rs`, `net.rs` or `monitor.rs`. In `firewall.rs`, lines that push an `Action` or mutate `s` stay byte-identical, apart from the new popup click described in Task 7.
- **Settings compatibility.** New settings fields use `#[serde(default)]`, and settings files from the current version load unchanged.
- **Window.** Minimum 1100×680, with the native title bar themed by the Appearance setting.
- **Repaints.** Repaint only while an animation runs: no continuous repaint, and no per-tick easing (keep `motion::EaseGate` and `motion::gated` behaviour).
- **No screenshots or automation.** Don't use window automation, screenshot scripts or PowerShell `Add-Type`, because the user's antivirus flags them. Verify with `cargo test` and `cargo build`, and leave visual checks to the user.
- **Gate per task:** `cargo build` shows only the 3 pre-existing warnings (`local`/`total`, `alias`/`speed`, `Refresh`/`Check`), and `cargo test` passes. Commit at the end of each task, with every message ending in `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Text.** All UI copy is our own wording. Don't copy GlassWire's text, icons or look.

## Review Focus

1. **Sorting an empty or one-row list, or a list where every value is "—".** Nothing panics, and the order is stable, with names as the tiebreak. Test in Task 3.
2. **Sorted Firewall groups.** Sorting must not move apps between Active and Inactive, and Active stays above Inactive. Test in Task 5.
3. **The popup for an app with no path, version or publisher (system processes).** The popup shows "—" or "Unknown", and VirusTotal is disabled with a reason. Nothing panics. Test in Task 7.
4. **Settings saved before this change, with no `sorts` key.** They load with today's default orders, and every other setting is kept. Test in Task 3.
5. **The popup open near the window's right or bottom edge, or at 1100×680.** It stays fully inside the window. Test in Task 7 (pure placement helper).

---

### Task 1: Branch, graphite palette, flat surfaces

**Files:**
- Modify: `src/ui/palette.rs`, `src/ui/theme.rs` (`card`, glass/glow helpers, `chart_frame`, `apply_visuals`), `src/ui/mod.rs`, `Cargo.toml`.
- Delete: `src/ui/backdrop.rs`.

**Interfaces:**
- Produces:
  - `palette::dark()` and `palette::light()` return the graphite values from the spec's tables.
  - The glass and mesh fields go away from `Palette` and `c::*`, if no caller needs them afterwards.
  - `theme::card` keeps its signature but goes back to the `2373ff6` flat look: `panel` fill, 1px `border`, radius 10, margin 16.
  - `theme::glow` becomes a no-op or is removed, with its callers updated.

- [ ] **Step 1:** On branch `classic-graphite` (already created from `glasswire-feature-parity`), add `sha2 = "0.10"` to `Cargo.toml`.
- [ ] **Step 2: Write the failing tests** in `palette.rs`:
  - `dark_is_classic_graphite`: `dark().bg == rgb(0x14161b)`, `.panel == rgb(0x1a1d23)`, `.accent == rgb(0x5b8dff)`, `.down == rgb(0x2dd4bf)`.
  - `light_is_graphite_light`: `light().bg == rgb(0xf3f4f7)`, `.panel == rgb(0xffffff)`, `.accent == rgb(0x3b6fe0)`.
  - Keep `text_contrasts_background`.
- [ ] **Step 3:** Run `cargo test palette`. Expected: FAIL.
- [ ] **Step 4: Implement.**
  - Set every palette value from the spec's tables. Light roles the table doesn't list: `orange #c05621`, `accent2 = accent`.
  - Delete `backdrop.rs` and its call in `mod.rs`.
  - Set the `CentralPanel` fill to `c::bg()`.
  - `card` and `chart_frame` go back to the `2373ff6` versions, keeping the `gaps()` gridline helper and its tests.
- [ ] **Step 5:** Run `cargo build && cargo test`. Expected: PASS, with 3 warnings. Commit "Classic graphite palette and flat surfaces".

### Task 2: Classic top bar shell

**Files:**
- Modify: `src/ui/mod.rs`.

**Interfaces:**
- Consumes: Task 1's palette.
- Produces:
  - `fn tab_rects(full: Rect, n: usize) -> Vec<Rect>`: tabs 132×50, 8px gaps, centred, starting no further left than `full.left() + 190`, vertically centred.
  - The `rail_slots` function and the `header`/`status_chip` code are removed. `unread_label` and `page_title` are kept (used for tooltips).

- [ ] **Step 1: Write the failing tests:**
  - `tab_rects_centered`: for `Rect` 1440×78, there are 5 rects, `rects[0].left() == (1440 - (5*132 + 4*8)) / 2` and the gaps are 8.
  - `tab_rects_clamp_left`: for width 1100, `rects[0].left() >= 190`.
  - Remove the `rail_slots_*` tests.
- [ ] **Step 2:** Run `cargo test ui::tests`. Expected: FAIL.
- [ ] **Step 3: Implement the top bar.** Replace the `SidePanel` rail and the header `TopBottomPanel` with the `2373ff6` `TopBottomPanel::top("nav").exact_height(78.0)` and its `nav()`:
  - brand, with FULL MONITORING or LIMITED MODE in `good` or `warn`;
  - two-line tab labels;
  - the unread badge on Activity;
  - the gear.
  The active highlight's x position glides with `motion::value` between the tab rects. Hover fades use `motion::hover`.
- [ ] **Step 4: Restyle the banners.** The limited-mode banner goes back to the `2373ff6` amber bar (40px). The Ask-to-Connect bar keeps its content, made flat with an `accent`-tinted fill and a 1px border, edge to edge. Keep the page fade and the toast.
- [ ] **Step 5:** Run `cargo build && cargo test`, then commit "Restore the classic top tab bar".

### Task 3: Sorting core and persisted sort state

**Files:**
- Create: `src/ui/sort.rs`.
- Modify: `src/store.rs` (`Settings`), `src/ui/mod.rs` (`mod sort;`).

**Interfaces:**
- Produces, in `sort.rs`:
  - `#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)] pub struct SortSpec { pub col: u8, pub desc: bool }`
  - `impl SortSpec { pub fn click(&mut self, col: u8, default_desc: bool) }`. Clicking the same column flips `desc`. Clicking another column sets `col` and `desc = default_desc`.
  - `pub fn cmp_text(a: &str, b: &str) -> Ordering`: case-insensitive.
  - `pub fn cmp_ip(a: &str, b: &str) -> Ordering`: octet-numeric. Strings that can't be parsed sort after valid IPs.
  - `pub fn cmp_opt<T: Ord>(a: Option<T>, b: Option<T>, desc: bool) -> Ordering`: `None` always last, whatever the direction.
  - `pub fn apply<T>(rows: &mut [T], spec: SortSpec, key: impl Fn(&T, &T, u8) -> Ordering, name: impl Fn(&T) -> &str)`: a stable sort. It reverses for `desc`, then breaks ties by `cmp_text(name)` ascending.
  - `pub fn header(ui: &mut Ui, rect: Rect, label: &str, col: u8, spec: &mut SortSpec, default_desc: bool, align: Align2) -> bool`: draws the label in `muted`, or in `text` when active, with ▼/▲ when active and a hover fade. It returns true when clicked.
- Produces, in `store.rs`: `Settings.sorts: BTreeMap<String, SortSpec>`, `#[serde(default)]`, empty by default. Keys are `"traffic"`, `"firewall"`, `"scanner"` and `"hardware"`.
  - `pub fn sort_for(&self, list: &str, default: SortSpec) -> SortSpec` on `Settings`.
  - `pub fn set_sort(&mut self, list: &str, spec: SortSpec)` on `Settings`.

- [ ] **Step 1: Write the failing tests** in `sort.rs`:
  - `text_is_case_insensitive`: `cmp_text("apple","Banana") == Less`.
  - `ip_numeric`: `cmp_ip("192.168.1.9","192.168.1.102") == Less`, and `cmp_ip("garbage","10.0.0.1") == Greater`.
  - `none_last_both_directions`: `cmp_opt(None, Some(1), false) == Greater` and `cmp_opt(None, Some(1), true) == Greater`.
  - `click_flips_then_switches`: from `{col:0, desc:false}`, `click(0, true)` gives `desc == true`, then `click(2, true)` gives `{col:2, desc:true}`.
  - `apply_empty_and_single`: sorting `[]` and `[x]` doesn't panic and leaves them unchanged (Review Focus 1).
  - `apply_all_equal_ties_by_name`: three rows with equal keys come out in name order (Review Focus 1).
- [ ] **Step 2: Write the failing settings tests** in `store.rs`:
  - `old_settings_without_sorts_load`: deserialize `{"daily_cap_mb":500}`. Expect `sorts.is_empty()` and `daily_cap_mb == 500`, and `sort_for("firewall", d) == d` (Review Focus 4).
  - `sorts_round_trip`: `set_sort("scanner", {col:4, desc:false})`, serialize, deserialize, and get it back unchanged.
- [ ] **Step 3:** Run `cargo test sort` and `cargo test settings`. Expected: FAIL.
- [ ] **Step 4:** Implement the sort functions and settings methods.
- [ ] **Step 5:** Run `cargo build && cargo test`, then commit "Sort comparators, header widget and persisted sort state".

### Task 4: Traffic page (classic layout and sorting)

**Files:**
- Modify: `src/ui/traffic.rs`.

**Interfaces:**
- Consumes: `sort::{SortSpec, apply, header, cmp_text}` and `Settings::{sort_for, set_sort}`.
- Columns: 0 = App (default asc), 1 = ↓ (default desc), 2 = ↑ (default desc). The list default is `{col:1, desc:true}`, which is today's order.

- [ ] **Step 1: Write the failing test** `traffic_rows_sort`: given rows `[("b",10,1),("A",5,9),("c",10,3)]`:
  - `{col:1,desc:true}` gives `b, c, A` (the tie on 10 broken by name);
  - `{col:0,desc:false}` gives `A, b, c`.

  Use a pure `fn sort_rows(rows: &mut [(String, u64, u64)], spec: SortSpec)`.
- [ ] **Step 2:** Run `cargo test traffic`. Expected: FAIL.
- [ ] **Step 3: Implement the layout and sorting.**
  - Restore the `2373ff6` layout: the full-height left panel, `min(470, 40%)` wide, with a `panel` fill and a right border; the chart on the right; and a 92px full-width bottom strip with a `panel` fill and a top border.
  - The strip keeps the current contents. Online status goes back to its right end, as an `Online`/`Offline` dot plus `apps · connections`, as in `2373ff6`'s footer.
  - Remove the glass cards and gutters.
  - The list headers use `sort::header`. "All apps" and "All traffic" stay first, and the main rows and "Connected now" both use `sort_rows`, with connection count as the tiebreak in "Connected now".
  - A header click calls `set_sort("traffic", …)` and marks the store dirty.
- [ ] **Step 4:** Run `cargo build && cargo test`, then commit "Traffic: classic layout with sortable app list".

### Task 5: Firewall page (classic layout and sorting)

**Files:**
- Modify: `src/ui/firewall.rs`.

**Interfaces:**
- Consumes: Task 3.
- Columns: 0 Apps, 1 Incoming, 2 Outgoing, 3 Publisher, 4 Version, 5 Hosts, 6 ↓, 7 ↑, 8 Traffic. The default is today's order: `{col:0, desc:false}`.
  - Incoming and Outgoing: Blocked sorts before Allowed in ascending order.
  - Hosts: by count.
  - Publisher and Version: through `cmp_opt`, so missing values go last.

- [ ] **Step 1:** List every line that pushes an `Action` or mutates `s` (they stay byte-identical).
- [ ] **Step 2: Write the failing test** `firewall_sort_keeps_groups`, using a pure `fn sort_group(rows: &mut [FwRow], spec: SortSpec)` where `FwRow` is a small test-friendly struct of the sortable fields. Sort active `[x,y]` and inactive `[z]` separately by Publisher descending. Each group keeps only its own members, and a row with no publisher comes last (Review Focus 2).
- [ ] **Step 3:** Run `cargo test firewall`. Expected: FAIL.
- [ ] **Step 4: Implement.**
  - Restore the `2373ff6` flat toolbar and table, keeping `2da002e`'s mode dropdown, profiles and title glyphs and tooltips.
  - The header row uses `sort::header` for all nine columns.
  - Active and Inactive each sort with `sort_group`.
  - Persist under `"firewall"`.
- [ ] **Step 5:** Run `cargo build && cargo test`. Check that `git diff glasswire-feature-parity -- src/ui/firewall.rs` changes none of the Step 1 lines. Commit "Firewall: classic table with sortable columns".

### Task 6: Scanner, Hardware, Activity and Settings back to classic

**Files:**
- Modify: `src/ui/scanner.rs`, `src/ui/hardware.rs`, `src/ui/log.rs`, `src/ui/settings.rs`.

**Interfaces:**
- Consumes: Task 3.
- Scanner columns: 0 status (online first), 1 Type, 2 Name, 3 Description, 4 IP (`cmp_ip`), 5 MAC, 6 Last seen, 7 First seen. The default `{col:0, desc:false}` reproduces today's online-then-IP order.

- [ ] **Step 1: Write the failing test** `scanner_default_matches_today`. With devices `(online, "192.168.1.102")`, `(offline, "192.168.1.5")` and `(online, "192.168.1.9")`, the default spec gives `.9, .102, .5`. Use a pure `fn sort_devices(rows: &mut [DevRow], spec: SortSpec)`.
- [ ] **Step 2:** Run `cargo test scanner`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **Scanner:** restore the `2373ff6` flat toolbar and table, keeping `2da002e`'s rename and interval controls. Headers use `sort::header`, persisted under `"scanner"`.
  - **Hardware:** restore the flat `2373ff6` look, and move its existing sort onto `sort::header` and the `"hardware"` key, with the same default.
  - **Activity:** restore the flat look, keeping `2da002e`'s filters and sort as they are.
  - **Settings:** cards go flat via `theme::card`, and every `2da002e` setting stays.
- [ ] **Step 4:** Run `cargo build && cargo test`, then commit "Scanner, Hardware, Activity and Settings back to classic".

### Task 7: App details popup

**Files:**
- Create: `src/ui/app_popup.rs`.
- Modify: `src/ui/mod.rs` (`Action::OpenApp`, popup state and drawing), `src/ui/traffic.rs`, `src/ui/firewall.rs`, `src/ui/log.rs` (name and icon click targets).

**Interfaces:**
- Consumes: `Action::Block(String, Dir, bool)`, `store::{AppRecord, Event}`, `Icons`.
- Produces:
  - `Action::OpenApp(String /*app key*/, Pos2 /*anchor*/)`.
  - `pub struct AppPopup { key: String, anchor: Pos2, tab: Tab /*Info|Alerts*/ }`, held as `Option<AppPopup>` on `VantageApp`.
  - `pub fn place(anchor: Pos2, size: Vec2, screen: Rect) -> Rect`: put the popup to the right of and below the anchor, flip it when it would overflow, then clamp it inside `screen` with an 8px margin.
  - `pub fn alerts_for<'a>(events: &'a [Event], key: &str) -> Vec<&'a Event>`: matches `e.app.as_deref() == Some(key)`, newest first.
  - `pub fn vt_url(sha256_hex: &str) -> String`: returns `"https://www.virustotal.com/gui/file/{hex}"`.
  - `fn sha256_file(path: &Path) -> io::Result<String>`: lowercase hex, using `sha2`. Called from a spawned thread on click. The result opens the URL through `explorer.exe <url>` or `cmd /c start`.

- [ ] **Step 1: Write the failing tests:**
  - `place_stays_inside`: an anchor at `(1090, 670)`, size `(360, 300)` and screen `1100×680` give a rect inside the screen minus 8px, and the anchor at `(10, 10)` places to the right and below (Review Focus 5).
  - `alerts_for_filters_and_orders`: events for apps `a`, `b` and `a` at t = 1, 2, 3 give `a`'s events at t = 3 then t = 1.
  - `vt_url_format`: `vt_url("ab12")` returns `"https://www.virustotal.com/gui/file/ab12"`.
  - `sha256_known_value`: hashing a temp file holding `abc` gives `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.
  - `info_rows_for_bare_record`: a pure `fn info_rows(rec: Option<&AppRecord>) -> Vec<(&'static str, String)>`, given `None` or an empty record, gives Path `—`, Version `—` and Publisher `Unknown`, without panicking (Review Focus 3).
- [ ] **Step 2:** Run `cargo test app_popup`. Expected: FAIL.
- [ ] **Step 3: Implement the popup** as an `egui::Area` on the foreground layer, about 360px wide, placed with `place`, with the flat `panel` fill, a 1px `border` and a soft shadow.
  - **Header:** icon, name, and `Info` and `Alerts (n)` tabs.
  - **Info tab:** `info_rows`, plus first seen, last seen and first network use (via `theme::date_time`), totals, and block state.
  - **Alerts tab:** `alerts_for`, or "No alerts for this app" when empty.
  - **VirusTotal button:** disabled with the tooltip "No file path for this app" when there's no path.
  - **Block ▾ menu:** pushes the same `Action::Block` calls the table cells do. It has 4 entries, and the current state is checked.
  - **More ▾ menu:**
    - Open file location: `explorer.exe /select,<path>`.
    - Copy path: `ctx.copy_text`.
  - Closes on Esc or a click outside.
- [ ] **Step 4: Wire the click targets.** The app name and icon in the Traffic rows (which also select the row), the Firewall rows (separate from expand and the block cells) and the Activity entries that have an app become click targets, with a hand cursor and an underline on hover. They push `Action::OpenApp(key, pointer_pos)`.
- [ ] **Step 5:** Run `cargo build && cargo test && cargo build --release`, then commit "App details popup".

### Task 8: Final verification

- [ ] **Step 1:** Run `cargo test` (all pass) and `cargo build` (3 pre-existing warnings), then `cargo build --release`.
- [ ] **Step 2:** Diff against `glasswire-feature-parity` and confirm that every `2da002e` feature listed in Global Constraints still has its UI entry point.
- [ ] **Step 3:** Hand the user a checklist to verify by eye: each page in both themes, at 1100×680 and with limited mode, sorting on each list, and the popup from each of the three entry points.
