//! Persistent state, saved as JSON under %APPDATA%\Vantage.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const MINUTE: i64 = 60;
pub const HOUR: i64 = 3600;
const MAX_MINUTES: usize = 24 * 60;
const MAX_HOURS: usize = 32 * 24;
const MAX_EVENTS: usize = 2000;
const MAX_APP_HOSTS: usize = 64;

/// Writers of `data.json` take turns on this lock.
static SAVE_LOCK: Mutex<()> = Mutex::new(());
/// Generation of each snapshot, in the order snapshots are taken.
static SAVE_GEN: AtomicU64 = AtomicU64::new(0);
/// Newest generation written to each data directory. Only touched while `SAVE_LOCK` is held.
static WRITTEN_GEN: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

/// Theme to show for a setting; `System` follows the OS and falls back to dark when it is unknown.
pub fn resolve(a: Appearance, system: Option<eframe::egui::Theme>) -> eframe::egui::Theme {
    use eframe::egui::Theme;
    match a {
        Appearance::Dark => Theme::Dark,
        Appearance::Light => Theme::Light,
        Appearance::System => system.unwrap_or(Theme::Dark),
    }
}

/// Window the data plan limit applies to.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlanPeriod {
    #[default]
    Day,
    Week,
    Month,
}

/// Which direction counts toward the data plan.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlanTraffic {
    #[default]
    Total,
    Download,
    Upload,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FirewallMode {
    /// Everything is allowed until you block it.
    #[default]
    ClickToBlock,
    /// Apps connecting for the first time are blocked until you allow them.
    AskToConnect,
    /// All outgoing traffic is blocked (loopback excepted).
    BlockAll,
}

/// Which column a list is sorted by and in which direction. Lives here (not in `ui::sort`) so the
/// store does not depend on the UI; `ui::sort` re-exports it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SortSpec {
    pub col: u8,
    pub desc: bool,
}

/// Saved block rules: app key -> (block incoming, block outgoing).
pub type Profile = BTreeMap<String, (bool, bool)>;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub notifications: bool,
    pub notify_new_apps: bool,
    pub notify_devices: bool,
    pub notify_internet: bool,
    pub notify_versions: bool,
    pub daily_cap_mb: u64,
    pub close_to_tray: bool,
    pub close_on_block: bool,
    pub firewall_enabled: bool,
    pub auto_scan: bool,
    pub appearance: Appearance,
    pub scan_minutes: u32,
    /// Also log known devices rejoining and devices leaving the network.
    pub notify_device_presence: bool,
    pub monitor_arp: bool,
    pub monitor_dns: bool,
    pub monitor_proxy: bool,
    pub monitor_rdp: bool,
    pub plan_period: PlanPeriod,
    pub plan_traffic: PlanTraffic,
    pub plan_exclude_lan: bool,
    pub speed_bits: bool,
    pub dns_lookup: bool,
    pub run_on_startup: bool,
    /// Desktop notifications are muted until this time.
    pub snooze_until: Option<i64>,
    pub firewall_mode: FirewallMode,
    /// Per-list sort order, keyed "traffic", "firewall", "scanner" or "hardware".
    pub sorts: BTreeMap<String, SortSpec>,
    /// Traffic view "Group by": 0 Apps, 1 Publishers, 2 Traffic types, 3 Countries.
    pub traffic_group: u8,
    /// Traffic view network filter: 0 WAN & LAN, 1 LAN only, 2 WAN only.
    pub traffic_net: u8,
    /// Traffic view direction filter: 0 both, 1 incoming only, 2 outgoing only.
    pub traffic_dir: u8,
    /// Graph scale per range: 0 is Auto, otherwise a preset index.
    pub graph_scale_5m: u8,
    pub graph_scale_3h: u8,
    pub graph_scale_24h: u8,
    pub graph_scale_7d: u8,
    pub graph_scale_30d: u8,
}

impl Settings {
    pub fn sort_for(&self, list: &str, default: SortSpec) -> SortSpec {
        self.sorts.get(list).copied().unwrap_or(default)
    }

