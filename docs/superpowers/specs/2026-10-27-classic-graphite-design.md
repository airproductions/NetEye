# Vantage "Classic Graphite" rebuild

Date: 2026-10-27. Status: draft for approval.
Builds on: branch `glasswire-feature-parity` at `2da002e`. That is `main` (Mesh Glass redesign and tray fix) plus the other chat's feature-parity commit.

## Why

The Mesh Glass redesign changed how Vantage is organised. It swapped the named top tabs for an unlabelled icon rail, floated cards in gutters, and added a page header. The user finds the new organisation worse and wants the previous layout back, with improvements. Most of all, the lists can't be sorted. A click on an app should also open a details popup, as in GlassWire. The user picked the look in the brainstorm mockup `.superpowers/brainstorm/1917-1793059582/content/classic-layout.html`, option **A · Classic graphite**.

## Goals

- Bring back the pre-redesign layout and its flat graphite look (`main`@`2373ff6` is the reference).
- Keep the redesign improvements that aren't about layout: Manrope, gated animations, light mode, the usage strip, the CPU fixes and the tray fix.
- Add sortable columns to every list.
- Add an app details popup that opens when you click an app's name.
- Keep every feature added in `2da002e`: firewall modes and profiles, the Ask-to-Connect bar, security monitors, Activity filters and sort, the data plan, device renaming, and the new settings.

## Non-goals

- New monitoring or firewall behaviour. The seven feature batches in `docs/research/2026-10-27-glasswire-feature-gap.md` come later, one at a time.
- Hide app, Incognito and Trust in the popup. Those belong to batch 4, so the popup's More menu leaves room for them but doesn't build them.
- VirusTotal scan results, which need an API key. The popup only links to VirusTotal.

## Anti-goals

- Losing or breaking anything from `2da002e`.
- Idle CPU going back up. The repaint-gating rules from the redesign's final fix still hold.
- Copying GlassWire's text, icons or exact look. We take the behaviour only and write our own wording.

## Constraints

- Rust with eframe/egui 0.31, and the vendored eframe patch stays.
- Don't change `wfp.rs`, `system.rs`, `net.rs` or `monitor.rs` logic. Adding a read-only accessor is fine.
- In `firewall.rs`, `Action` and state lines stay as they are.
- Settings and data must keep loading from existing files: new fields use `#[serde(default)]`.
- Minimum window size stays 1100×680, with the native title bar.
- Don't use window-automation or screenshot scripts (antivirus). Verify with tests and builds, and let the user look.

## Decided

### Look: classic graphite (dark)

The palette comes back from `2373ff6`:

| Role | Colour |
|---|---|
| bg | `#14161b` |
| panel | `#1a1d23` |
| raised | `#21252d` |
| row_alt | `#1d2027` |
| hover | `#272c36` |
| selected | `#233352` |
| border | `#2a2f39` |
| text | `#e7eaf0` |
| muted | `#8d95a5` |
| faint | `#5c6370` |
| accent | `#5b8dff` |
| down | `#2dd4bf` |
| up | `#a78bfa` |
| good | `#34d399` |
| bad | `#f87171` |
| warn | `#fbbf24` |
| orange | `#fb923c` |

The look is flat: opaque panels, 1px borders, no backdrop gradient, no mesh, no glass, no glow.

### Light graphite (Claude's call)

| Role | Colour |
|---|---|
| bg | `#f3f4f7` |
| panel | `#ffffff` |
| raised | `#eceef2` |
| row_alt | `#f7f8fa` |
| hover | `#e6e9ef` |
| selected | `#dde7fb` |
| border | `#dcdfe5` |
| text | `#1b1f27` |
| muted | `#5f6776` |
| faint | `#9aa1ad` |
| accent | `#3b6fe0` |
| down | `#0f9f8f` |
| up | `#7c5cd6` |
| good | `#16a34a` |
| bad | `#dc2626` |
| warn | `#b7791f` |

The Dark / Light / Follow Windows setting and the themed title bar stay as they are.

### Layout (from `2373ff6`)

- **Top bar (78px):**
  - Left: the logo, "Vantage", and FULL MONITORING / LIMITED MODE in green or amber underneath.
  - Middle: five named two-line tabs (Traffic Monitor, Firewall Rules, Activity Log, Network Scanner, Hardware Resources), each with an icon. Activity keeps its unread-count badge.
  - Right: the settings gear.
  - The selected tab's highlight slides between tabs.
