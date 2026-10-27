# Vantage "Traffic Insights" batch: design

Date: 2026-10-27. Status: draft for approval.

Builds on: branch `classic-graphite` (`311016e`), which has the classic layout, sorting, the app details popup and the other chat's features.

Research behind this design: a 6-agent investigation of the code, DB-IP, flags and protocol naming. The full findings are summarised in the "Facts this design rests on" section.

## Why

The user wants the GlassWire-style traffic analysis Vantage lacks. Their screenshots show:
- grouping the traffic list by Apps / Publishers / Traffic type / Countries;
- Network (WAN & LAN / LAN / WAN) and Direction (Incoming & Outgoing / Incoming only / Outgoing only) filters;
- a Usage view with Hosts, Traffic type and Countries columns, including flags;
- graph event markers, a scale dropdown and a pause button;
- "Mark all read" in place of "Clear log".

The user approved bundling the free DB-IP country database.

## Goals

- **Traffic list.** A "Group by" menu (Apps, Publishers, Traffic type, Countries), a "Network" menu (WAN & LAN, LAN, WAN) and a "Direction" menu (Incoming & Outgoing, Incoming only, Outgoing only) sit in the list header. The list, the chart, the Usage view and the strip's totals all follow them.
- **Usage view.** The chart-mode middle button opens three full-height columns: **Hosts**, **Traffic type** and **Countries**.
  - Each row shows a flag (Hosts and Countries), a name, a total and a bar split by direction.
  - The view follows the selected range, the selection (an app or a group row) and both filters.
  - An **Export** button saves the view as CSV.
- **Graph.**
  - Clock markers in a lane under the plot at alert times; hovering one shows the event.
  - A **scale** menu (Auto plus fixed maxima).
  - A **pause** button that freezes the graph.
- **Activity.** "Mark all read" replaces "Clear log". Clearing moves to Settings.
- **Offline countries.** A bundled DB-IP country database, with DB-IP attribution.

## Non-goals

- New capture sources. UDP, QUIC, ICMP and other unattributed bytes stay in one "Other traffic" bucket; ETW or WFP flow byte capture is out of scope.
- Who opened a connection. "Incoming"/"Outgoing" means bytes received/sent, not who initiated the connection.
- The world map, blocking by country, VirusTotal scan results, and automatic GeoIP updates. These stay in later batches.

## Anti-goals

- Higher idle CPU. No per-frame re-aggregation of history, and the repaint rules from the redesign still hold: no continuous repaint, no per-tick easing, and a 30 fps glide.
- UI stalls from the larger data file.
- Losing or corrupting existing history:
  - Old `data.json` files must load.
  - New stored preferences must not make an older build reset the store.
- Sending anything off the PC. GeoIP lookups are local, and nothing is fetched at runtime.
- Copying GlassWire's text, icons or look.

## Constraints

- Rust with eframe/egui 0.31. The vendored eframe patch stays.
- **Firewall code.** No changes to firewall logic in `wfp.rs`/`system.rs`. In `firewall.rs`, Action and state lines stay byte-identical.
- **`monitor.rs`/`store.rs`.** They may change for the data pipeline. That is new for this batch; earlier batches kept them off-limits.
- **Settings compatibility.** New settings fields are `u8` or `String`. Older builds ignore unknown fields, because there is no `deny_unknown_fields`. What resets the store is a new variant written into an enum that is already persisted (`Appearance`, `PlanPeriod`, `PlanTraffic`, `FirewallMode`, `EventKind`), or a changed field type. Neither is allowed. New `Bucket` fields use `#[serde(default)]`.
- **Verification.** No window automation, screenshots or PowerShell `Add-Type`, because the user's antivirus flags them. Verify with `cargo test` and `cargo build`. The user checks visuals.
- **Minimum window.** 1100×680. At that size the chart area is about 364 px tall as admin and about 324 px in limited mode.

## Facts this design rests on