    pub fn set_sort(&mut self, list: &str, spec: SortSpec) {
        self.sorts.insert(list.to_string(), spec);
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notifications: true,
            notify_new_apps: true,
            notify_devices: true,
            notify_internet: true,
            notify_versions: false,
            daily_cap_mb: 0,
            close_to_tray: true,
            close_on_block: false,
            firewall_enabled: true,
            auto_scan: true,
            appearance: Appearance::System,
            scan_minutes: 5,
            notify_device_presence: false,
            monitor_arp: true,
            monitor_dns: true,
            monitor_proxy: true,
            monitor_rdp: true,
            plan_period: PlanPeriod::Day,
            plan_traffic: PlanTraffic::Total,
            plan_exclude_lan: false,
            speed_bits: false,
            dns_lookup: true,
            run_on_startup: false,
            snooze_until: None,
            firewall_mode: FirewallMode::ClickToBlock,
            sorts: BTreeMap::new(),
            traffic_group: 0,
            traffic_net: 0,
            traffic_dir: 0,
            graph_scale_5m: 0,
            graph_scale_3h: 0,
            graph_scale_24h: 0,
            graph_scale_7d: 0,
            graph_scale_30d: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AppRecord {
    pub name: String,
    pub path: Option<String>,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub first_net: Option<i64>,
    pub down: u64,
    pub up: u64,
    pub block_in: bool,
    pub block_out: bool,
    pub hosts: Vec<String>,
}

impl AppRecord {
    pub fn note_host(&mut self, host: &str) {
        if let Some(i) = self.hosts.iter().position(|h| h == host) {
            if i == 0 {
                return;
            }
            self.hosts.remove(i);
        }
        self.hosts.insert(0, host.to_string());
        self.hosts.truncate(MAX_APP_HOSTS);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Bucket {
    pub t: i64,
    pub down: u64,
    pub up: u64,
    pub lan_down: u64,
    pub lan_up: u64,
    pub apps: HashMap<String, [u64; 2]>,
    /// Bytes per host label. New buckets keep the top `*_HOST_CAP` and merge the rest into
    /// `OTHER_HOSTS`, so older builds still read every host total they used to.
    pub hosts: HashMap<String, [u64; 2]>,
    /// This bucket's string table: host labels, type ids and country codes used by `flows`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strs: Vec<String>,
    /// Per app, `[host_idx, type_idx, country_idx, down, up]` with indices into `strs`.
    /// Absent in old files, limited-mode buckets and the bucket open at upgrade time.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub flows: HashMap<String, Vec<[u64; 5]>>,
}

/// One app's bytes to one host, of one traffic type, in one country.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Flow {
    /// Host label (PTR name or IP text); `""` for bytes no connection accounts for.
    pub h: String,
    /// A `ports` id, `other_tcp` or `unattributed`.
    pub ty: String,
    /// ISO-2 code, `LAN` or `ZZ`.
    pub cc: String,
    pub d: u64,
    pub u: u64,
}

/// Flow caps, per app and bucket. Stored shape: each bucket has a string table `strs` and, per
/// app, entries `[host_idx, type_idx, country_idx, down, up]`.
///
/// A capped bucket keeps, for each app, the N hosts with the most bytes (summed over type and
/// country) and merges the rest into one `OTHER_HOSTS` entry per (type, country). The
/// unattributed host `""` is never merged. So an app keeps at most N named hosts plus
/// |types × countries present| other entries (a named host seen under several types or
/// countries keeps one entry for each). N = 8 per minute and 16 per hour, the spec's starting
/// values. The heavy-profile budget is 25 MB (`heavy_fixture`, about 5x the real data, measures
/// about 20.6 MB with these values; the profile is 8.2 MB with no flows at all), enforced by
/// the `heavy_profile_under_budget` test.
///
/// Closed buckets are capped when they roll over; the open bucket is capped early once an app
/// passes 4 × N entries or `hosts` passes 4 × its host cap.
pub const MINUTE_FLOW_CAP: usize = 8;
pub const HOUR_FLOW_CAP: usize = 16;
/// Global `hosts` map caps: the top 32 per minute bucket and 64 per hour bucket are kept,
/// the rest merge into `OTHER_HOSTS`.
pub const MINUTE_HOST_CAP: usize = 32;
pub const HOUR_HOST_CAP: usize = 64;
/// Host label that capped hosts are merged into, in both `flows` and `hosts`.
pub const OTHER_HOSTS: &str = "(other hosts)";

impl Bucket {
    /// Adds one flow for `app`, merging with an existing entry of the same (host, type, country).
    pub fn add_flow(&mut self, app: &str, f: &Flow) {
        if f.d == 0 && f.u == 0 {
            return;
        }
        let key = [self.intern(&f.h), self.intern(&f.ty), self.intern(&f.cc)];
        let list = match self.flows.get_mut(app) {
            Some(l) => l,
            None => self.flows.entry(app.to_string()).or_default(),
        };
        match list.iter_mut().find(|e| e[..3] == key) {
            Some(e) => {
                e[3] = e[3].saturating_add(f.d);
                e[4] = e[4].saturating_add(f.u);
            }
            None => list.push([key[0], key[1], key[2], f.d, f.u]),
        }
    }

    /// Every flow as (app, flow). Entries whose indices fall outside `strs` (a damaged file)
    /// are skipped, so their bytes show up as the app's unclassified residual instead.
    #[allow(dead_code)] // consumed by Task 6; removed in Task 11
    pub fn flows_of(&self) -> impl Iterator<Item = (&str, Flow)> + '_ {
        let s = |i: u64| usize::try_from(i).ok().and_then(|i| self.strs.get(i));
        self.flows.iter().flat_map(move |(app, list)| {
            list.iter().filter_map(move |e| {
                let (h, ty, cc) = (s(e[0])?, s(e[1])?, s(e[2])?);
                Some((app.as_str(), Flow { h: h.clone(), ty: ty.clone(), cc: cc.clone(), d: e[3], u: e[4] }))
            })
        })
    }

    fn intern(&mut self, s: &str) -> u64 {
        match self.strs.iter().position(|x| x == s) {
            Some(i) => i as u64,
            None => {
                self.strs.push(s.to_string());
                (self.strs.len() - 1) as u64
            }
        }
    }
}

/// Caps a bucket's flows (per app) and its `hosts` map, merging the rest into `OTHER_HOSTS`.
/// Bytes are conserved. See `MINUTE_FLOW_CAP` for the rules.
fn cap_bucket(b: &mut Bucket, flow_cap: usize, host_cap: usize) {
    let other = b.intern(OTHER_HOSTS);
    for list in b.flows.values_mut() {
        cap_flows(list, flow_cap, other, &b.strs);
    }
    cap_hosts(&mut b.hosts, host_cap);
    reintern(&mut b.strs, &mut b.flows);
}

fn cap_flows(list: &mut Vec<[u64; 5]>, cap: usize, other: u64, strs: &[String]) {
    let name = |i: u64| usize::try_from(i).ok().and_then(|i| strs.get(i)).map(String::as_str);
    // The unattributed host and earlier merges always stay; they don't take a slot.
    let pinned = |i: u64| matches!(name(i), Some(s) if s.is_empty() || s == OTHER_HOSTS);
    let mut totals: Vec<(u64, u64)> = Vec::new();
    for e in list.iter().filter(|e| !pinned(e[0])) {
        let bytes = e[3].saturating_add(e[4]);
        match totals.iter_mut().find(|t| t.0 == e[0]) {
            Some(t) => t.1 = t.1.saturating_add(bytes),
            None => totals.push((e[0], bytes)),
        }
    }
    if totals.len() <= cap {
        return;
    }
    totals.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| name(a.0).cmp(&name(b.0))));
    let keep: Vec<u64> = totals[..cap].iter().map(|t| t.0).collect();
    let (mut out, rest): (Vec<[u64; 5]>, Vec<[u64; 5]>) = list.drain(..).partition(|e| pinned(e[0]) || keep.contains(&e[0]));
    for e in rest {
        match out.iter_mut().find(|o| o[0] == other && o[1] == e[1] && o[2] == e[2]) {
            Some(o) => {
                o[3] = o[3].saturating_add(e[3]);
                o[4] = o[4].saturating_add(e[4]);
            }
            None => out.push([other, e[1], e[2], e[3], e[4]]),
        }
    }
    *list = out;
}

