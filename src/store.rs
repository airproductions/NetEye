//! Persistent state, saved as JSON under %APPDATA%\Vantage.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const MINUTE: i64 = 60;
pub const HOUR: i64 = 3600;
const MAX_MINUTES: usize = 24 * 60;
const MAX_HOURS: usize = 32 * 24;
const MAX_EVENTS: usize = 2000;
const MAX_APP_HOSTS: usize = 64;

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
    pub hosts: HashMap<String, [u64; 2]>,
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

#[derive(Serialize, Deserialize, Default)]
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

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let tmp = self.dir.join("data.json.tmp");
        if let Ok(bytes) = serde_json::to_vec(&self.data) {
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, self.dir.join("data.json"));
            }
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
    pub fn add_usage(&mut self, t: i64, totals: [u64; 4], apps: &HashMap<String, [u64; 2]>, hosts: &HashMap<String, [u64; 2]>) -> [u64; 2] {
        fn bucket(list: &mut VecDeque<Bucket>, t: i64, size: i64, max: usize) -> &mut Bucket {
            let start = t - t.rem_euclid(size);
            if list.back().map(|b| b.t) != Some(start) {
                list.push_back(Bucket { t: start, ..Default::default() });
                while list.len() > max {
                    list.pop_front();
                }
            }
            list.back_mut().unwrap()
        }
        for b in [bucket(&mut self.data.minutes, t, MINUTE, MAX_MINUTES), bucket(&mut self.data.hours, t, HOUR, MAX_HOURS)] {
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