- **What is recorded today.** Per-app [down, up] bytes in minute buckets (1440) and hour buckets (768). Global per-host bytes keyed by PTR name or IP text, plus a global LAN total. Remote port, protocol and IP are not stored.
- **TCP only.** Only TCP connections in Established, FinWait or CloseWait state, measured by EStats with admin rights, get per-connection bytes. UDP, ICMP and everything else falls into `__other`: 32% of bytes on this machine.
- **Limited mode** has only interface totals: no per-app, per-host or LAN bytes.
- **DB-IP Lite country MMDB.**
  - Format: 8.0 MB uncompressed, IPv4 and IPv6, released monthly.
  - License: CC BY 4.0. Embedding is allowed with attribution ("IP Geolocation by DB-IP", linking https://db-ip.com).
  - Reader: the `maxminddb` 0.32 crate (ISC licence) reads it from `include_bytes!`.
  - Download URLs only last about two months, so the file is pinned in the repo.
- **Flags.** Emoji flags can't render in egui 0.31. A flag atlas PNG rasterised from lipis/flag-icons (MIT) adds no runtime crates.
- **Data file size.** `data.json` is rewritten every 30 s while the shared lock is held. Today's size, measured on this machine and extrapolated, is about 4 MB at full retention. If every bucket were an admin-mode bucket it would be about 6.4 MB.

## Decided

### Data pipeline (monitor and store)

- **Limited mode.** No flows are written, so those buckets go through the history path below.
- **Classification.** At sampling time, on the monitor thread outside the lock, each measured TCP connection gets three labels:
  - **type** is the service port to `ports::classify` id.
    - The service port is the local port for inbound connections and the remote port otherwise.
    - If that port isn't in the table, the other port is tried when it is below 49152.
    - A connection with no match is `other_tcp`.
  - **country** is an ISO-2 code from GeoIP.
    - Private, link-local and ULA addresses (same rule as today's LAN check) get `LAN`.
    - No data, or a reserved range, gets `ZZ`.
    - Lookups are cached per IP in a bounded map local to the monitor loop.
  - **host** is the existing label: the PTR name or the IP text.

  Residual `__other` bytes get type `unattributed` and country `ZZ`.
- **Closed code sets.** These values are stored history, so they are fixed now:
  - type: table id, `other_tcp` or `unattributed`
  - country: ISO-2, `LAN` or `ZZ`
- **New `Bucket` field `flows`.** It maps app key to a list of `[host, type, country, down, up]` entries. The exact JSON shape is left to the builder, but it must be compact, e.g. a nested map or an index into per-bucket string tables.
- **Global `hosts` map.** New buckets **keep writing it, capped**: the top 32 per minute bucket and the top 64 per hour bucket, with the rest merged into `(other hosts)`. All-traffic host totals stay exact for the hosts shown, and running an older build loses only `flows`.
- **Unattributed bytes.** Their flow entries use host `""`.
- **Live samples.** They gain per-second flows for the last 300 seconds only, so the 5m view can regroup. Older live samples stay per-app only.
- **Size budget.**
  - Target: under about 10 MB at full retention, checked by a cargo test on a stated heavy profile.
  - Heavy profile: an hour bucket with 25 apps, 250 app-host pairs, 12 countries and 6 types; a minute bucket with 8 apps and 40 pairs.
  - Flows are capped per app, with separate N for minute and hour buckets (start at 8 and 16). The rest is merged into an `(other hosts)` row per type and country.
  - If the test fails, lower the hour-bucket N first. Retention doesn't change.
- **Saving.**
  - Take a snapshot clone under the lock, then serialise and write off the lock. An ignored release-mode test times the clone of a full-retention fixture.
  - The periodic save interval goes from 30 s to **120 s**, plus saves on exit and when the window hides to the tray. That's about 4× fewer disk writes, and a crash loses at most 2 minutes.
  - The JSON format stays.
- **LAN rule.** It stays the current address-class check, for consistency with stored history.

### GeoIP

- The unmodified `dbip-country-lite-2026-10.mmdb` goes in `assets/geo/` with `DBIP-LICENSE.txt`. It is embedded with `include_bytes!` through `maxminddb::Reader::from_source` in a `LazyLock<Option<Reader>>`.
- A corrupt asset means every country shows as `ZZ`, with no crash.
- Lookups read only `country.iso_code`.
- English country names come from a small static ISO→name table in `src/geo.rs`. Our own copy is fine, since country names are facts.
- **Attribution.**
  - "IP Geolocation by DB-IP" with a link under the Countries column.
  - A Settings footer line credits DB-IP (CC BY 4.0), flag-icons (MIT, if used) and Manrope (OFL).
  - The README gets a matching "Third-party data" section.
- There is no runtime download and no override file.

### Flags (pending the user's answer)

The chip fallback (`theme::country_chip`, new) is built first in either case. Flags are a separate final task, done only if the user approves downloading flag-icons.

If approved:
- Flags come from lipis/flag-icons (MIT, 4x3), rasterised once offline into one atlas PNG (`assets/flags/atlas.png`) with an index and `LICENSE`.
- They are drawn with `painter.image` and a UV sub-rectangle, at 16×12 logical size.
- Fallback: a 2-letter chip (`theme::country_chip`), used for `ZZ`, `LAN` (shown as a small network glyph), a missing code or a failed load.

If declined, chips are used everywhere.

### Traffic list (list header, left panel)

- **Header.** Three compact menus sit in a row: `Apps ▾`, `WAN & LAN ▾`, `⇅ ▾`. Each is a click-to-open popup menu with checkmarks, built with egui's popup and not ComboBox. Search stays where it is.
- **Saved settings.** All three are persisted as `u8` in `Settings`.
- **Pinned row.** It's renamed from "All apps" to "All traffic".
- **Group by.**
  - **Apps** is today's list, with pinned rows, "Connected now", sorting and the popup link.
  - **Publishers** gives one row per publisher, with apps that have no publisher grouped under "Unknown publisher". The icon is the largest app's icon.
  - **Traffic type** gives one row per type label, with a small type glyph.
  - **Countries** gives one row per country, with its flag. `LAN` is labelled "Local network" and `ZZ` "Unknown".
  - **Unattributed `__other`.**
    - In Apps and Publishers modes it gets its own "Other traffic" row with the globe glyph, never under "Unknown publisher".
    - In Type and Countries modes it is spread as classified.
    - In limited mode the `__other` row is hidden, because it equals the pinned total.
  - **Changing Group by** resets the selection to All traffic. That counts as a selection change, so it also resumes a paused graph.
  - In every mode, rows show ↓ and ↑ totals for the range and are sortable through the existing `sort::header`. Each mode has its own sort key (`traffic`, `traffic.publisher`, `traffic.type`, `traffic.country`), and the natural order is total descending.
  - "Connected now" shows only in Apps mode.
  - Selecting a group row filters the chart and the Usage view to that group. The app details popup link exists only on app rows.
- **Network filter.** WAN & LAN, LAN only or WAN only filters what the list, the chart and the Usage view count. WAN is everything that isn't `LAN`, including unattributed bytes.
- **Direction filter.** Incoming & Outgoing, Incoming only or Outgoing only.
  - The list shows only the matching column.
  - The chart draws only that series.
  - Totals use only that direction.
  - The Usage bars show one colour.

### Strip (bottom bar)

- **↓ and ↑ totals and rates.** They follow the selection and both filters. Under a direction filter, the excluded side is dimmed and shows "—".
- **Internet and Local network rows.** They follow the selection and the Direction filter. Under a Network filter, the excluded row is dimmed.
- **Data-plan gauge.** It ignores view filters; it has its own plan direction and LAN setting.

### Usage view (chart mode 2)

- **Modes.** The chart-mode buttons become: **Graph**, **Usage** (the new three columns) and **Bars** (today's usage-over-time bars, kept). Today's Hosts mode is removed, because Hosts is now a column. The globe or map stays a later batch.
- **Layout.** Three equal columns, full height, separated by 1px border lines.
  - Each column has a title, its own scroll and its own `sort::header` (name, total). The default sort is total descending.
  - Rows: flag or glyph, name (truncated, with the full text in a tooltip), total, and a thin bar split by direction relative to the column's largest row.
  - A host's flag is the country with the most bytes for that host.
- **Hosts column.**
  - With nothing selected it uses the global `hosts` map, old and capped new. With a selection it uses flows only.
  - Under a Network filter, an old host counts as LAN when its label is an IP that passes `net::is_private`; otherwise it counts as WAN.
  - Unattributed bytes appear as a final "Other traffic (no host)" row, so the three columns reconcile.
  - IP placeholder rows and their later PTR-name rows are not merged; that is a known limitation.
- **Content.** The view shows the current range, selection and filters. In limited mode it shows "Per-app and per-host detail needs administrator mode" with the existing restart button pattern.
- **Export button.** It is an icon with a tooltip, placed in the chart header.
  - The UI takes a snapshot of the three columns' rows and pushes `Action::ExportUsage(rows)`.
  - `run_actions` writes the file after the lock is released, to `Downloads\Vantage usage <range> <date-time>.csv`, then shows a toast and reveals the file with `explorer /select`.
  - Contents: exactly what the screen shows. Under a Direction filter the hidden direction is 0. The selection and filters go in the file name.
  - File format: UTF-8 with BOM, one long table with columns `column,name,country_code,download_bytes,upload_bytes,total_bytes`, and raw integers.
  - Fields are quoted per RFC 4180. Cells starting with `= + - @`, tab or CR are prefixed with `'` (host names come from remote PTR records).

### History without flows

This covers old buckets, limited-mode buckets and the bucket that is open at upgrade time.

- At read time only (never stored), compute the residual per bucket and app: `apps[k]` − Σ`flows[k]`, saturating.
- The residual shows as a UI-only **"Not classified"** row under Traffic type and as Unknown under Countries, so group rows always add up to All traffic.
- Network filter: with nothing selected, old buckets use the bucket's global `lan_down`/`lan_up`. With a selection, the per-app residual counts as WAN.

### Unknown codes

- Classification stores any code of exactly two uppercase letters as is, and anything else as `ZZ`.
- In the UI, an unknown country shows its code as the name, with a chip. An unknown type id shows its raw id with the "other" glyph and still forms its own group.
- When the app isn't elevated, a saved admin-only Group by or Network value is shown as Apps or WAN & LAN. The saved `u8` is left untouched (`effective_group(saved, admin)`).

### Graph

- **Event markers.**
  - **With a group selected.** A publisher shows its apps' events. A type, country or Other traffic row shows all Important events.
  - **Lane.** They sit in an 18px lane under the plot; the chart bottom moves up 18px.
  - **Which events.** Only the kinds the Activity "Important" filter shows. When an app is selected, only that app's events are marked.
  - **Clustering.** Markers closer than 14px merge into one marker with a count badge.
  - **Hover.** A tooltip lists up to 5 events, each with time and message.
  - **Not clickable.**
  - **Glide.** On 5m, markers move with the glided rect.
- **Scale menu.**
  - It sits at the top-left of the plot, e.g. `Auto ▾` or `1 MB/s ▾`.
  - Presets: Auto, 10 KB/s, 100 KB/s, 1 MB/s, 10 MB/s, 100 MB/s, 1 GB/s.
  - Units stay rates on every range, as today. There is one byte-based preset list, labelled through `theme::rate`, which shows bits when `speed_bits` is on.
  - The choice is saved per range as a `u8` preset index, with 0 meaning Auto.
  - When data exceeds a fixed max, the series is clipped and a small "▲ clipped" note shows.
- **Pause button.**
  - It is an icon at the top-right of the plot, and works on every range.
  - It freezes the graph's inputs: series, markers and the time axis. The list, Usage view and strip stay live.
  - The glide stops while paused.
  - A "Paused" chip shows.
  - Changing range, selection or a filter resumes.
  - The live sample store is kept as is; the paused view is a snapshot.

### Performance

- **Aggregate cache.** List, chart and Usage aggregates are computed once per change. The cache key is (range, group-by, filters, selection, newest bucket `t`, newest live `t`, pause state). The work is cloned out of the lock or done in place without cloning per frame. Re-aggregating history every frame is forbidden.
- **GeoIP.** Lookups never happen in the UI.

### Activity

- **Read state.** Today, every frame the Activity page is shown marks all events read. That stops.
  - Unread dots, the badge, and whether "Mark all read" is enabled all follow one rule: important and unread.
  - Firewall events caused by the user's own click are created already read.
  - A row is marked read when it is clicked, or when it is opened through its app link.
  - **Mark all read** sits where "Clear log" was and marks every event read. It is disabled when nothing is unread.
  - The tab badge counts unread important events, as today.
- **Clear activity log** moves to Settings, in the existing "Reset usage history" pattern with a two-step confirm. The confirm text says it also removes graph markers and the Alerts tab history.

## Calls Claude made (check these)

1. **Per-app scope.** Everything (Usage columns, filters, graph, group selection) follows the selected app or group, as in the user-approved playback.
   - This needs the per-app `flows` store, so the data file grows from about 4 MB to under about 10 MB on a heavy day.
   - In exchange, saving moves off the lock and runs every 2 minutes.
2. **Unattributed traffic.** It stays visible as "Other traffic (UDP, ICMP, system)" under type and Unknown under country. No new capture source.
3. **Limited mode.**
   - Group by Publishers/Type/Countries, LAN-only/WAN-only and the Usage view are shown disabled, with a "Needs administrator mode" note.
   - Direction, markers, scale, pause and Mark all read work.
4. **Graph units stay rates.** The "2 GB" in the screenshot comes from GlassWire's per-slice amounts. Vantage keeps rates, so the scale presets are rates.
5. **Modes.** Graph | Usage | Bars. Today's Hosts mode is removed, and Bars is kept.
6. **Pause.** It freezes only the graph, on all ranges, and changing what's viewed resumes it.
7. **Markers.** Important events only, filtered to the selected app, hover only.
8. **CSV.** Written to Downloads with a toast and a reveal; no Save-As dialog.
9. **Filters live in the list header.** The data-plan gauge ignores them.
10. **Mark all read.** It marks everything, opening the page no longer auto-marks events read, and clicking a row marks it read.
11. **LAN rule unchanged.** The current address-class check, so CGNAT and Tailscale `100.64/10` count as WAN.
12. **GeoIP.** The raw MMDB is embedded (exe grows about 8 MB), with no override and no runtime download.
13. **Older history** shows as "Not classified" under Traffic type and Countries, so totals always reconcile.
14. **Save interval** goes from 30 s to 2 min, so a crash can lose up to 2 minutes of history.
15. **Strip** totals and the Internet/Local rows follow the selection and the filters. The data-plan gauge stays global.

## Left to the builder

- The compact JSON shape of `flows`, and the per-app host cap N (pick N to meet the about 6 MB budget; document it).
- How saving gets off the lock.
- New modules to create: `src/ports.rs` (`ports::classify`) and `src/geo.rs`.
- The port table contents: about 110 self-written entries, IANA as reference, no Nmap or Wireshark text.
- The cache structure, and how the snapshot is taken.
- Glyphs for traffic types: a handful of categories (web, name lookup, file sharing, mail, remote access, streaming, gaming, other).
