# Vantage "Traffic Insights": Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build these features on top of a new per-app flow history classified by traffic type and country (from a bundled DB-IP database):
- Group by, Network and Direction filters;
- a three-column Usage view with flags and CSV export;
- graph markers, a scale menu and pause;
- "Mark all read".

**Architecture:**
- **Monitor thread:** classifies each measured TCP connection as (host, type, country), using the new `ports` and `geo` modules.
- **Store:** each bucket records compact per-app `flows`, next to capped `hosts`. Saving moves off the lock, is serialised by its own mutex, and runs every 120 s.
- **`ui/traffic_data.rs`** (new, pure): turns buckets and live samples into one cached `Agg` per `Query` (range, group, filters, selection).
- **UI:** the Traffic list, the Usage view (`ui/usage_view.rs`), the graph and the strip all draw from `Agg`.
- **Flags:** come from a committed atlas PNG, generated once by `tools/flag-atlas`.

**Tech Stack:**
- Rust, eframe/egui 0.31 (vendored patch) and serde.
- New crate: `maxminddb = "0.32"` (ISC licence, `default-features = false`).
- Tool-only crates, not used by the app: `resvg` and `png`.

**Spec:** `docs/superpowers/specs/2026-10-27-traffic-insights-design.md` (approved, including flags). It is binding.

## Global Constraints

- **Branch.** Base on `classic-graphite`. Waves A and B run in parallel git worktrees. Don't merge into `main`.
- **Data compatibility.**
  - No new variants in enums that are already persisted (`Appearance`, `PlanPeriod`, `PlanTraffic`, `FirewallMode`, `EventKind`), and no changed field types.
  - New `Settings` fields are `u8` or `String`.
  - New `Bucket` fields have `#[serde(default)]`.
- **Firewall code.**
  - No changes to firewall logic in `wfp.rs` or `system.rs`.
  - In `firewall.rs`, lines that push an `Action` or mutate `s` stay byte-identical.
- **Repaints.**
  - No continuous repaint and no per-tick easing (`motion::EaseGate`/`gated`).
  - The glide stays capped at 30 fps.
  - Never re-aggregate history every frame.
- **Verification.**
  - No window automation, screenshots or PowerShell `Add-Type`; the user's antivirus flags them.
  - Verify with `cargo test` and `cargo build` only.
- **Gate per task.**
  - `cargo build` shows no new warnings; only the 3 pre-existing ones (fields `local`/`total`, fields `alias`/`speed`, Glyph variants `Refresh`/`Check`).
  - `cargo test` passes.
  - A module may carry a temporary `#![allow(dead_code)] // consumed by Task N; removed in Task 11`.
- **Commits.** Messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Copy.** All UI copy and port-table labels are our own wording. Don't take text from GlassWire, Nmap or Wireshark.
- **Downloads.** Only these are approved:
  - the DB-IP 2026-10 country MMDB;
  - the lipis/flag-icons source at a pinned tag;
  - crates from crates.io.

  Record the source URL and SHA-256 of each in its asset README.

## Review Focus

1. **Upgrading with a month of old history.** The 7d and 30d views mix old buckets (no `flows`) with new ones. Type and Country group rows and all three Usage columns must add up to "All traffic", via "Not classified" rows, with no panic and no double counting. Tests in Task 6.
2. **Limited (non-admin) mode.**
   - No flows are written.
   - An admin-only saved Group by or Network value falls back to Apps / WAN & LAN without being overwritten.
   - The Usage view shows the administrator note.
   - Direction, markers, scale, pause and Mark all read still work.

   Tests in Tasks 5, 6 and 7.
3. **Remote-controlled host names in the CSV.** A PTR name like `=HYPERLINK(...)`, or one containing commas, quotes or newlines, must come out as an inert, correctly quoted cell. Test in Task 8.
4. **Concurrent or downgraded saves.**
   - A background save racing an exit or hide save must never corrupt `data.json`. A corrupt file resets all history.
   - A file an older build rewrote (no `flows`) must load as "Not classified".

   Tests in Tasks 4 and 6.
5. **Pausing the graph, then hiding to the tray and reopening.** No stuck glide, and no repaint while paused. Changing range, selection or a filter resumes. Test in Task 9.

---

### Setup (controller, before Wave A)

Create stub files containing only a `//!` doc line, so that parallel tasks never edit the module lists:
- `src/ports.rs` and `src/geo.rs`, declared in `src/main.rs`;
- `src/ui/flags.rs`, `src/ui/traffic_data.rs`, `src/ui/usage_view.rs`, `src/ui/csv_export.rs` and `src/ui/markers.rs`, declared in `src/ui/mod.rs`.

Also add `tools/flag-atlas/target/` to `.gitignore`. Commit as "Scaffold Traffic Insights modules".

## Wave A: Tasks 1–4 in parallel (disjoint files)

### Task 1: Port → traffic-type table

**Files:**
- Modify: `src/ports.rs`

**Interfaces:**
- Produces:
  - `pub enum Pr { Tcp, Udp, Both }`
  - `pub enum Cat { Web, Names, Files, Mail, Remote, Media, Games, System, Other }`
  - `pub struct PortType { pub lo: u16, pub hi: u16, pub proto: Pr, pub id: &'static str, pub label: &'static str, pub desc: &'static str, pub cat: Cat }`
  - `pub const OTHER_TCP: &str = "other_tcp"; pub const UNATTRIBUTED: &str = "unattributed"; pub const UNCLASSIFIED: &str = "__unclassified";` (`UNCLASSIFIED` is UI-only and is never stored)
  - `pub fn classify(local_port: u16, remote_port: u16, inbound: bool, tcp: bool) -> &'static str`
  - `pub fn label(id: &str) -> String`
  - `pub fn category(id: &str) -> Cat`
- How `classify` picks a type:
  1. The service port is `local_port` when `inbound`, otherwise `remote_port`.
  2. If the service port has no row, try the other port when it is below 49152.
  3. Otherwise return `OTHER_TCP`.
  - A proto-specific row beats `Both`, and a single port beats a range.
  - No `Tcp` or `Both` row may cover anything in 49152–65535 (the Windows ephemeral range).