fn cap_hosts(hosts: &mut HashMap<String, [u64; 2]>, cap: usize) {
    let named = hosts.len() - usize::from(hosts.contains_key(OTHER_HOSTS));
    if named <= cap {
        return;
    }
    let mut ranked: Vec<(&String, u64)> = hosts.iter().filter(|(k, _)| k.as_str() != OTHER_HOSTS).map(|(k, v)| (k, v[0].saturating_add(v[1]))).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let merged: Vec<String> = ranked[cap..].iter().map(|(k, _)| (*k).clone()).collect();
    let mut sum = [0u64; 2];
    for k in merged {
        if let Some(v) = hosts.remove(&k) {
            sum = [sum[0].saturating_add(v[0]), sum[1].saturating_add(v[1])];
        }
    }
    let e = hosts.entry(OTHER_HOSTS.to_string()).or_default();
    *e = [e[0].saturating_add(sum[0]), e[1].saturating_add(sum[1])];
}

/// Rebuilds `strs` with only the strings `flows` still uses (each once), remapping the entries
/// and merging any that now share a key. Entries with out-of-range indices are dropped.
fn reintern(strs: &mut Vec<String>, flows: &mut HashMap<String, Vec<[u64; 5]>>) {
    let old = std::mem::take(strs);
    let mut remap: Vec<Option<u64>> = vec![None; old.len()];
    let mut lists: Vec<(&String, &mut Vec<[u64; 5]>)> = flows.iter_mut().collect();
    lists.sort_by(|a, b| a.0.cmp(b.0));
    for (_, list) in lists {
        let mut out: Vec<[u64; 5]> = Vec::with_capacity(list.len());
        'entry: for e in list.iter() {
            let mut key = [0u64; 3];
            for (k, slot) in key.iter_mut().enumerate() {
                let Some(i) = usize::try_from(e[k]).ok().filter(|&i| i < old.len()) else { continue 'entry };
                *slot = *remap[i].get_or_insert_with(|| match strs.iter().position(|x| *x == old[i]) {
                    Some(p) => p as u64,
                    None => {
                        strs.push(old[i].clone());
                        (strs.len() - 1) as u64
                    }
                });
            }
            match out.iter_mut().find(|o| o[..3] == key) {
                Some(o) => {
                    o[3] = o[3].saturating_add(e[3]);
                    o[4] = o[4].saturating_add(e[4]);
                }
                None => out.push([key[0], key[1], key[2], e[3], e[4]]),
            }
        }
        *list = out;
    }
    flows.retain(|_, l| !l.is_empty());
}

/// Full retention (1440 minute and 768 hour buckets) of a heavy machine, every bucket capped:
/// - minute buckets: 8 apps, 40 app-host pairs, one browser with 25 hosts;
/// - hour buckets: 25 apps, 250 app-host pairs, one browser with 120 hosts, 6 types, 12 countries;
/// - every host label is distinct within its bucket and 32 characters long; app keys are
///   45-character paths;
/// - the browser's hosts are https (every fifth http) across all 12 countries; every other app
///   uses one type and spreads its hosts over countries;
/// - each bucket also has an `__other` app with unattributed bytes, and `apps`/`hosts` filled;
/// - plus 2000 events and app records that each remember 64 hosts.
#[cfg(test)]
pub fn heavy_fixture() -> Data {
    heavy_fixture_with(MINUTE_FLOW_CAP, HOUR_FLOW_CAP)
}

