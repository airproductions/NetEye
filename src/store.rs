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
        }
    }
    pub fn important(self) -> bool {
        !matches!(self, Self::Firewall)
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
    pub cap_alerted: Option<String>,
    pub last_scan: Option<i64>,
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
        let d = self.data.daily.entry(day).or_default();
        d[0] += totals[0];
        d[1] += totals[1];
        self.dirty = true;
        *d
    }

    pub fn reset_usage(&mut self) {
        self.data.minutes.clear();
        self.data.hours.clear();
        self.data.daily.clear();
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