- Labels:
  - `label(OTHER_TCP)` is "Other TCP".
  - `label(UNATTRIBUTED)` is "Other traffic (UDP, ICMP, system)".
  - `label(UNCLASSIFIED)` is "Not classified".
  - An unknown id returns the id itself.
- `category` of an unknown or sentinel id is `Cat::Other`.

- [ ] **Step 1: Write the failing tests:**
  - `https_outbound`: `classify(50123,443,false,true)=="https"`
  - `rdp_inbound`: `classify(3389,51000,true,true)=="rdp"`
  - `fallback_to_other_port`: `classify(8080,50999,false,true)` returns the 8080 row's id.
  - `unknown_is_other_tcp`: `classify(50001,50002,false,true)==OTHER_TCP`
  - `precedence_specific_beats_range_and_both`: add a test table with an overlapping range row and a `Both` row. Assert the single-port and proto-specific row wins. Make the resolver generic over a slice so the test can pass its own table.
  - `labels_for_sentinels` (all three) and `category_unknown_is_other`
  - `table_valid`:
    - every id appears once;
    - `lo<=hi` for every row;
    - no `Tcp` or `Both` row overlaps 49152–65535.
- [ ] **Step 2:** Run `cargo test ports`. Expected: compile FAIL.
- [ ] **Step 3: Implement.** Write about 110 self-written rows (use IANA as a reference). Cover web, names and discovery, Windows and directory, files, mail, remote access, media and streaming, games and launchers, and system and updates. Labels are short ("HTTPS", "DNS", "SMB"), and `desc` is plain English ("Secure web").
- [ ] **Step 4:** Run `cargo test ports` and `cargo build`. Expected: PASS, and no new warnings. Commit "Port to traffic-type table".

### Task 2: GeoIP (DB-IP country MMDB)