#[cfg(test)]
fn heavy_fixture_with(minute_flow_cap: usize, hour_flow_cap: usize) -> Data {
    const TYPES: [&str; 6] = ["https", "http", "ssh", "imaps", "rdp", "other_tcp"];
    const CCS: [&str; 12] = ["US", "IL", "DE", "GB", "NL", "FR", "IE", "JP", "SG", "CA", "SE", "LAN"];
    let app_key = |a: usize| format!(r"c:\program files\vendor{a:02}\suite\bin\app{a:02}.exe");
    let host = |a: usize, i: usize| format!("n{i:03}-a{a:02}.edge-cdn.example-co.net");
    // `per_app[a]` hosts for app a; host i of an app gets about `big / (i + 1)` bytes down.
    let build = |per_app: &[usize], big: u64, flow_cap: usize, host_cap: usize| {
        let mut b = Bucket::default();
        for (a, &n) in per_app.iter().enumerate() {
            let key = app_key(a);
            for i in 0..n {
                let d = big / (i as u64 + 1) + (a * 7 + i) as u64;
                let u = d / 9;
                let (ty, cc) = match a {
                    0 => (if i % 5 == 4 { "http" } else { "https" }, CCS[i % CCS.len()]),
                    _ => (TYPES[a % TYPES.len()], CCS[(a + i) % CCS.len()]),
                };
                let f = Flow { h: host(a, i), ty: ty.into(), cc: cc.into(), d, u };
                b.add_flow(&key, &f);
                let e = b.apps.entry(key.clone()).or_default();
                *e = [e[0] + d, e[1] + u];
                b.hosts.insert(f.h.clone(), [d, u]);
                b.down += d;
                b.up += u;
                if cc == "LAN" {
                    b.lan_down += d;
                    b.lan_up += u;
                }
            }
        }
        let other = Flow { h: String::new(), ty: "unattributed".into(), cc: "ZZ".into(), d: big / 3, u: big / 30 };
        b.add_flow("__other", &other);
        b.apps.insert("__other".into(), [other.d, other.u]);
        b.down += other.d;
        b.up += other.u;
        cap_bucket(&mut b, flow_cap, host_cap);
        b
    };
    // minute: browser 25 + 7 apps with 15 pairs; hour: browser 120 + 24 apps with 130 pairs
    let minute_apps: Vec<usize> = std::iter::once(25).chain((0..7).map(|a| if a == 0 { 3 } else { 2 })).collect();
    let hour_apps: Vec<usize> = std::iter::once(120).chain((0..24).map(|a| if a < 10 { 6 } else { 5 })).collect();
    assert_eq!((minute_apps.iter().sum::<usize>(), hour_apps.iter().sum::<usize>()), (40, 250));
    assert_eq!(host(0, 0).len(), 32);
    let minute = build(&minute_apps, 10_000_000, minute_flow_cap, MINUTE_HOST_CAP);
    let hour = build(&hour_apps, 500_000_000, hour_flow_cap, HOUR_HOST_CAP);

    let start = 1_789_999_200 - 32 * 24 * HOUR;
    let mut d = Data { created: start, ..Default::default() };
    for i in 0..MAX_HOURS {
        d.hours.push_back(Bucket { t: start + i as i64 * HOUR, ..hour.clone() });
    }
    let minutes_from = start + (MAX_HOURS as i64 - 24) * HOUR;
    for i in 0..MAX_MINUTES {
        d.minutes.push_back(Bucket { t: minutes_from + i as i64 * MINUTE, ..minute.clone() });
    }
    for a in 0..hour_apps.len() {
        let mut rec = AppRecord { name: format!("Application {a:02}"), path: Some(app_key(a)), first_seen: start, last_seen: start, ..Default::default() };
        for i in 0..MAX_APP_HOSTS {
            rec.note_host(&host(a, i));
        }
        d.apps.insert(app_key(a), rec);
    }
    for i in 0..MAX_EVENTS {
        d.next_event += 1;
        d.events.push_back(Event { id: d.next_event, t: start + i as i64, kind: EventKind::NewApp, app: Some(app_key(i % 25)), subject: host(i % 25, i % 120), read: false });
    }
    for day in 0..32 {
        d.daily.insert(format!("2026-09-{:02}", day % 30 + 1), [9_000_000_000, 900_000_000]);
    }
    d
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DeviceRecord {
    pub mac: String,
    pub ip: String,
    pub name: Option<String>,
    pub vendor: Option<String>,
    /// Name the user gave the device; shown instead of the discovered name.
    pub label: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    NewApp,
    InternetUp,
    InternetDown,
    VersionChanged,
    NewDevice,
    Firewall,
    DataCap,
    DeviceLeft,
    DeviceJoined,
    ArpSpoof,
    DnsChanged,
    ProxyChanged,
    Rdp,
}

impl EventKind {
    pub fn source(self) -> &'static str {
        match self {
            Self::NewApp => "First network activity",
            Self::InternetUp | Self::InternetDown => "Internet access monitor",
            Self::VersionChanged => "Application info monitor",
            Self::NewDevice => "Network scanner",
            Self::Firewall => "Firewall",
            Self::DataCap => "Data usage monitor",
            Self::DeviceLeft | Self::DeviceJoined => "Device list monitor",
            Self::ArpSpoof => "ARP spoofing detection",
            Self::DnsChanged => "DNS server settings monitor",
            Self::ProxyChanged => "Proxy settings monitor",
            Self::Rdp => "RDP detection",
        }
    }
    pub fn important(self) -> bool {
        !matches!(self, Self::Firewall | Self::DeviceLeft | Self::DeviceJoined)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Event {
    pub id: u64,
    pub t: i64,
    pub kind: EventKind,
    /// App key, when the event is about an app.
    pub app: Option<String>,
    /// Kind-specific subject: host, device name, version "a → b", ...
    pub subject: String,
    #[serde(default)]
    pub read: bool,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Data {
    pub created: i64,
    pub settings: Settings,
    pub apps: HashMap<String, AppRecord>,
    pub devices: HashMap<String, DeviceRecord>,
    pub events: VecDeque<Event>,
    pub minutes: VecDeque<Bucket>,
    pub hours: VecDeque<Bucket>,
    pub daily: BTreeMap<String, [u64; 2]>,
    /// LAN share of `daily`, so the data plan can exclude local traffic.
    pub daily_lan: BTreeMap<String, [u64; 2]>,
    pub cap_alerted: Option<String>,
    pub last_scan: Option<i64>,
    /// Last seen (gateway ip, gateway mac), for ARP spoofing detection.
    pub gateway_mac: Option<(String, String)>,
    pub dns_servers: Option<Vec<String>>,
    pub proxy: Option<String>,
    pub profiles: BTreeMap<String, Profile>,
    pub active_profile: Option<String>,
    next_event: u64,
}

pub struct Store {
    pub data: Data,
    pub dir: PathBuf,
    pub fresh: bool,
    dirty: bool,
}

impl Store {
    pub fn load() -> Self {
        let dir = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Vantage");
        let _ = std::fs::create_dir_all(&dir);
        crate::diag::init(&dir);
        let (data, fresh) = match std::fs::read(dir.join("data.json")).ok().and_then(|b| serde_json::from_slice::<Data>(&b).ok()) {
            Some(d) => (d, false),
            None => (Data { created: now(), ..Default::default() }, true),
        };
        Self { data, dir, fresh, dirty: false }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Saves now, on the calling thread. Prefer `snapshot` under the shared lock and
    /// `write_snapshot` after releasing it.
    pub fn save(&mut self) {
        if let Some((dir, gen, data)) = self.snapshot() {
            Self::write_snapshot(&dir, gen, &data);
        }
    }

    /// A copy of the data to save, with its generation, or `None` when nothing changed.
    /// Clears the dirty flag.
    pub fn snapshot(&mut self) -> Option<(PathBuf, u64, Data)> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let gen = SAVE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
        Some((self.dir.clone(), gen, self.data.clone()))
    }

    /// Writes a snapshot to `dir\data.json` through `data.json.tmp`. Writers take turns, and a
    /// snapshot older than one already written to `dir` is dropped, so a slow background save
    /// can never replace a newer exit or hide save, and two writers never share the temp file.
    pub fn write_snapshot(dir: &Path, gen: u64, data: &Data) {
        let _turn = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut written = WRITTEN_GEN.lock().unwrap_or_else(|e| e.into_inner());
        if written.get(dir).is_some_and(|&g| g >= gen) {
            return;
        }
        let Ok(bytes) = serde_json::to_vec(data) else { return };
        let tmp = dir.join("data.json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, dir.join("data.json")).is_ok() {
            written.insert(dir.to_path_buf(), gen);
        }
    }

    pub fn add_event(&mut self, kind: EventKind, app: Option<String>, subject: String) -> Event {
        self.data.next_event += 1;
        let e = Event { id: self.data.next_event, t: now(), kind, app, subject, read: false };
        self.data.events.push_front(e.clone());
        self.data.events.truncate(MAX_EVENTS);
        self.dirty = true;
        e
    }

    /// Records one tick of traffic. Returns today's (down, up) totals.
    pub fn add_usage(&mut self, t: i64, totals: [u64; 4], apps: &HashMap<String, [u64; 2]>, hosts: &HashMap<String, [u64; 2]>, flows: &HashMap<String, Vec<Flow>>) -> [u64; 2] {
        /// The open bucket for `t`; a bucket that closes is capped first.
        fn bucket(list: &mut VecDeque<Bucket>, t: i64, size: i64, max: usize, flow_cap: usize, host_cap: usize) -> &mut Bucket {
            let start = t - t.rem_euclid(size);
            if list.back().map(|b| b.t) != Some(start) {
                if let Some(closed) = list.back_mut() {
                    cap_bucket(closed, flow_cap, host_cap);
                }
                list.push_back(Bucket { t: start, ..Default::default() });
                while list.len() > max {
                    list.pop_front();
                }
            }
            list.back_mut().unwrap()
        }
        let lists = [
            (&mut self.data.minutes, MINUTE, MAX_MINUTES, MINUTE_FLOW_CAP, MINUTE_HOST_CAP),
            (&mut self.data.hours, HOUR, MAX_HOURS, HOUR_FLOW_CAP, HOUR_HOST_CAP),
        ];
        for (list, size, max, flow_cap, host_cap) in lists {
            let b = bucket(list, t, size, max, flow_cap, host_cap);
            b.down += totals[0];
            b.up += totals[1];
            b.lan_down += totals[2];
            b.lan_up += totals[3];
            for (k, v) in apps {
                let e = b.apps.entry(k.clone()).or_default();
                e[0] += v[0];
                e[1] += v[1];
            }
            for (k, v) in hosts {
                let e = b.hosts.entry(k.clone()).or_default();
                e[0] += v[0];
                e[1] += v[1];
            }
            for (app, list) in flows {
                for f in list {
                    b.add_flow(app, f);
                }
            }
            if b.flows.values().any(|l| l.len() > 4 * flow_cap) || b.hosts.len() > 4 * host_cap {
                cap_bucket(b, flow_cap, host_cap);
            }
        }
        for (k, v) in apps {
            if let Some(a) = self.data.apps.get_mut(k) {
                a.down += v[0];
                a.up += v[1];
            }
        }
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        let l = self.data.daily_lan.entry(day.clone()).or_default();
        l[0] += totals[2];
        l[1] += totals[3];
        let d = self.data.daily.entry(day).or_default();
        d[0] += totals[0];
        d[1] += totals[1];
        self.dirty = true;
        *d
    }

    /// Usage counted toward the data plan in the current period, and the period's label
    /// (used to alert once per period).
    pub fn plan_usage(&self) -> (u64, String) {
        use chrono::Datelike;
        let st = &self.data.settings;
        let today = chrono::Local::now().date_naive();
        let (start, label) = match st.plan_period {
            PlanPeriod::Day => (today, today.format("%Y-%m-%d").to_string()),
            PlanPeriod::Week => {
                let s = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
                (s, format!("week {}", s.format("%Y-%m-%d")))
            }
            PlanPeriod::Month => (today.with_day(1).unwrap_or(today), today.format("month %Y-%m").to_string()),
        };
        let from = start.format("%Y-%m-%d").to_string();
        let pick = |v: &[u64; 2]| match st.plan_traffic {
            PlanTraffic::Total => v[0] + v[1],
            PlanTraffic::Download => v[0],
            PlanTraffic::Upload => v[1],
        };
        let mut used: u64 = self.data.daily.range(from.clone()..).map(|(_, v)| pick(v)).sum();
        if st.plan_exclude_lan {
            let lan: u64 = self.data.daily_lan.range(from..).map(|(_, v)| pick(v)).sum();
            used = used.saturating_sub(lan);
        }
        (used, label)
    }

    pub fn reset_usage(&mut self) {
        self.data.minutes.clear();
        self.data.hours.clear();
        self.data.daily.clear();
        self.data.daily_lan.clear();
        for a in self.data.apps.values_mut() {
            a.down = 0;
            a.up = 0;
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;
    use eframe::egui::Theme;

    #[test]
    fn old_settings_json_loads() {
        let s: Settings = serde_json::from_str(r#"{"notifications":false,"daily_cap_mb":500}"#).unwrap();
        assert_eq!(s.appearance, Appearance::System);
        assert!(!s.notifications);
        assert_eq!(s.daily_cap_mb, 500);
        assert!(s.close_to_tray);
    }

    #[test]
    fn old_settings_without_sorts_load() {
        let s: Settings = serde_json::from_str(r#"{"daily_cap_mb":500}"#).unwrap();
        assert!(s.sorts.is_empty());
        assert_eq!(s.daily_cap_mb, 500);
        let d = SortSpec { col: 3, desc: true };
        assert_eq!(s.sort_for("firewall", d), d);
    }

    #[test]
    fn sorts_round_trip() {
        let mut s = Settings::default();
        let spec = SortSpec { col: 4, desc: false };
        s.set_sort("scanner", spec);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.sort_for("scanner", SortSpec { col: 0, desc: true }), spec);
    }

    #[test]
    fn plan_usage_counts_direction_and_excludes_lan() {
        let mut st = Store { data: Data::default(), dir: PathBuf::new(), fresh: true, dirty: false };
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        st.data.daily.insert(today.clone(), [1000, 200]);
        st.data.daily_lan.insert(today.clone(), [300, 50]);
        st.data.daily.insert("2000-01-01".into(), [9999, 9999]); // outside any current period
        assert_eq!(st.plan_usage().0, 1200);
        st.data.settings.plan_traffic = PlanTraffic::Download;
        assert_eq!(st.plan_usage().0, 1000);
        st.data.settings.plan_exclude_lan = true;
        assert_eq!(st.plan_usage().0, 700);
        st.data.settings.plan_period = PlanPeriod::Month;
        assert!(st.plan_usage().1.starts_with("month "));
    }

    #[test]
    fn resolve_cases() {
        for sys in [Some(Theme::Light), Some(Theme::Dark), None] {
            assert_eq!(resolve(Appearance::Dark, sys), Theme::Dark);
            assert_eq!(resolve(Appearance::Light, sys), Theme::Light);
        }
        assert_eq!(resolve(Appearance::System, Some(Theme::Light)), Theme::Light);
        assert_eq!(resolve(Appearance::System, Some(Theme::Dark)), Theme::Dark);
        assert_eq!(resolve(Appearance::System, None), Theme::Dark);
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;

    /// Start of an hour, so minute and hour buckets begin at the same time.
    const T0: i64 = 1_789_999_200;

    fn store() -> Store {
        Store { data: Data::default(), dir: PathBuf::new(), fresh: true, dirty: false }
    }

    fn flow(h: &str, ty: &str, cc: &str, d: u64, u: u64) -> Flow {
        Flow { h: h.into(), ty: ty.into(), cc: cc.into(), d, u }
    }

    fn sorted(mut v: Vec<(String, Flow)>) -> Vec<(String, Flow)> {
        v.sort_by(|a, b| (&a.0, &a.1.h, &a.1.ty, &a.1.cc).cmp(&(&b.0, &b.1.h, &b.1.ty, &b.1.cc)));
        v
    }

    fn all_flows(b: &Bucket) -> Vec<(String, Flow)> {
        sorted(b.flows_of().map(|(a, f)| (a.to_string(), f)).collect())
    }

    /// Sum of (down, up) over a bucket's flows.
    fn flow_sum(b: &Bucket) -> [u64; 2] {
        b.flows_of().fold([0, 0], |s, (_, f)| [s[0] + f.d, s[1] + f.u])
    }

    fn host_sum(b: &Bucket) -> [u64; 2] {
        b.hosts.values().fold([0, 0], |s, v| [s[0] + v[0], s[1] + v[1]])
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("vantage-test-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One tick's maps for `add_usage`: app "a" gets `flows`; `hosts` get the given (down, up).
    fn tick(flows: Vec<Flow>, hosts: &[(String, [u64; 2])]) -> (HashMap<String, [u64; 2]>, HashMap<String, [u64; 2]>, HashMap<String, Vec<Flow>>) {
        let total = flows.iter().fold([0, 0], |s, f| [s[0] + f.d, s[1] + f.u]);
        let apps = HashMap::from([("a".to_string(), total)]);
        let hosts = hosts.iter().cloned().collect();
        (apps, hosts, HashMap::from([("a".to_string(), flows)]))
    }

    fn named_hosts(n: usize) -> Vec<(String, [u64; 2])> {
        (0..n).map(|i| (format!("host-{i:03}.example.net"), [(i as u64 + 1) * 100, i as u64 + 1])).collect()
    }

    #[test]
    fn flows_merge_by_key() {
        let mut st = store();
        for t in [T0, T0 + 1] {
            let (apps, hosts, flows) = tick(vec![flow("x.example.net", "https", "US", 10, 1)], &[]);
            st.add_usage(t, [10, 1, 0, 0], &apps, &hosts, &flows);
        }
        let want = vec![("a".to_string(), flow("x.example.net", "https", "US", 20, 2))];
        assert_eq!(all_flows(&st.data.minutes[0]), want);
        assert_eq!(all_flows(&st.data.hours[0]), want);
        assert_eq!(st.data.minutes[0].flows["a"].len(), 1);
    }

    #[test]
    fn compact_roundtrip() {
        let mut b = Bucket { t: T0, ..Default::default() };
        let input = vec![
            ("a", flow("x.example.net", "https", "US", 10, 1)),
            ("a", flow("y.example.net", "https", "IL", 20, 2)),
            ("a", flow("", "unattributed", "ZZ", 5, 5)),
            ("b", flow("x.example.net", "ssh", "LAN", 7, 3)),
        ];
        for (app, f) in &input {
            b.add_flow(app, f);
        }
        // Each distinct string is stored once: 2 hosts, "", 3 types, 4 countries.
        assert_eq!(b.strs.len(), 10);
        let json = serde_json::to_string(&b).unwrap();
        let back: Bucket = serde_json::from_str(&json).unwrap();
        let want = sorted(input.into_iter().map(|(a, f)| (a.to_string(), f)).collect());
        assert_eq!(all_flows(&back), want);
    }

    #[test]
    fn cap_keeps_top_hosts() {
        let mut b = Bucket::default();
        let tys = ["https", "ssh"];
        let ccs = ["US", "IL"];
        // host i has (i+1)*100 down; hosts 0..4 are the smallest and span all four (ty, cc) pairs
        for i in 0..12u64 {
            let f = flow(&format!("h{i:02}"), tys[(i % 2) as usize], ccs[((i / 2) % 2) as usize], (i + 1) * 100, i + 1);
            b.add_flow("a", &f);
        }
        let before = flow_sum(&b);
        cap_bucket(&mut b, 8, 32);
        let after = all_flows(&b);
        let named: Vec<&str> = after.iter().map(|(_, f)| f.h.as_str()).filter(|h| *h != OTHER_HOSTS).collect();
        let want: Vec<String> = (4..12).map(|i| format!("h{i:02}")).collect();
        assert_eq!(named, want);
        let mut others: Vec<(String, String)> = after.iter().filter(|(_, f)| f.h == OTHER_HOSTS).map(|(_, f)| (f.ty.clone(), f.cc.clone())).collect();
        others.sort();
        others.dedup();
        assert_eq!(others.len(), 4, "one {OTHER_HOSTS} entry per (type, country)");
        assert_eq!(after.len(), 12);
        assert_eq!(flow_sum(&b), before);
        assert!(!b.strs.iter().any(|s| s == "h00"), "merged host names leave the string table");
    }

    #[test]
    fn cap_keeps_unattributed() {
        let mut b = Bucket::default();
        b.add_flow("a", &flow("", "unattributed", "ZZ", 1, 1));
        for i in 0..12u64 {
            b.add_flow("a", &flow(&format!("h{i:02}"), "https", "US", 1000 + i, 10));
        }
        let before = flow_sum(&b);
        cap_bucket(&mut b, 8, 32);
        let after = all_flows(&b);
        assert!(after.contains(&("a".to_string(), flow("", "unattributed", "ZZ", 1, 1))));
        assert_eq!(after.len(), 8 + 1 + 1);
        assert_eq!(flow_sum(&b), before);
    }

    #[test]
    fn cap_hosts_merges_rest() {
        let mut b = Bucket::default();
        b.hosts = named_hosts(40).into_iter().collect();
        let before = host_sum(&b);
        cap_bucket(&mut b, 8, 32);
        assert_eq!(b.hosts.len(), 33);
        assert!(b.hosts.contains_key(OTHER_HOSTS));
        for (h, _) in named_hosts(40).into_iter().skip(8) {
            assert!(b.hosts.contains_key(&h), "{h} is in the top 32");
        }
        assert_eq!(host_sum(&b), before);
        // capping again changes nothing
        let again = b.hosts.clone();
        cap_bucket(&mut b, 8, 32);
        assert_eq!(b.hosts, again);
    }

    #[test]
    fn rollover_caps_closed_bucket_minute_and_hour() {
        let mut st = store();
        let fl: Vec<Flow> = (0..20u64).map(|i| flow(&format!("f{i:02}.example.net"), "https", "US", 100 + i, 1)).collect();
        let (apps, hosts, flows) = tick(fl, &named_hosts(70));
        st.add_usage(T0, [0; 4], &apps, &hosts, &flows);
        let minute_bytes = flow_sum(&st.data.minutes[0]);
        let minute_hosts = host_sum(&st.data.minutes[0]);
        assert_eq!(st.data.minutes[0].flows["a"].len(), 20, "open bucket below the soft cap is not capped");

        // next minute: the first minute bucket closes and is capped; the hour stays open
        let (apps, hosts, flows) = tick(vec![flow("late.example.net", "https", "US", 1, 1)], &[]);
        st.add_usage(T0 + MINUTE, [0; 4], &apps, &hosts, &flows);
        let closed = &st.data.minutes[0];
        assert_eq!(closed.flows["a"].len(), MINUTE_FLOW_CAP + 1);
        assert_eq!(closed.hosts.len(), MINUTE_HOST_CAP + 1);
        assert_eq!(flow_sum(closed), minute_bytes);
        assert_eq!(host_sum(closed), minute_hosts);
        assert_eq!(st.data.hours[0].flows["a"].len(), 21);
        assert_eq!(st.data.hours[0].hosts.len(), 70);
        let hour_bytes = flow_sum(&st.data.hours[0]);
        let hour_hosts = host_sum(&st.data.hours[0]);

        // next hour: the hour bucket closes and is capped, and so is the second minute bucket
        let (apps, hosts, flows) = tick(vec![flow("next.example.net", "https", "US", 1, 1)], &[]);
        st.add_usage(T0 + HOUR, [0; 4], &apps, &hosts, &flows);
        let closed = &st.data.hours[0];
        assert_eq!(closed.flows["a"].len(), HOUR_FLOW_CAP + 1);
        assert_eq!(closed.hosts.len(), HOUR_HOST_CAP + 1);
        assert_eq!(flow_sum(closed), hour_bytes);
        assert_eq!(host_sum(closed), hour_hosts);
        assert_eq!(st.data.hours.len(), 2);
        assert_eq!(st.data.minutes.len(), 3);
    }

    #[test]
    fn open_bucket_soft_cap() {
        let push = |n: usize, hosts: usize| {
            let mut st = store();
            let fl: Vec<Flow> = (0..n as u64).map(|i| flow(&format!("f{i:03}.example.net"), "https", "US", 100 + i, 1)).collect();
            let (apps, hosts, flows) = tick(fl, &named_hosts(hosts));
            st.add_usage(T0, [0; 4], &apps, &hosts, &flows);
            assert_eq!((st.data.minutes.len(), st.data.hours.len()), (1, 1), "still the open buckets");
            st
        };
        let sum = |n: usize| [(0..n as u64).map(|i| 100 + i).sum::<u64>(), n as u64];

        // 5x the minute flow cap passes 4x it: the minute bucket is capped, the hour is not.
        let n = 5 * MINUTE_FLOW_CAP;
        let st = push(n, 0);
        let (m, h) = (&st.data.minutes[0], &st.data.hours[0]);
        assert_eq!(m.flows["a"].len(), MINUTE_FLOW_CAP + 1);
        assert_eq!(h.flows["a"].len(), n, "below 4x the hour flow cap: untouched");
        assert_eq!((flow_sum(m), flow_sum(h)), (sum(n), sum(n)));

        // 5x the hour flow cap: both are capped.
        let n = 5 * HOUR_FLOW_CAP;
        let st = push(n, 0);
        let (m, h) = (&st.data.minutes[0], &st.data.hours[0]);
        assert_eq!(m.flows["a"].len(), MINUTE_FLOW_CAP + 1);
        assert_eq!(h.flows["a"].len(), HOUR_FLOW_CAP + 1);
        assert_eq!((flow_sum(m), flow_sum(h)), (sum(n), sum(n)));

        // 5x the minute host cap: the minute hosts are capped, the hour hosts are not.
        let n = 5 * MINUTE_HOST_CAP;
        let st = push(0, n);
        let (m, h) = (&st.data.minutes[0], &st.data.hours[0]);
        assert_eq!(m.hosts.len(), MINUTE_HOST_CAP + 1);
        assert_eq!(h.hosts.len(), n, "below 4x the hour host cap: untouched");
        assert_eq!(host_sum(m), host_sum(h));
    }

    #[test]
    fn old_file_without_flows_loads() {
        let json = r#"{"created":5,"minutes":[{"t":60,"down":10,"up":5,"lan_down":1,"lan_up":0,
            "apps":{"a":[10,5]},"hosts":{"x.example.net":[9,4]}}],
            "hours":[{"t":0,"down":10,"up":5,"apps":{"a":[10,5]},"hosts":{"x.example.net":[9,4]}}]}"#;
        let d: Data = serde_json::from_str(json).unwrap();
        for b in [&d.minutes[0], &d.hours[0]] {
            assert_eq!(b.apps["a"], [10, 5]);
            assert_eq!(b.hosts["x.example.net"], [9, 4]);
            assert!(b.strs.is_empty());
            assert!(b.flows.is_empty());
            assert_eq!(b.flows_of().count(), 0);
        }
        assert_eq!(d.created, 5);
    }

    #[test]
    fn new_bucket_loads_in_older_build() {
        // The bucket shape older builds deserialise.
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct OldBucket {
            t: i64,
            apps: HashMap<String, [u64; 2]>,
            hosts: HashMap<String, [u64; 2]>,
        }
        let mut b = Bucket { t: T0, ..Default::default() };
        b.apps.insert("a".into(), [10, 1]);
        b.hosts.insert("x.example.net".into(), [10, 1]);
        b.add_flow("a", &flow("x.example.net", "https", "US", 10, 1));
        let old: OldBucket = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!((old.t, old.apps["a"], old.hosts["x.example.net"]), (T0, [10, 1], [10, 1]));
        // Buckets without flows stay as compact as before.
        let json = serde_json::to_string(&Bucket::default()).unwrap();
        assert!(!json.contains("flows") && !json.contains("strs"), "{json}");
    }

    #[test]
    fn settings_view_fields_default() {
        let s = Settings::default();
        let old: Settings = serde_json::from_str(r#"{"daily_cap_mb":500}"#).unwrap();
        for s in [&s, &old] {
            assert_eq!([s.traffic_group, s.traffic_net, s.traffic_dir], [0; 3]);
            assert_eq!([s.graph_scale_5m, s.graph_scale_3h, s.graph_scale_24h, s.graph_scale_7d, s.graph_scale_30d], [0; 5]);
        }
        let mut s = Settings::default();
        s.traffic_group = 3;
        s.graph_scale_7d = 4;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!((back.traffic_group, back.graph_scale_7d), (3, 4));
    }

    #[test]
    fn snapshot_only_when_dirty() {
        let mut st = store();
        assert!(st.snapshot().is_none());
        st.mark_dirty();
        let (_, g1, _) = st.snapshot().unwrap();
        assert!(st.snapshot().is_none(), "a snapshot clears dirty");
        st.add_event(EventKind::Rdp, None, "10.0.0.1".into());
        let (_, g2, data) = st.snapshot().unwrap();
        assert!(g2 > g1);
        assert_eq!(data.events.len(), 1);
    }

    #[test]
    fn concurrent_writes_never_corrupt() {
        let dir = temp_dir("concurrent");
        // Each generation's data has a different size, so an interleaved write could not parse.
        let data_for = |gen: u64| {
            let mut d = Data { created: gen as i64, ..Default::default() };
            for i in 0..(200 + gen as usize * 7 % 50) {
                let mut b = Bucket { t: i as i64 * MINUTE, down: gen, ..Default::default() };
                b.hosts = named_hosts(10).into_iter().collect();
                b.add_flow("a", &flow("x.example.net", "https", "US", gen, 1));
                d.minutes.push_back(b);
            }
            d
        };
        for round in 0..50u64 {
            let (old, new) = (2 * round + 1, 2 * round + 2);
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let handles: Vec<_> = [old, new]
                .into_iter()
                .map(|gen| {
                    let (dir, barrier, data) = (dir.clone(), barrier.clone(), data_for(gen));
                    std::thread::spawn(move || {
                        barrier.wait();
                        Store::write_snapshot(&dir, gen, &data);
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
            let bytes = std::fs::read(dir.join("data.json")).unwrap();
            let d: Data = serde_json::from_slice(&bytes).expect("data.json parses");
            assert_eq!(d.created, new as i64, "round {round}: the newest generation wins");
            assert_eq!(d.minutes.len(), data_for(new).minutes.len());
        }
        // An older generation arriving later never overwrites a newer one.
        Store::write_snapshot(&dir, 1, &data_for(1));
        let d: Data = serde_json::from_slice(&std::fs::read(dir.join("data.json")).unwrap()).unwrap();
        assert_eq!(d.created, 100);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_writes_through_snapshot() {
        let dir = temp_dir("save");
        let mut st = Store { data: Data { created: 42, ..Default::default() }, dir: dir.clone(), fresh: false, dirty: true };
        st.save();
        let d: Data = serde_json::from_slice(&std::fs::read(dir.join("data.json")).unwrap()).unwrap();
        assert_eq!(d.created, 42);
        assert!(!dir.join("data.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn heavy_profile_under_budget() {
        let d = heavy_fixture();
        assert_eq!(d.minutes.len(), MAX_MINUTES);
        assert_eq!(d.hours.len(), MAX_HOURS);
        let m = &d.minutes[0];
        let h = &d.hours[0];
        assert!(m.flows.len() >= 8 && h.flows.len() >= 25);
        assert!(m.hosts.len() <= MINUTE_HOST_CAP + 1 && h.hosts.len() <= HOUR_HOST_CAP + 1);
        let longest = |b: &Bucket| b.flows.values().map(Vec::len).max().unwrap_or(0);
        eprintln!("heavy profile: minute bucket max entries/app {}, hour bucket max entries/app {}", longest(m), longest(h));
        let size = serde_json::to_vec(&d).unwrap().len();
        eprintln!("heavy profile: data.json is {size} bytes");
        assert!(size < 25_000_000, "data.json is {size} bytes");
    }

    #[test]
    #[ignore]
    fn snapshot_clone_time() {
        let mut st = Store { data: heavy_fixture(), dir: PathBuf::new(), fresh: false, dirty: false };
        let mut times = Vec::new();
        for _ in 0..5 {
            st.mark_dirty();
            let start = std::time::Instant::now();
            let snap = st.snapshot();
            times.push(start.elapsed());
            assert!(snap.is_some());
        }
        eprintln!("snapshot clone times: {times:?}");
        let best = times.iter().min().unwrap();
        assert!(*best < std::time::Duration::from_millis(5), "cloning the heavy fixture took {best:?}");
    }
}
