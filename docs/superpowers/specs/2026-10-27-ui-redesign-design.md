# Vantage UI redesign — "Mesh Glass"

Date: 2026-10-27 · Status: draft for approval (updated after the firewall session finished)

## Why

Vantage works, but the UI reads as basic and nothing moves. The goal is for it to feel like a polished, professional network monitor. References the user supplied: the Payno banking dashboard (soft glassy cards, airy spacing, big numbers, icon rail) and a dark-blue glowing network-mesh image. A GlassWire-style usage strip was given as the reference for the summary bar.

Mockups agreed in the brainstorm live in `.superpowers/brainstorm/1917-1793059582/content/` (`traffic-icons.html` for the page, `traffic-strip.html` for the strip; the strip goes at the **bottom**, not the top as drawn there).

## Goals

- A distinctive dark "mesh glass" look across every page, plus a matching light mode.
- Fewer words, more icons. Labels move into tooltips wherever an icon is clear.
- Smooth, light animation everywhere state changes. Nothing bouncy or flashy.
- A bundled, non-default font: **Manrope**.

## Non-goals

- No change to what any page does, what data it shows or how monitoring/firewall logic works.
- No new pages or features beyond the theme switch.
- No real backdrop blur (egui can't do it). Glass is faked.

## Anti-goals (would count as failure even if it works)

- Animations that feel slow, distracting, or make the app laggy.
- Noticeably higher idle CPU/GPU while Vantage sits open or in the tray.
- Icons so terse that a feature becomes hard to find (every icon-only control needs a tooltip).
- Breaking the firewall behaviour the other session just finished.

## Constraints

- Rust + eframe/egui 0.31, all UI hand-painted through `src/ui/theme.rs` helpers.
- **Build on the firewall session's finished code** (done 2026-10-27, checked: it builds cleanly). That session changed the backend (`wfp.rs`, `system.rs`, `monitor.rs`, `store.rs`) and one Settings description. The UI structure (`mod.rs` `Page`/`Action`, `theme.rs` helpers and palette) is unchanged. This redesign must not change `wfp.rs`, `system.rs` or `net.rs`, and it touches `monitor.rs`/`store.rs` only to add the appearance setting. In `firewall.rs` it changes only drawing code, never block/allow logic.
- Not a git repo: before starting, snapshot `src/` (a dated copy) so the pre-redesign state can be restored. Keep the app compiling after every step.
- Window min size stays 1100×680.

## Decided

### Look (dark — default)

- **Background:** radial gradient, bright navy glow top-right (`#143a72`) fading through `#0a1426` to near-black `#05080f`, with a faint network mesh (cyan-blue nodes + thin lines, low opacity) concentrated in the lower half.
- **Cards ("glass"):** translucent blue-tinted fill with a subtle top-left-to-bottom-right gradient, 1px light-blue border at low alpha, soft dark shadow, 14px radius.
- **Accent:** blue→cyan (`#2f6bff`→`#4fd1ff`). Download = cyan, upload = violet (`#a78bfa`). Good/bad/warn stay green/red/amber.
- **Active states glow:** selected nav icon and selected segment get a blue fill plus a soft outer glow.

### Look (light)

- Pale blue-grey gradient (`#c7d5ec` haze at top → `#f3f5f9`), white translucent cards with a white border and a soft blue-grey shadow, navy accent (`#24407a`/`#2f6fd6`), download blue, upload violet, a faint grey mesh. Matches `traffic-icons.html` bottom half.

### Window frame

- Keep the native Windows title bar (user's decision). Match it to the theme: dark title bar in dark mode, light in light mode (DWM immersive dark mode attribute).

### Theme switch

- Settings gets **Appearance: Dark / Light / Follow Windows** (default: Follow Windows if detectable, else Dark). Switching cross-fades colors over ~250ms.

### Font

- **Manrope** (SIL OFL), Regular + SemiBold + Bold, bundled into the binary via `include_bytes!` from `assets/fonts/`. Replaces Segoe UI as the proportional family; Segoe UI Symbol stays as a fallback for symbols. Numbers use tabular figures where Manrope offers them.

### Layout

- **Left icon rail (≈54px)** replaces the top tab bar: logo at top, then Traffic, Firewall, Activity (bell), Scanner (radar), Hardware (chip); Settings pinned at the bottom. Tooltips with the page names. Activity shows a red dot for unread important events (count in the tooltip).
- **Page header:** page title on the left; on the right three small status icons — online/offline, firewall on/off, admin/limited — each with a tooltip.
- **Limited-mode banner** stays, restyled as a slim amber glass bar under the header with the "Restart as administrator" button.
- **Traffic page:** app list card on the left, chart card on the right; view switch (Live graph / Usage / Hosts) as an icon-only segmented control; range as short codes `5m · 3h · 24h · 7d · 30d`; app rows show real app icons (existing `Icons`).
- **Usage strip at the bottom of Traffic** (replaces today's footer), one glass card: download total + rate · ↓ · semicircle gauge (today's total in the middle; arc split download/upload; fills against the daily cap if set) · ↑ · upload total + rate · divider · Internet and LAN rows (icon, total, long bar split download/upload) · divider · active apps · connections.
- **Other pages** (Activity, Scanner, Hardware, Settings, then Firewall) get the same treatment: glass cards, icon-first headers and toolbars, tooltips instead of labels where the icon is clear. Their structure and content stay as they are.

### Motion (all ~150–250ms, ease-out)

- Hover highlights fade in/out on every interactive element.
- Page switch: content cross-fades with a small upward slide (~8px).
- Nav-rail selection glow slides between icons.
- Segmented controls: the selected pill slides.
- Headline numbers (rates, totals) ease to new values instead of jumping.
- Charts: new samples glide in rather than redrawing in steps.
- Toasts slide in from the bottom-right and fade out (existing fade kept).
- Theme switch cross-fades.

### Calls made during review (Claude's, not the user's — check these)

- **Strip numbers keep today's meaning** (no behaviour change is a goal): the gauge shows today's machine-wide total against the cap; the download/upload totals and the Internet/LAN bars follow the selected range and app, as the footer does now.
- **Gauge without a cap** (the default): full arc split by the download/upload ratio. With a cap: fills against it, amber from 80%, red at 100%, as now. No bell icon.
- **CPU budget:** no continuous repaint. Animations request repaints only while they run (≈250ms after a sample, hover or click). When hidden or in the tray, no animation repaints at all. The mesh is static.
- **Numbers:** egui can't turn on tabular digits, so animated numbers are right-aligned in fixed-width slots to avoid jitter.
- **Light mode:** the Appearance setting is exposed only once every page, Firewall included, uses the runtime palette, so no page ever shows dark-only colors on a light background.

## Left to the builder

- The exact shape of the runtime palette (all pages, Firewall included, move off the `c::*` constants).
- How animation state is stored (egui `animate_value_with_time` / `animate_bool` per id is expected).
- Icon drawing: extend the existing `Glyph` vector set (new glyphs needed: bell, radar, wifi/online, globe already exists, clock, apps grid, up/down arrows exist).
- Exact spacing and type scale, within the spirit of the mockups.
- Where to fetch Manrope from (Google Fonts / the official GitHub release); ask the user before downloading.