**Files:**
- Modify: `src/geo.rs` and `Cargo.toml` (add `maxminddb = { version = "0.32", default-features = false }`)
- Create:
  - `assets/geo/dbip-country-lite-2026-10.mmdb`
  - `assets/geo/DBIP-LICENSE.txt` (CC BY 4.0, the line "IP Geolocation by DB-IP", https://db-ip.com, and a note that the data is unmodified)
  - `assets/geo/README.md` (URL, release month, SHA-256)

**Interfaces:**
- Produces:
  - `pub const LAN: &str = "LAN"; pub const UNKNOWN: &str = "ZZ";`
  - `pub fn country_of(ip: IpAddr) -> String`. It returns `LAN` when `net::is_private(&ip)`; this is the same rule as today's LAN check at `monitor.rs:727`, with no `is_local` added. Otherwise it returns the normalised `country.iso_code`, or `UNKNOWN`.
  - `pub fn normalize(code: Option<&str>) -> String`. It returns the code only when it is exactly 2 ASCII uppercase letters, otherwise `UNKNOWN`.
  - `pub fn name(code: &str) -> String`:
    - `LAN` gives "Local network".
    - `ZZ` gives "Unknown".
    - An ISO code gives its English name from a static table of about 250 entries, which must include `XK` "Kosovo".
    - Anything else gives the code itself.
  - `pub fn loaded() -> bool`
  - `fn lookup_in(reader: Option<&maxminddb::Reader<&[u8]>>, ip: IpAddr) -> String`. This is the testable core: `None`, or a failed lookup, gives `UNKNOWN`.
- Reader: `static GEO: LazyLock<Option<maxminddb::Reader<&'static [u8]>>>`, built from `include_bytes!` with `Reader::from_source`.
- **API check first:** the maxminddb lookup API changed around 0.27, so check it on docs.rs for 0.32. As of 0.27, `lookup(ip)` returns a result whose `decode_path(...)` reads `country`/`iso_code`. Use whatever the 0.32 docs show.

- [ ] **Step 1: Download the database.** Get `https://download.db-ip.com/free/dbip-country-lite-2026-10.mmdb.gz` (approved) into an empty scratch directory. Gunzip it, copy it to `assets/geo/`, and record both SHA-256 values.
- [ ] **Step 2: Write the failing tests:**
  - `private_is_lan` for `192.168.1.5`
  - `ula_is_lan` for `fd00::1`
  - `link_local_is_lan` for `fe80::1`
  - `cgnat_is_not_lan`: `country_of(100.64.1.1) != LAN`
  - `public_lookup`: `loaded()` is true, and `country_of(8.8.8.8)=="US"`
  - `corrupt_reader_is_unknown`: `Reader::from_source(&b"garbage"[..])` fails, so `lookup_in(None, ip)=="ZZ"`
  - `normalize_rules`: `"IL"`→`IL`, `"il"`→`ZZ`, `"ISR"`→`ZZ`, `None`→`ZZ`
  - `names`: `IL`, `XK` (not `"XK"`), `LAN`, `ZZ`, and `QQ`→`"QQ"`
- [ ] **Step 3:** Run `cargo test geo`. Expected: FAIL.
- [ ] **Step 4:** Implement.
- [ ] **Step 5:** Run `cargo test` and `cargo build`. Expected: PASS, and no new warnings. Commit "Embed DB-IP country database".

### Task 3: Flag atlas and country chips

**Files:**
- Create:
  - `tools/flag-atlas/` (its own Cargo project with an empty `[workspace]` table; commit its `Cargo.lock`)
  - `assets/flags/atlas.png`
  - `assets/flags/index.txt`
  - `assets/flags/LICENSE` (flag-icons MIT)
  - `assets/flags/README.md` (tag, URL, SHA-256)
- Modify: `src/ui/flags.rs`

**Interfaces:**
- Produces in `ui::flags`:
  - `pub const CELL: [u32; 2] = [32, 24]; pub const PAD: u32 = 1; pub const SIZE: egui::Vec2 = egui::vec2(16.0, 12.0);`. Every caller draws flags at `SIZE`.
  - `pub fn uv(index: usize, cols: usize, rows: usize) -> egui::Rect`. It returns the **inner** cell rect, without the padding:
    - min = `((col*34+PAD)/W, (row*26+PAD)/H)`
    - max = `min + (32/W, 24/H)`
    - where `W = cols*34` and `H = rows*26`.
  - `pub fn draw(ui: &egui::Ui, rect: egui::Rect, code: &str)`:
    - It lowercases `code` and looks it up in the index.
    - It draws the flag from the atlas texture, which is loaded once, lazily, and cached in `ctx.data`.
    - With no flag, or if the atlas fails to load, it draws `chip(...)`.
    - `LAN` draws the network chip.
  - `pub fn chip(p: &egui::Painter, rect: egui::Rect, code: &str)`: a rounded `c::raised()` rect with the 2 letters in `c::muted()`.
- `index.txt`: the first line is `cols`, then one lowercase ISO code per cell, row-major. It contains ISO 3166-1 alpha-2 codes plus `xk`, and no sub-regions.

- [ ] **Step 1: Download the flags.** Get the lipis/flag-icons source archive at a pinned release tag (approved) into an empty scratch directory. Record the URL and SHA-256.
- [ ] **Step 2: Write the generator.** `tools/flag-atlas` rasterises `flags/4x3/*.svg` with `resvg` at 32×24, packs them alphabetically with 1 px padding (34×26 cells), and writes `atlas.png` and `index.txt`. Run it from its own directory; its `target/` is git-ignored. Commit the outputs and the tool.
- [ ] **Step 3: Write the failing tests:**
  - `uv_inner_rect`: `uv(0,16,17).min == (1/544, 1/442)`, and the last index (`rows*cols-1`) maps to the last row and column.
  - `index_parses`: at least 245 codes, including `il`, `it`, `us` and `xk`.
  - `atlas_decodes`: `eframe::icon_data::from_png_bytes` gives `cols*34 × rows*26`.
  - `draw_lowercases`: the lookup helper finds `"IL"`.
- [ ] **Step 4:** Run `cargo test flags`. Expected: FAIL.
- [ ] **Step 5:** Implement.
- [ ] **Step 6:** Run the tests and `cargo build`. Commit "Flag atlas and country chips".

### Task 4: Store: compact flows, capped hosts, view settings, safe off-lock save

**Files:**
- Modify:
  - `src/store.rs`: `Bucket` (:184-194), `add_usage` (:329-372), `save` (:307-318), `Data` (:260), `Settings`
  - `src/monitor.rs`: `LiveSample` (:26-32), the push at :877, and the `add_usage` call at :881

**Interfaces:**
- **In-memory flow type** (used by the monitor, live samples and aggregation):
  `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)] pub struct Flow { pub h: String, pub ty: String, pub cc: String, pub d: u64, pub u: u64 }`
  - `h` is the host label, or `""` for unattributed bytes.
  - `ty` is a `ports` id.
  - `cc` is an ISO-2 code, `LAN` or `ZZ`.
- **Compact stored shape in `Bucket`** (both `#[serde(default)]`):
  - `pub strs: Vec<String>`: a per-bucket string table.
  - `pub flows: HashMap<String /*app*/, Vec<[u64; 5]>>`: each entry is `[host_idx, type_idx, cc_idx, down, up]`, where the indices point into `strs`.
  - Helpers:
    - `pub fn add_flow(&mut self, app: &str, f: &Flow)` interns strings and merges on (h, ty, cc).
    - `pub fn flows_of(&self) -> impl Iterator<Item = (&str /*app*/, Flow)>`
- **Caps:**
  - `pub const MINUTE_FLOW_CAP: usize = 8; pub const HOUR_FLOW_CAP: usize = 16; pub const MINUTE_HOST_CAP: usize = 32; pub const HOUR_HOST_CAP: usize = 64; pub const OTHER_HOSTS: &str = "(other hosts)";`
  - `fn cap_bucket(b: &mut Bucket, flow_cap: usize, host_cap: usize)`:
    - Per app, keep the `flow_cap` hosts with the most bytes (summed across ty and cc). Merge the rest into `OTHER_HOSTS` entries, one per (ty, cc). `h == ""` is never merged away.
    - Cap `hosts` the same way into `OTHER_HOSTS`.
    - Re-intern `strs`.
  - Per app this keeps at most `flow_cap + |types×countries present|` entries. Document that on the constants, together with the chosen N and the shape.
  - When to cap:
    - when a bucket closes (rollover);
    - on the open bucket too, whenever an app's entries pass `4 × flow_cap` or `hosts` passes `4 × host_cap`.
- **`add_usage`:** `add_usage(&mut self, t: i64, totals: [u64; 4], apps: &HashMap<String,[u64;2]>, hosts: &HashMap<String,[u64;2]>, flows: &HashMap<String, Vec<Flow>>) -> [u64; 2]`
- **`Data`:** add `Clone` to its derive at :260.
- **Settings:** `pub traffic_group: u8, pub traffic_net: u8, pub traffic_dir: u8`, plus `pub graph_scale_5m: u8, pub graph_scale_3h: u8, pub graph_scale_24h: u8, pub graph_scale_7d: u8, pub graph_scale_30d: u8`. All default to 0; for the scales, 0 means Auto.
- **Saving:**
  - `static SAVE_LOCK: Mutex<()>`, with a monotonic `static SAVE_GEN: AtomicU64`.
  - `pub fn snapshot(&mut self) -> Option<(PathBuf, u64 /*gen*/, Data)>` returns `None` unless the store is dirty. Otherwise it clones `data`, clears dirty and takes the next generation.
  - `pub fn write_snapshot(dir: &Path, gen: u64, data: &Data)` holds `SAVE_LOCK` across serialisation, the write to `data.json.tmp` and the rename. It skips the write if a newer generation has already been written.
  - `save()` becomes `snapshot` followed by `write_snapshot`.
- **`LiveSample`:** `pub flows: Vec<(String /*app*/, Flow)>`. In this task the push site sets it to `Vec::new()`; Task 5 fills it.

- [ ] **Step 1: Write the failing tests:**
  - `flows_merge_by_key`: two ticks with the same (h, ty, cc) give one entry with summed d/u.
  - `compact_roundtrip`: `add_flow`, then serde round-trip, then `flows_of` returns the same flows.
  - `cap_keeps_top_hosts`: 12 hosts with cap 8 gives 8 named hosts plus `OTHER_HOSTS` per (ty, cc), with bytes conserved.
  - `cap_keeps_unattributed`: an `h==""` entry survives the cap.
  - `cap_hosts_merges_rest`: 40 hosts with cap 32 gives 32 hosts plus `OTHER_HOSTS`, with bytes conserved.
  - `rollover_caps_closed_bucket_minute_and_hour`: both the minute and the hour boundary.
  - `open_bucket_soft_cap`: pushing 5× `flow_cap` hosts into the open bucket triggers a cap.
  - `old_file_without_flows_loads`: buckets with no `strs` or `flows` load with `apps` and `hosts` intact (Review Focus 4).
  - `settings_view_fields_default`
  - `concurrent_writes_never_corrupt`: two threads write snapshots of different gens at once, 50 times; afterwards `data.json` parses and holds the newest gen's data (Review Focus 4).
  - `heavy_profile_under_budget`: build full retention.
    - Minute buckets: 8 apps, 40 app-host pairs, one browser app with 25 hosts.
    - Hour buckets: 25 apps, 250 pairs, one browser with 120 hosts, 6 types, 12 countries.
    - Every host label is 32 or more chars. The `apps` and `hosts` maps are filled, then capped.
    - Assert that the serialised size is under 10,000,000 bytes.
  - `#[ignore] snapshot_clone_time`: in release, cloning the heavy fixture takes under 5 ms. If it fails, keep the design and log the measured time in the report.
- [ ] **Step 2:** Run `cargo test store`. Expected: FAIL.
- [ ] **Step 3:** Implement it. Update the monitor call site to pass `&HashMap::new()` for flows. Route `Monitor::save()` (monitor.rs:191-193) through `snapshot()` taken under the lock, then `write_snapshot()` after the lock is released.
- [ ] **Step 4:** Run `cargo test` and `cargo test --release -- --ignored snapshot_clone_time`. If the size test fails, lower `HOUR_FLOW_CAP` first. Commit "Store compact per-app flows, capped hosts, safe saves".

## Wave B: Tasks 5, 6 and 10 in parallel (after Wave A merges)

### Task 5: Monitor: classify, record flows, save every 2 min

**Files:**
- Modify:
  - `src/monitor.rs`: the sampling loop (:650-760) and publishing (:870-911)
  - `src/ui/mod.rs`: one line; the hide-to-tray path at :170-172 calls `self.monitor.save()`

**Interfaces:**
- Consumes:
  - `ports::{classify, UNATTRIBUTED}` and `geo::{country_of, UNKNOWN}`
  - `store::Flow`, `LiveSample.flows` and `Store::{snapshot, write_snapshot}` (all from Task 4)
- Produces:
  - `pub const SAVE_EVERY: Duration = Duration::from_secs(120);`
  - `fn accumulate_flow(per_flow: &mut HashMap<String, HashMap<(String,String,String),[u64;2]>>, app: &str, host: &str, ty: &str, cc: &str, rate: [u64;2])`
  - `fn residual_flow(other: [u64;2], admin: bool) -> Option<Flow>`. It returns `Some(Flow{h:"",ty:UNATTRIBUTED,cc:"ZZ",d,u})` only when `admin` and `other != [0,0]`.
  - `struct GeoCache { map: HashMap<IpAddr,String>, cap: usize }`:
    - `fn get_with(&mut self, ip: IpAddr, f: impl FnOnce(IpAddr)->String) -> String` clears the map when it reaches `cap` (4096).
    - `fn get(&mut self, ip) -> String` calls `get_with(ip, geo::country_of)`.
    - It lives in the monitor loop, not in `Shared`.
  - `fn trim_live_flows(live: &mut VecDeque<LiveSample>, keep: usize)` clears `flows` on every sample except the newest `keep` (300).
- Behaviour:
  - In the `rate != [0,0]` branch, call `accumulate_flow(app, label, classify(c.local_port, c.remote_port, inbound, true), geo.get(c.remote), rate)`.
  - Add `residual_flow` for `OTHER_KEY`. Limited mode writes no flows.
  - Build the `Vec<Flow>` map for `add_usage`, and fill `LiveSample.flows`, then call `trim_live_flows`.
  - Save every `SAVE_EVERY`: take `snapshot()` while the guard is held, then call `write_snapshot` after the guard drops.
  - Don't touch firewall code paths.

- [ ] **Step 1: Write the failing tests:**
  - `accumulate_merges`: two calls with the same key sum.
  - `residual_only_when_admin`, and `residual_zero_is_none`
  - `geo_cache_bounded`: with `get_with` and a counting closure, cap+1 distinct IPs leave `len <= cap`, and a repeated IP doesn't call the closure.
  - `live_flows_trimmed`
  - `save_every_is_two_minutes`
- [ ] **Step 2:** Run `cargo test monitor`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test` and `cargo build`. Commit "Classify traffic into flows; save every 2 min off the lock".

### Task 6: Aggregation and cache (pure)

**Files:**
- Modify: `src/ui/traffic_data.rs`

**Interfaces:**
- Consumes:
  - `store::{Data, Bucket, Flow, OTHER_HOSTS}` (iterate flows with `Bucket::flows_of`)
  - `monitor::{LiveSample, OTHER_KEY, other_name}`
  - `ports::{label, UNCLASSIFIED, UNATTRIBUTED}`
  - `geo::{name, LAN, UNKNOWN}`
  - `net::is_private`
- Produces:
  - `#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]` on each of `pub enum Group { Apps, Publishers, Types, Countries }`, `pub enum Net { All, Lan, Wan }` and `pub enum DirF { Both, In, Out }`. Each has `from_u8`/`to_u8`; out-of-range values map to the first variant.
  - `pub fn effective(group: Group, net: Net, admin: bool) -> (Group, Net)`: not admin gives `(Apps, All)`.
  - `#[derive(Clone, PartialEq, Eq, Hash, Debug)] pub enum Sel { All, App(String), Group(Group, String) }`
  - `#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub enum Span { Preset(u8) /*0=5m,1=3h,2=24h,3=7d,4=30d*/, Day(i64 /*local midnight, unix*/), Week(i64 /*Monday midnight*/), Month(i64 /*1st midnight*/), AllTime, Custom(i64, i64) /*[from, to) local midnights*/ }`
  - `pub fn span_window(span: Span, now: i64, data: &Data) -> (i64 /*start*/, i64 /*end*/, i64 /*step*/, Source /*Live|Minutes|Hours*/)`
  - `pub fn span_step(span: Span, dir: i8, data: &Data) -> Span` moves Day/Week/Month to the previous or next period that has stored data. It never goes past the current period.
  - `#[derive(Clone, PartialEq, Eq, Hash, Debug)] pub struct Query { pub span: Span, pub group: Group, pub net: Net, pub dir: DirF, pub sel: Sel, pub admin: bool, pub end_offset: usize /*steps back from now; 0 = live; only for Preset spans*/ }`
  - `#[derive(Clone, Debug, Default)] pub struct Row { pub key: String, pub label: String, pub down: u64, pub up: u64, pub cc: Option<String>, pub icon_key: Option<String>, pub apps: Vec<String> /*Hosts rows: up to 3 app keys by bytes*/, pub more_apps: usize }`
  - `#[derive(Clone, Debug, Default)] pub struct Agg { pub times: Vec<i64>, pub step: i64, pub down: Vec<f64>, pub up: Vec<f64>, pub step_top: Vec<Vec<(String, [u64;2])>>, pub rows: Vec<Row>, pub hosts: Vec<Row>, pub types: Vec<Row>, pub countries: Vec<Row>, pub total: [u64;2], pub lan: [u64;2], pub wan: [u64;2], pub rate: [u64;2] }`
  - `pub fn aggregate(data: &Data, live: &VecDeque<LiveSample>, q: &Query, now: i64) -> Agg`
  - `pub struct AggCache`, with `pub fn get(&mut self, key: CacheKey, f: impl FnOnce() -> Agg) -> Arc<Agg>`. `CacheKey = (Query, i64, u64)`:
    - range 0: `(q, newest live t, 0)`;
    - other ranges: `(q, newest bucket t, newest bucket down+up)`. That way 7d and 30d don't re-aggregate every second.
- **Rules:**
  - **Spans.** Presets reuse today's window and step logic from `traffic.rs::series` (:122-220); Task 7 removes it from there.
    - 5m: every column, row and total comes from live samples (live flows for the newest 300).
    - 3h and 24h: minute buckets.
    - Last 7 Days and Last 30 Days: hour buckets.
    - Day, Week, Month, All Time and Custom: minute buckets if the window is 24 hours or less and fully inside minute retention, otherwise hour buckets. The window is clipped to the stored data, and a window with no data gives an empty `Agg`.
    - Local midnights come from chrono `Local`.
  - **Residual.** For each bucket and app, `apps[k] − Σflows_of(k)` (saturating) is treated as type `UNCLASSIFIED`, country `UNCLASSIFIED` and host `UNCLASSIFIED`, all labelled "Not classified". This covers old buckets, limited-mode buckets and the upgrade bucket alike.
  - **`__other`.**
    - Apps and Publishers modes: its own row (`OTHER_KEY`, label `other_name(true)`).
    - Types and Countries modes: spread through its flows (`unattributed`, `ZZ`).
  - **Publishers.** The key is `AppRecord.publisher`, or "Unknown publisher". `icon_key` is the app with the most bytes in the group.
  - **LAN/WAN.**
    - `Agg.lan` and `Agg.wan` with `Sel::All` come from the bucket's `lan_down`/`lan_up`, which is exact for every bucket. With a selection, flows with `cc == "LAN"` count as LAN and everything else (including the residual) counts as WAN.
    - `Agg.lan` and `Agg.wan` **ignore** the Net filter, so a dimmed strip row keeps its value. They do follow selection and direction.
  - **Net filter on rows and columns.** Only bytes of the chosen kind are counted. With `Sel::All`, the bucket-level LAN or WAN residual (bucket LAN minus Σ LAN flows, or the WAN equivalent) becomes a "Not classified" row, so rows still reconcile with `total`.
  - **Direction.** It zeroes the excluded side in rows, columns, totals, `rate` and `step_top`. The graph skips the excluded series (Task 7).
  - **Hosts column.**
    - `Sel::All` reads the `hosts` maps, plus a final "Other traffic (no host)" row = `total − Σhosts` (saturating).
    - A selection reads flows (`h==""` becomes "Other traffic (no host)"), plus the residual as "Not classified".
    - Rows are split by country. One row per (host, cc), with key `"{host}|{cc}"` and `cc = Some(cc)`. Rows that come only from the `hosts` maps have `cc = None` (the UI draws a chip).
    - `apps`: up to 3 app keys by bytes, from flows, and `more_apps` counts the rest. Rows that come only from the `hosts` maps have no apps.
    - Old IP hosts under the Net filter: an IP that passes `is_private` counts as LAN.
  - **`rate`.** It comes from the newest live sample under the same selection, Net and Dir rules, for every range.
  - **`step_top`.** The top 4 row keys per step for the active group.
  - **`end_offset`.** The window ends `end_offset` steps before now. For 5m a step is one live sample.
    - `Agg` also gets `pub history: Vec<(i64, f64)>`: the whole stored history for the range unit, as (t, bytes), for the scrubber strip.
    - It also gets `pub max_offset: usize`.
    - `CacheKey` includes `end_offset`.

- [ ] **Step 1: Write the failing tests** (fixture: `let mut d = Data::default(); d.minutes.push(...)`; don't use struct-literal syntax for `Data`):
  - `apps_mode_matches_today`
  - `types_rows_sum_to_total_with_old_history`: an old bucket and a new bucket give Σrows == total and a `__unclassified` row equal to the old bytes (Review Focus 1).
  - `countries_old_history_is_not_classified`: the old bytes go under `__unclassified`, not `ZZ`.
  - `columns_reconcile`: on a mixed old/new fixture, Σhosts == Σtypes == Σcountries == total, for both `Sel::All` and `Sel::App("a")`.
  - `publishers_group_and_other_row`
  - `group_selection_filters_series_and_columns`:
    - `Sel::Group(Countries,"IL")`: series, total, hosts and types contain only IL bytes.
    - `Sel::Group(Publishers,"P")`: only P's apps.
    - `Sel::Group(Types, UNCLASSIFIED)`: only the residual.
  - `net_filter_rows_reconcile`: under `Net::Lan` and `Net::Wan`, on a mixed bucket, Σrows == total, and `Agg.lan`/`Agg.wan` are unchanged by the filter.
  - `direction_in_zeroes_up`: covers rows, total, rate and step_top.
  - `rate_follows_selection_and_dir`
  - `hosts_split_by_country`: host "x" with 10 B in IT and 90 B in IL gives two rows with keys `x|IT` and `x|IL`.
  - `host_apps_top3`: a host used by 5 apps gives `apps.len()==3` and `more_apps==2`, ordered by bytes.
  - `span_day_uses_hours_when_old`: a Day 3 days back reads hour buckets, giving 24 steps.
  - `span_step_skips_to_data`: from today, stepping Day back lands on the newest earlier day that has buckets, and stepping forward from today stays on today.
  - `custom_span_clipped`: a Custom range reaching before the oldest bucket starts at the oldest bucket.
  - `end_offset_shifts_window`: with `end_offset=2` on 3h, the window excludes the newest 2 minute buckets, and `max_offset` equals the stored minutes minus the window length.
  - `effective_limited`, and `limited_direction_works_other_present`: `admin=false` with `DirF::Out` works.
  - `cache_reuses_until_key_changes`
  - `#[ignore] aggregate_30d_heavy_under_budget`: in release, `aggregate` on Task 4's heavy fixture for 30d takes under 10 ms. Reuse that fixture builder through a `#[cfg(test)] pub fn heavy_fixture()` in store.rs.
- [ ] **Step 2:** Run `cargo test traffic_data`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test`, `cargo test --release -- --ignored aggregate_30d` and `cargo build`. Commit "Traffic aggregation with groups, filters and cache".

### Task 10: Activity read state, Mark all read, Clear log in Settings, credits

**Files:**
- Modify:
  - `src/ui/log.rs`: :42-51 marks every event read on every frame; :45 and :166 hold the `fresh` set; :77 is Clear log.
  - `src/ui/settings.rs`

**Interfaces:**
- Produces:
  - `pub fn dot_shown(e: &Event) -> bool { !e.read && e.kind.important() }`
  - `pub fn unread_important<'a>(events: impl IntoIterator<Item = &'a Event>) -> usize`
- Behaviour:
  - Delete the per-frame mark-read loop and the `fresh` snapshot. Dots use `dot_shown`.
  - A row click sets that event's `read = true` and marks the store dirty. Opening the event's app link does the same.
  - **Mark all read** sets every event read. It is disabled when `unread_important == 0`.
  - Firewall events are not `important()`, so they never show a dot; no change is needed to how they are created.
- Settings:
  - **"Clear activity log":** a two-step confirm, following the existing "Reset usage history" pattern. The text warns that it also removes graph markers and the app popup's Alerts history.
  - **Footer credits:** "IP Geolocation by DB-IP" (a hyperlink to https://db-ip.com, CC BY 4.0) · "Flags: flag-icons (MIT)" · "Font: Manrope (OFL)".

- [ ] **Step 1: Write the failing tests:**
  - `dot_hidden_for_unread_firewall`, and `dot_shown_for_unread_new_app`
  - `unread_counts_important_only`
  - `mark_all_read_sets_all`
  - `click_marks_one`: `mark_read(&mut events, id)` changes only that event.
- [ ] **Step 2:** Run `cargo test log`. Expected: FAIL.
- [ ] **Step 3:** Implement. The reviewer checks that no code path in `show()` sets `read` other than a click or Mark all read.
- [ ] **Step 4:** Run `cargo test` and `cargo build`. Commit "Activity: mark all read; clear log moves to Settings; credits".

## Wave C: sequential (all touch traffic.rs)

### Task 7: Traffic list: menus, group rows, selection, strip

**Files:**
- Modify:
  - `src/ui/traffic.rs`
  - `src/ui/theme.rs`: the menu helper, plus the category glyphs
  - `src/ui/mod.rs`: the Activity badge uses `log::unread_important`

**Interfaces:**
- Consumes:
  - `traffic_data::*`, `flags::{draw, SIZE}`, `ports::category`, `sort::*`, `log::unread_important`
  - the `Settings` fields `traffic_group`, `traffic_net` and `traffic_dir`
- Produces:
  - `theme::menu(ui: &mut Ui, id: &str, label: &str, items: &[&str], selected: usize, disabled: &[bool], disabled_tip: &str) -> Option<usize>`. It is a compact header button that opens an egui popup. The current item is marked with a painted check mark; don't use `Glyph::Check`, so that variant stays unused, as today. Disabled items are greyed out and show `disabled_tip`. It returns the picked index.
  - New `Glyph` variants `TypeWeb`, `TypeNames`, `TypeFiles`, `TypeMail`, `TypeRemote`, `TypeMedia`, `TypeGames`, `TypeSystem` and `TypeOther`, drawn on the 20×20 grid.
  - `pub fn cat_glyph(c: ports::Cat) -> Glyph`
- Behaviour:
  - **Header row** (left panel `min(470, 40%)`, about 414 px at the 1100 px minimum): `[Group ▾ ~110px] [Network ▾ ~100px] [⇅ ▾ ~44px]` plus the search icon.
    - Group items: "Apps", "Publishers", "Traffic type", "Countries".
    - Network items: "WAN & LAN", "LAN", "WAN".
    - Direction items: "Incoming & Outgoing", "Incoming only", "Outgoing only".
    - Picking an item writes the setting and marks the store dirty.
    - Admin-only items are disabled when not admin ("Needs administrator mode"). `effective()` decides which values show.
  - **Selection.** `selected` becomes `Sel`. Changing Group by resets it to `Sel::All`.
  - **Pinned row.** "All traffic".
  - **Rows per mode:**
    - Apps: rows, "Connected now" and the popup link as today.
    - Publishers: the `icon_key` app's icon.
    - Types: `cat_glyph`.
    - Countries: `flags::draw` at `SIZE`.
    - The `__other` row is hidden in limited mode.
  - **Sort keys.** `traffic`, `traffic.publisher`, `traffic.type` and `traffic.country`. The natural order is total descending.
  - **Direction filter.** Only one number column shows.
  - **Data source.** `Series` and `series()` are replaced by the cached `Agg` (via `AggCache`). Every section reads the same `Arc<Agg>`. The graph tooltip uses `step_top` and skips the series excluded by Direction.
  - **Time menu.** It replaces `segmented_text(RANGE_CODES)`.
    - Items, using `theme::menu`, with divider support added to it: 5 Minutes, 3 Hours, 24 Hours, Last 7 Days, Last 30 Days | Day 📅, Week 📅, Month 📅, All Time | Custom. Draw the calendar icon as a new `Glyph::Calendar`.
    - The button label is the current span, e.g. "24 Hours ▾" or "Day ▾".
    - Day, Week and Month show a `◀ label ▶` stepper next to the menu, using `span_step`. Custom shows `From ◀ d ▶ To ◀ d ▶` day steppers.
    - Changing the span resets `end_offset` to 0 and clears pause.
    - Extend `theme::menu` with an `items: &[MenuItem]` form, where `MenuItem { label, icon: Option<Glyph>, divider_before: bool, disabled: bool }`. Keep a thin wrapper for the simple string-slice form.
  - **Connected now shows speeds.** "Connected now" rows show each app's current ↓/↑ speed (from `apps_live` rate, through `theme::rate`) in the two number columns, not "N conns". The connection count goes in the row tooltip. In limited mode the cells show "—".
  - **Strip:**
    - A bell-plus icon above the gauge total, with the tooltip "Set a data alert". Clicking it pushes a new `Action::OpenSettings` that switches to the Settings page; add the variant and its handling in `mod.rs`.
    - ↓/↑ totals come from `Agg.total` and rates from `Agg.rate`. The excluded direction is dimmed and shows "—".
    - The Internet/Local rows come from `Agg.wan`/`Agg.lan`. Under a Net filter the excluded row is dimmed but keeps its value.
    - The gauge is unchanged.

- [ ] **Step 1: Write the failing tests:**
  - `group_change_resets_selection`
  - `admin_only_items`
  - `sort_list_key_per_mode`
  - `strip_cells_dim_excluded`: with `DirF::In`, ↑ is dimmed.
  - `cat_glyph_covers_all`
  - `span_label`: `Preset(2)` gives "24 Hours", `Day(..)` gives "Day", and `AllTime` gives "All Time".
  - `connected_cells_show_rates`: a pure `connected_cells(rate, admin)` gives formatted rates when admin and `["—","—"]` when limited.
- [ ] **Step 2:** Run `cargo test traffic`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test` and `cargo build`. Commit "Traffic list: group by, network and direction filters".

### Task 8: Usage view, chart modes, CSV export

**Files:**
- Modify:
  - `src/ui/usage_view.rs`
  - `src/ui/csv_export.rs`
  - `src/ui/traffic.rs`: the modes and the export button
  - `src/ui/mod.rs`: `Action::ExportUsage`
  - `src/ui/theme.rs`: `Glyph::Export` and `Glyph::Columns`
- `Win32_UI_Shell` and `Win32_System_Com` are already enabled, so `SHGetKnownFolderPath(FOLDERID_Downloads)` and `CoTaskMemFree` are available.

**Interfaces:**
- `csv_export`:
  - `#[derive(Clone, Debug)] pub struct ExportRow { pub column: &'static str, pub name: String, pub country_code: String, pub down: u64, pub up: u64 }`
  - `pub fn to_csv(rows: &[ExportRow]) -> String`:
    - It starts with a BOM, then the header `column,name,country_code,download_bytes,upload_bytes,total_bytes`.
    - Fields follow RFC 4180.
    - A leading `=`, `+`, `-`, `@`, tab or CR gets a `'` prefix.
    - Numbers are raw integers.
  - `pub fn file_name(range_code, sel, net, dir, stamp: &str) -> String`: Windows-illegal characters are stripped.
- `usage_view`:
  - `pub fn show(ui: &mut Ui, agg: &Agg, s: &mut Shared, actions: &mut Vec<Action>, admin: bool)`
  - Three equal columns separated by 1 px `c::border()` lines.
  - Each column has its own scroll and `sort::header` (keys `usage.hosts`, `usage.types`, `usage.countries`; the default and natural order is total descending).
  - Rows: a flag, chip or glyph at `flags::SIZE`; a truncated name with a tooltip; the total; and a thin bar split by direction, scaled to the column's largest row.
  - Hosts rows end with up to 3 app icons (`Row.apps`, drawn with `Icons::draw` at 16 px), plus "+N" when `more_apps > 0`.
  - Each column shows its top 20 rows, then a "+N more" row with `Glyph::Grid` and the remaining bytes. Clicking it expands that column. `expanded: [bool; 3]` resets when the range or selection changes.
  - Pure helper: `fn visible_rows(rows: &[Row], expanded: bool) -> (Vec<&Row>, Option<(usize, u64)>)`.
  - Under the Countries column: the "IP Geolocation by DB-IP" hyperlink.
  - Limited mode: the view opens and shows the administrator note and the restart button, and the Usage button stays clickable.
- Export:
  - The button is an icon (`Glyph::Export`) with the tooltip "Export as CSV". It sits in the chart header, left of the clock and range codes.
  - Export rows follow the on-screen order.
  - `Action::ExportUsage(Vec<ExportRow>, String)` is handled in `run_actions` after the guard drops:
    1. Resolve Downloads (and free it with `CoTaskMemFree`).
    2. Write the file.
    3. Show a toast.
    4. Reveal the file with `Command::new("explorer").raw_arg(format!("/select,\"{}\"", path))`.
- Modes: `[(Graph,"Graph"), (Columns,"Usage"), (Bars,"Bars")]`.
  - The `hosts()` mode is removed.
  - `bars()` is fed from `Agg`.
  - The mode isn't persisted, so no migration is needed.

- [ ] **Step 1: Write the failing tests:**
  - `csv_neutralises_each_leading_char`: table-driven over `=`, `+`, `-`, `@`, `\t` and `\r`.
  - `csv_quotes_comma_newline_and_quote`: a name with a `"` comes out doubled and quoted.
  - `csv_starts_with_bom_header`
  - `csv_raw_integers_and_total`
  - `file_name_strips_illegal`
  - `visible_rows_collapses_tail`: 25 rows, collapsed, gives 20 rows plus `Some((5, sum_of_last_5))`. Expanded, it gives 25 rows and `None`.
- [ ] **Step 2:** Run `cargo test csv`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test` and `cargo build`. Commit "Usage view with hosts, types and countries; CSV export".

### Task 9: Graph markers, scale menu, pause

**Files:**
- Modify:
  - `src/ui/markers.rs`
  - `src/ui/traffic.rs`: `graph()`
  - `src/ui/theme.rs`: `Glyph::Pause` and `Glyph::Play`

**Interfaces:**
- `markers`:
  - `pub struct Cluster { pub x: f32, pub events: Vec<u64> }`
  - `pub fn clusters(events: &[(i64, u64)], t0: i64, t1: i64, x0: f32, width: f32, min_gap: f32) -> Vec<Cluster>`. It drops events outside `[t0, t1]` and merges events closer than `min_gap` (14.0).
  - `pub fn pick_events<'a>(events: impl IntoIterator<Item = &'a Event>, sel: &Sel, publisher_of: impl Fn(&str) -> Option<String>) -> Vec<&'a Event>`. Only `important()` events are kept, then by selection:
    - `Sel::App(OTHER_KEY)`, a type, a country, or `All`: all of them.
    - `Sel::App(k)`: that app's events.
    - A publisher: events of that publisher's apps.