- **Below the bar:** the limited-mode banner, then the Ask-to-Connect bar from `2da002e`, restyled flat.
- **Removed:** the left icon rail, the page header with its status chips, the backdrop, and floating cards.
- **Traffic:**
  - Left: a full-height app list panel, 40% wide, at most 470px.
  - Right: the chart area.
  - Bottom: the usage strip, full width and flat, about 92px tall. It keeps the gauge, totals, Internet/LAN bars and app/connection counts from the redesign. Online status goes back to the right end of the strip, as in the old footer.
  - Icon-only view switch and short range codes stay (Claude's call).
- **Other pages:** back to the `2373ff6` structure, which is full-width flat toolbars and tables, including the controls `2da002e` added (firewall mode dropdown, profiles, log filters). The icons and tooltips the redesign added to toolbars stay.
- **Page change:** a short fade stays (0.25s, gated).

### Sorting

- **How it works:**
  - Clicking a column header sorts by it. Clicking the same header again reverses the order.
  - A small ▼ or ▲ marks the active column, and headers show a hover state.
  - Sorting is stable and ties fall back to name. Text sorts case-insensitively. IP addresses sort numerically, octet by octet. Times and sizes sort by value. Empty values ("—") always go last.
- **Firewall:** every column (Apps, Incoming, Outgoing, Publisher, Version, Hosts, ↓, ↑, Traffic). The Active and Inactive groups stay, and each sorts inside itself (Claude's call). Incoming and Outgoing sort as Blocked before Allowed. Hosts sorts by count.
- **Scanner:** every column (status, Type, Name, Description, IP, MAC address, Last seen, First seen).
- **Traffic app list:** App (name), ↓ and ↑. "All apps" and "All traffic" stay pinned at the top, and the "Connected now" section sorts by the same column (connections as a tiebreak).
- **Hardware:** keeps its existing column sort, restyled to the same header look.
- **Activity:** keeps the date/type/app sort from `2da002e`.
- **Persistence:** each list's sort (column and direction) is saved in settings, so it survives a restart.
- **Defaults:** each list starts in today's order.

### App details popup

- **Opening:** click an app's name or icon in the Traffic app list, the Firewall table or an Activity entry. The popup is anchored next to the click and stays inside the window. Clicking outside or pressing Esc closes it, and only one is open at a time.
- **Header:** app icon, display name, and two tabs, **Info** and **Alerts (n)**.
- **Info tab:**
  - Name, Path, Version and Publisher. When there's no publisher, show "Unknown" (see the Publisher call below).
  - First seen, last seen and first network use.
  - Total ↓/↑, and the current block state.
  - Values that are missing show "—".
- **Alerts tab:** the app's events from the Activity log, newest first, with time and message. Empty state: "No alerts for this app".
- **Footer buttons:**
  - **VirusTotal:** opens `https://www.virustotal.com/gui/file/<sha256>` in the default browser. The SHA-256 is computed locally only when clicked, off the UI thread, and nothing is uploaded. If there's no path, or the file can't be read, the button is disabled and has a tooltip saying why.
  - **Block ▾:** a menu with Allow all, Block incoming, Block outgoing and Block both. It uses the existing `Action::Block`, and the current state is checked in the menu.
  - **More ▾:** Open file location (Explorer with the file selected) and Copy path. Hide, Incognito and Trust are added here in batch 4.

### Calls made after the builder review (Claude's, so check these)

- **Base branch.** The other chat's work is on branch `glasswire-feature-parity` (`2da002e`), not yet merged into `main`. This rebuild branches from `glasswire-feature-parity`. When and in what order to merge it into `main` is the user's choice.
- **What a click does.** Only the app's **name or icon** opens the popup. Clicks elsewhere keep today's behaviour:
  - Traffic: clicking the rest of the row selects the app and filters the chart. Clicking the name or icon also selects the row, then opens the popup.
  - Firewall: the chevron and the rest of the row expand the connections. The Incoming and Outgoing cells stay click-to-block.
  - Name and icon show a hand cursor and an underline on hover.
- **Activity.** Only entries with an app open the popup, through their app name or icon. The popup anchors to the clicked name or icon.
- **Alerts tab.** It lists every event whose `app` is this app, the user's own firewall changes included. n is the total count. Opening the tab doesn't mark anything read, so the Activity badge doesn't change.
- **Publisher.** Shows the version-info company name, or "Unknown" if there isn't one. There's **no** "Unsigned" warning, because Vantage doesn't check signatures yet, and flagging apps that just lack version info would mislead. A real signature check, with its warning, comes in batch 2.
- **Block menu.** It pushes the same `Action::Block` as the Firewall table cells, so the firewall event, WFP apply and mode handling are identical. It's available whenever the table cells are.
- **Live data.** The popup holds the app key and re-reads the store each frame, so totals update on the monitor's normal tick and it adds no repaints of its own. If the app leaves "Connected now", the popup stays open with stored data.
- **Traffic sort keys.** ↓ and ↑ sort by the totals over the selected range that the rows show. "Connected now" uses the same key, with connections as the tiebreak.
- **Tests expected.**
  - Comparators: case-insensitive text, numeric IP, empty values last, stable ties.
  - Reversing a sort with a second click.
  - Sort-setting round trips, and old settings files loading with default sorts.
  - The VirusTotal URL from a hash.
  - Popup alert filtering.
  - Palette contrast for both themes.

## Left to the builder

- How to restore the old layout: either revert the layout code to `2373ff6` and port in `2da002e`'s additions, or edit the current code back. Either way, the `2da002e` features must survive.
- How the palette is shaped. The runtime palette from the redesign can stay; the glass-only fields can go.
- The sort-state data structure, and where comparison helpers live. They should be pure and unit-tested.
- How the popup is built: an `egui::Area` or a window.
- Exact spacing, within the spirit of `2373ff6` and the mockup.