- `traffic.rs`:
  - **Marker lane.** 18 px under the plot; the plot bottom moves up 18 px, leaving about 306 px of plot in limited mode at 1100×680.
    - Each cluster draws a clock glyph, plus a count badge when it holds more than one event.
    - The hover tooltip lists up to 5 events, with time and message.
    - On 5m, markers move with the glided rect.
  - **Scale.**
    - `const SCALE_PRESETS: [f64; 7] = [0.0, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9]` (bytes/s; 0 means Auto).
    - A `theme::menu` sits at the top-left of the plot, labelled via `theme::rate`.
    - The choice is saved in the matching `graph_scale_*` field.
    - When data exceeds a fixed scale, the series is clipped and the note "▲ clipped" shows.
  - **Timeline scrubber.**
    - A 26 px strip under the graph and Bars views shows a faint area of `Agg.history`, time labels, and a draggable handle at the window end.
    - Dragging sets `end_offset` (from 0 to `Agg.max_offset`).
    - When `end_offset > 0`, a "Now" chip appears, and clicking it sets 0. The glide stops, and the list, Usage view and strip all use the offset `Query`.
    - Changing the span sets `end_offset = 0`. The scrubber shows only for Preset spans.
    - The chart height budget at 1100×680 in limited mode is about 306 px minus 26 px, so about 280 px.
    - Pure helper: `fn offset_from_x(x: f32, strip: Rect, max_offset: usize, window: usize) -> usize`.
  - **Pause.**
    - An icon button (`Pause`/`Play`) sits at the top-right of the plot.
    - `paused: Option<Arc<Agg>>` is a snapshot used for the series, markers and axis. The list, Usage view and strip stay live.
    - A "Paused" chip shows.
    - A change of `Query` clears `paused`.
- Pure helpers:
  - `fn next_pause(paused: Option<Arc<Agg>>, old: &Query, new: &Query) -> Option<Arc<Agg>>`
  - `fn glide_repaint(paused: bool, last_sample_age_s: f64) -> Option<Duration>`. It returns `None` while paused or when the newest sample is older than 2 s (for example after the window was hidden), otherwise `Some(33 ms)`. The glide uses it for `request_repaint_after`.

- [ ] **Step 1: Write the failing tests:**
  - `clusters_merge_close_and_drop_outside`
  - `pick_events_by_selection`: Firewall is excluded; also covers `Sel::App("a")`, `Sel::App(OTHER_KEY)`, `Sel::Group(Publishers,"P")` and `Sel::Group(Types,..)`.
  - `scale_preset_lookup`: index 0 gives `None`, and index 3 gives 1e6.
  - `pause_cleared_on_query_change`
  - `glide_no_repaint_when_paused_or_stale` (Review Focus 5)
  - `offset_from_x_clamps`: x at the right edge gives 0, x at the left edge gives `max_offset`, and anything outside the strip is clamped.
- [ ] **Step 2:** Run `cargo test markers` and `cargo test traffic`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test` and `cargo build`. Commit "Graph event markers, scale menu and pause".

### Task 11: Cleanup, README, final verification

**Files:**
- Modify:
  - `README.md`: a "Third-party data" section covering DB-IP (CC BY 4.0, release 2026-10, unmodified, with link), flag-icons (MIT) and Manrope (OFL).
  - Any module that still has a temporary `allow(dead_code)`.

- [ ] **Step 1:** Remove the temporary `#![allow(dead_code)]` lines and delete anything that turns out to be unused.
- [ ] **Step 2:** Run `cargo test`, `cargo build` (only the 3 pre-existing warnings), `cargo build --release` and `cargo test --release -- --ignored`.
- [ ] **Step 3:** Run `git diff classic-graphite -- src/ui/firewall.rs`: no Action or state lines may have changed. Run `git diff classic-graphite -- src/wfp.rs src/system.rs`: it must be empty.
- [ ] **Step 4:** Commit "Traffic Insights: cleanup and third-party notices". Then give the user this visual checklist:
  - the three header menus at 1100×680, including disabled items in limited mode;
  - group rows with icons, glyphs and flags;
  - the Usage view's three columns, with flags or chips and the DB-IP link;
  - CSV export: the file in Downloads, opened in Excel;
  - the marker lane and its hover tooltip;
  - the scale menu and the "clipped" note;
  - pause, the Paused chip, and resuming when the range changes;
  - the strip's dimmed cells;
  - Mark all read, and dots only on important events;
  - Settings: Clear activity log and the credits line;
  - light mode for all of the above.
