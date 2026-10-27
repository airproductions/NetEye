//! The background engine. One thread samples connections, traffic and hardware every second
//! and publishes the results into `Shared`, which the UI reads.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::hw::{ProcSample, Sampler, SystemSample};
use crate::net::{self, Adapter, Conn, Proto, TcpState};
use crate::store::{self, AppRecord, DeviceRecord, EventKind, FirewallMode, Store};
use crate::system::{self, Dir};

pub const LIVE_SECONDS: usize = 3600;
pub const HW_SECONDS: usize = 300;
pub const SYSTEM_KEY: &str = "system";
pub const OTHER_KEY: &str = "__other";
const RDP_PORT: u16 = 3389;
/// How often the monitor writes `data.json` in the background.
pub const SAVE_EVERY: Duration = Duration::from_secs(120);
/// Live samples (newest first) that keep their per-flow detail; older ones keep totals only.
const LIVE_FLOW_SECONDS: usize = 300;
/// Distinct addresses the monitor remembers a country for before starting over.
const GEO_CACHE_CAP: usize = 4096;

/// Per app, bytes this tick keyed by (host, traffic type, country).
type FlowAcc = HashMap<String, HashMap<(String, String, String), [u64; 2]>>;

/// Adds one connection's bytes to its app's (host, type, country) entry.
fn accumulate_flow(per_flow: &mut FlowAcc, app: &str, host: &str, ty: &str, cc: &str, rate: [u64; 2]) {
    let e = per_flow.entry(app.to_string()).or_default().entry((host.to_string(), ty.to_string(), cc.to_string())).or_default();
    e[0] += rate[0];
    e[1] += rate[1];
}

/// The flow for bytes no connection accounts for. Only an administrator can attribute bytes to
/// connections at all, so limited mode records none.
fn residual_flow(other: [u64; 2], admin: bool) -> Option<store::Flow> {
    if !admin || other == [0, 0] {
        return None;
    }
    Some(store::Flow { h: String::new(), ty: crate::ports::UNATTRIBUTED.into(), cc: crate::geo::UNKNOWN.into(), d: other[0], u: other[1] })
}

/// Remembers the country of each remote address, so the database is read once per address.
struct GeoCache {
    map: HashMap<IpAddr, String>,
    cap: usize,
}

impl GeoCache {
    fn new() -> Self {
        GeoCache { map: HashMap::new(), cap: GEO_CACHE_CAP }
    }

    /// The cached code for `ip`, or `f(ip)` stored for next time. A full cache starts over.
    fn get_with(&mut self, ip: IpAddr, f: impl FnOnce(IpAddr) -> String) -> String {
        if let Some(cc) = self.map.get(&ip) {
            return cc.clone();
        }
        if self.map.len() >= self.cap {
            self.map.clear();
        }
        let cc = f(ip);
        self.map.insert(ip, cc.clone());
        cc
    }

    fn get(&mut self, ip: IpAddr) -> String {
        self.get_with(ip, crate::geo::country_of)
    }
}

/// Drops the flow detail of every live sample except the newest `keep`.
fn trim_live_flows(live: &mut VecDeque<LiveSample>, keep: usize) {
    let old = live.len().saturating_sub(keep);
    for s in live.iter_mut().take(old) {
        if !s.flows.is_empty() {
            s.flows = Vec::new();
        }
    }
}

/// Desktop notifications stay quiet until this unix time (Settings → Snooze).
static SNOOZE_UNTIL: AtomicI64 = AtomicI64::new(0);

#[derive(Clone, Debug)]
pub struct LiveSample {
    pub t: i64,
    pub down: u64,
    pub up: u64,
    pub apps: Vec<(String, [u64; 2])>,
    /// This second's flows per app; filled for the newest samples only (Task 5).
    #[allow(dead_code)] // consumed by Task 6; removed in Task 11
    pub flows: Vec<(String, store::Flow)>,
}

#[derive(Clone, Debug)]
pub struct ConnView {
    pub proto: Proto,
    pub state: TcpState,
    pub local: IpAddr,
    pub local_port: u16,
    pub remote: IpAddr,
    pub remote_port: u16,
    pub pid: u32,
    pub host: Option<String>,
    pub rate: [u64; 2],
    pub total: Option<[u64; 2]>,
    pub inbound: bool,
}

#[derive(Clone, Debug, Default)]
pub struct AppLive {
    pub conns: Vec<ConnView>,
    pub rate: [u64; 2],
    pub in_conns: usize,
    pub out_conns: usize,
}

#[derive(Clone, Debug, Default)]
pub struct AppMeta {
    pub name: String,
    pub path: Option<String>,
    pub publisher: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AppHw {
    pub total: ProcSample,
    pub procs: Vec<(u32, String, ProcSample)>,
}

#[derive(Default)]
pub struct Hardware {
    pub history: VecDeque<SystemSample>,
    pub apps: HashMap<String, AppHw>,
    pub app_history: HashMap<String, VecDeque<ProcSample>>,
}

pub type Icon = Option<(usize, usize, Vec<u8>)>;

pub struct Shared {
    pub admin: bool,
    pub store: Store,
    pub live: VecDeque<LiveSample>,
    pub apps_live: HashMap<String, AppLive>,
    pub meta: HashMap<String, AppMeta>,
    pub adapters: Vec<Adapter>,
    pub hw: Hardware,
    pub names: HashMap<IpAddr, String>,
    pub internet: Option<bool>,
    pub scanning: bool,
    pub online: HashSet<String>,
    pub icons: HashMap<String, Icon>,
    pub icon_requests: HashSet<String>,
    pub toast: Option<String>,
    pub firewall_busy: bool,
    pub firewall_product: Option<String>,
    pub gateway: Option<String>,
    pub local_ip: Option<String>,
    /// Ask to Connect: apps blocked on first connection, waiting for the user's answer.
    pub asks: Vec<String>,
}

impl Shared {
    pub fn app_name(&self, key: &str) -> String {
        if let Some(m) = self.meta.get(key) {
            return m.name.clone();
        }
        if let Some(a) = self.store.data.apps.get(key) {
            return a.name.clone();
        }
        match key {
            OTHER_KEY => other_name(self.admin).into(),
            _ => key.rsplit(['\\', ':']).next().unwrap_or(key).to_string(),
        }
    }

    pub fn host_label(&self, ip: &IpAddr) -> String {
        self.names.get(ip).cloned().unwrap_or_else(|| ip.to_string())
    }
}

pub fn other_name(admin: bool) -> &'static str {
    if admin { "Other traffic (UDP, ICMP, system)" } else { "All traffic" }
}

#[derive(Clone)]
pub struct Monitor {
    shared: Arc<Mutex<Shared>>,
    dns_tx: Sender<IpAddr>,
}

impl Monitor {
    pub fn start(repaint: impl Fn() + Send + 'static) -> Self {
        let store = Store::load();
        let shared = Arc::new(Mutex::new(Shared {
            admin: net::is_admin(),
            store,
            live: VecDeque::with_capacity(LIVE_SECONDS),
            apps_live: HashMap::new(),
            meta: HashMap::new(),
            adapters: Vec::new(),
            hw: Hardware::default(),
            names: HashMap::new(),
            internet: None,
            scanning: false,
            online: HashSet::new(),
            icons: HashMap::new(),
            icon_requests: HashSet::new(),
            toast: None,
            firewall_busy: false,
            firewall_product: None,
            gateway: None,
            local_ip: None,
            asks: Vec::new(),
        }));

        let (dns_tx, dns_rx) = channel::<IpAddr>();
        let dns_rx = Arc::new(Mutex::new(dns_rx));
        for _ in 0..4 {
            let (rx, sh) = (dns_rx.clone(), shared.clone());
            std::thread::spawn(move || loop {
                let ip = match rx.lock().unwrap().recv() {
                    Ok(ip) => ip,
                    Err(_) => return,
                };
                if let Some(name) = system::reverse_dns(ip) {
                    sh.lock().unwrap().names.insert(ip, name);
                }
            });
        }

        let m = Self { shared, dns_tx };
        let engine = m.clone();
        std::thread::Builder::new().name("monitor".into()).spawn(move || engine.run(repaint)).unwrap();

        let fw = m.clone();
        std::thread::spawn(move || fw.reconcile_firewall());
        let inet = m.clone();
        std::thread::spawn(move || inet.internet_loop());
        let scan = m.clone();
        std::thread::spawn(move || scan.scan_loop());
        let sec = m.clone();
        std::thread::spawn(move || sec.security_loop());
        m
    }

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Clones the store under the lock, then writes it after the lock is released.
    pub fn save(&self) {
        let snap = self.lock().store.snapshot();
        if let Some((dir, gen, data)) = snap {
            Store::write_snapshot(&dir, gen, &data);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Firewall

    fn reconcile_firewall(&self) {
        let product = system::firewall_product();
        crate::diag::log(&format!("system firewall product: {product:?}"));
        self.lock().firewall_product = product;
        if self.lock().admin {
            let removed = system::remove_legacy_rules();
            if removed > 0 {
                crate::diag::log(&format!("removed {removed} legacy Windows Firewall rules"));
            }
        }
        let (keys, paths): (Vec<String>, Vec<String>) = {
            let s = self.lock();
            if !s.store.data.settings.firewall_enabled {
                return;
            }
            s.store.data.apps.iter().filter_map(|(k, a)| a.path.clone().map(|p| (k.clone(), p))).unzip()
        };
        // Only trust WFP's answer when it could actually be queried (needs elevation).
        let Some(states) = crate::wfp::query(&paths) else { return };
        let mut s = self.lock();
        for (k, (block_in, block_out)) in keys.iter().zip(states) {
            if let Some(a) = s.store.data.apps.get_mut(k) {
                a.block_in = block_in;
                a.block_out = block_out;
            }
        }
        s.store.mark_dirty();
    }

    pub fn set_block(&self, key: &str, dir: Dir, block: bool) {
        let (path, admin, enabled, close_open) = {
            let s = self.lock();
            let Some(path) = s.store.data.apps.get(key).and_then(|a| a.path.clone()) else { return };
            if block && system::is_protected(&path) {
                return;
            }
            (path, s.admin, s.store.data.settings.firewall_enabled, s.store.data.settings.close_on_block)
        };
        let me = self.clone();
        let key = key.to_string();
        self.lock().firewall_busy = true;
        std::thread::spawn(move || {
            let res = if enabled { system::apply_firewall(&[(path, dir, block)], admin, close_open) } else { Ok(()) };
            let mut s = me.lock();
            s.firewall_busy = false;
            match res {
                Ok(()) => {
                    let name = s.app_name(&key);
                    if let Some(a) = s.store.data.apps.get_mut(&key) {
                        match dir {
                            Dir::In => a.block_in = block,
                            Dir::Out => a.block_out = block,
                        }
                    }
                    let verb = if block { "Blocked" } else { "Allowed" };
                    let d = if dir == Dir::In { "incoming" } else { "outgoing" };
                    s.store.add_event(EventKind::Firewall, Some(key), format!("{verb} {d} connections"));
                    s.toast = Some(format!("{verb} {d} connections for {name}"));
                }
                Err(e) => {
                    crate::diag::log(&format!("firewall change failed: {e}"));
                    s.toast = Some(format!("Firewall change failed: {e}"));
                }
            }
        });
    }

    pub fn set_firewall_enabled(&self, enabled: bool) {
        let (changes, admin, close_open) = {
            let s = self.lock();
            let mut c = Vec::new();
            for a in s.store.data.apps.values() {
                if let Some(p) = a.path.as_ref().filter(|p| !system::is_protected(p)) {
                    if a.block_in {
                        c.push((p.clone(), Dir::In, enabled));
                    }
                    if a.block_out {
                        c.push((p.clone(), Dir::Out, enabled));
                    }
                }
            }
            (c, s.admin, s.store.data.settings.close_on_block)
        };
        let me = self.clone();
        self.lock().firewall_busy = true;
        std::thread::spawn(move || {
            let res = system::apply_firewall(&changes, admin, close_open);
            let mut s = me.lock();
            s.firewall_busy = false;
            match res {
                Ok(()) => {
                    s.store.data.settings.firewall_enabled = enabled;
                    s.store.mark_dirty();
                    let msg = if enabled { "Firewall rules turned on" } else { "Firewall rules turned off" };
                    s.store.add_event(EventKind::Firewall, None, msg.into());
                    s.toast = Some(msg.into());
                }
                Err(e) => {
                    crate::diag::log(&format!("firewall change failed: {e}"));
                    s.toast = Some(format!("Firewall change failed: {e}"));
                }
            }
        });
    }

    /// Switches between Click to Block, Ask to Connect and Block All. Only Block All touches
    /// the filtering engine.
    pub fn set_firewall_mode(&self, mode: FirewallMode) {
        let (old, admin) = {
            let s = self.lock();
            (s.store.data.settings.firewall_mode, s.admin)
        };
        if old == mode {
            return;
        }
        let me = self.clone();
        self.lock().firewall_busy = true;
        std::thread::spawn(move || {
            let res = if (old == FirewallMode::BlockAll) != (mode == FirewallMode::BlockAll) {
                system::set_block_all(mode == FirewallMode::BlockAll, admin)
            } else {
                Ok(())
            };
            let mut s = me.lock();
            s.firewall_busy = false;
            match res {
                Ok(()) => {
                    s.store.data.settings.firewall_mode = mode;
                    s.store.mark_dirty();
                    let msg = match mode {
                        FirewallMode::ClickToBlock => "Firewall mode: Click to Block",
                        FirewallMode::AskToConnect => "Firewall mode: Ask to Connect",
                        FirewallMode::BlockAll => "Firewall mode: Block All (all outgoing traffic is blocked)",
                    };
                    s.store.add_event(EventKind::Firewall, None, msg.into());
                    s.toast = Some(msg.into());
                }
                Err(e) => {
                    crate::diag::log(&format!("firewall mode change failed: {e}"));
                    s.toast = Some(format!("Firewall change failed: {e}"));
                }
            }
        });
    }

    /// Applies a saved profile: every app's block state becomes what the profile recorded
    /// (apps the profile doesn't mention are allowed).
    pub fn apply_profile(&self, name: &str) {
        let (changes, admin, close_open, updates) = {
            let s = self.lock();
            let Some(profile) = s.store.data.profiles.get(name) else { return };
            let mut changes = Vec::new();
            let mut updates = Vec::new();
            for (k, a) in &s.store.data.apps {
                let (want_in, want_out) = profile.get(k).copied().unwrap_or((false, false));
                let Some(p) = a.path.as_ref().filter(|p| !system::is_protected(p)) else { continue };
                if a.block_in != want_in {
                    changes.push((p.clone(), Dir::In, want_in));
                }
                if a.block_out != want_out {
                    changes.push((p.clone(), Dir::Out, want_out));
                }
                updates.push((k.clone(), want_in, want_out));
            }
            if !s.store.data.settings.firewall_enabled {
                changes.clear(); // rules are recorded and applied when the firewall is turned on
            }
            (changes, s.admin, s.store.data.settings.close_on_block, updates)
        };
        let me = self.clone();
        let name = name.to_string();
        self.lock().firewall_busy = true;
        std::thread::spawn(move || {
            let res = system::apply_firewall(&changes, admin, close_open);
            let mut s = me.lock();
            s.firewall_busy = false;
            match res {
                Ok(()) => {
                    for (k, i, o) in updates {
                        if let Some(a) = s.store.data.apps.get_mut(&k) {
                            a.block_in = i;
                            a.block_out = o;
                        }
                    }
                    s.store.data.active_profile = Some(name.clone());
                    s.store.add_event(EventKind::Firewall, None, format!("Switched to firewall profile \"{name}\""));
                    s.toast = Some(format!("Firewall profile \"{name}\" is active"));
                }
                Err(e) => {
                    crate::diag::log(&format!("profile switch failed: {e}"));
                    s.toast = Some(format!("Firewall change failed: {e}"));
                }
            }
        });
    }

    /// Answers an Ask to Connect prompt.
    pub fn answer_ask(&self, key: &str, allow: bool) {
        self.lock().asks.retain(|k| k != key);
        if allow {
            self.set_block(key, Dir::Out, false);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Security monitors: DNS servers, proxy settings and the gateway's MAC (ARP spoofing)

    fn security_loop(&self) {
        let mut tick: u64 = 0;
        loop {
            let st = self.lock().store.data.settings.clone();
            if st.monitor_dns {
                let now = system::dns_servers();
                let mut s = self.lock();
                if !now.is_empty() && s.store.data.dns_servers.as_ref() != Some(&now) {
                    if s.store.data.dns_servers.is_some() {
                        let msg = format!("DNS servers changed to {}", now.join(", "));
                        s.store.add_event(EventKind::DnsChanged, None, now.join(", "));
                        if st.notifications {
                            notify(&msg);
                        }
                    }
                    s.store.data.dns_servers = Some(now);
                    s.store.mark_dirty();
                }
            }
            if st.monitor_proxy {
                let now = system::proxy_settings();
                let mut s = self.lock();
                if s.store.data.proxy.as_deref() != Some(now.as_str()) {
                    if s.store.data.proxy.is_some() {
                        s.store.add_event(EventKind::ProxyChanged, None, now.clone());
                        if st.notifications {
                            notify(&format!("Proxy settings changed: {now}"));
                        }
                    }
                    s.store.data.proxy = Some(now);
                    s.store.mark_dirty();
                }
            }
            if st.monitor_arp && tick % 3 == 0 {
                if let Some((gw, mac)) = system::lan().and_then(|l| Some((l.gateway?, system::arp(&l, l.gateway?)?))) {
                    self.check_gateway(&gw.to_string(), &mac);
                }
            }
            tick += 1;
            std::thread::sleep(Duration::from_secs(10));
        }
    }

    /// Records the gateway's MAC; a different MAC for the same gateway IP means another
    /// device may be answering for the router (ARP spoofing).
    fn check_gateway(&self, ip: &str, mac: &str) {
        let mut s = self.lock();
        let notify_on = s.store.data.settings.notifications && s.store.data.settings.monitor_arp;
        if let Some((old_ip, old_mac)) = s.store.data.gateway_mac.clone() {
            if old_ip == ip && old_mac != mac {
                s.store.add_event(EventKind::ArpSpoof, None, format!("{ip}: {old_mac} → {mac}"));
                if notify_on {
                    notify(&format!("Possible ARP spoofing: your router {ip} now answers from a different device ({mac})."));
                }
            }
        }
        if s.store.data.gateway_mac.as_ref().map(|(i, m)| (i.as_str(), m.as_str())) != Some((ip, mac)) {
            s.store.data.gateway_mac = Some((ip.to_string(), mac.to_string()));
            s.store.mark_dirty();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Internet + LAN scanning

    fn internet_loop(&self) {
        loop {
            let up = system::internet_reachable();
            let mut s = self.lock();
            let prev = s.internet;
            s.internet = Some(up);
            if prev.is_some() && prev != Some(up) {
                let kind = if up { EventKind::InternetUp } else { EventKind::InternetDown };
                s.store.add_event(kind, None, String::new());
                if s.store.data.settings.notifications && s.store.data.settings.notify_internet {
                    notify(if up { "Internet connection now available." } else { "Internet connection was lost." });
                }
            }
            drop(s);
            std::thread::sleep(Duration::from_secs(if up { 15 } else { 5 }));
        }
    }

    fn scan_loop(&self) {
        let oui = system::load_oui(&self.lock().store.dir);
        let mut first = true;
        loop {
            let (auto, minutes) = {
                let s = self.lock();
                (s.store.data.settings.auto_scan, s.store.data.settings.scan_minutes.max(1))
            };
            if first || auto {
                self.scan(&oui, first);
            }
            first = false;
            for _ in 0..minutes * 60 {
                std::thread::sleep(Duration::from_secs(1));
                if self.lock().scanning {
                    break; // manual scan requested
                }
            }
        }
    }

    pub fn request_scan(&self) {
        self.lock().scanning = true;
    }

    fn scan(&self, oui: &HashMap<String, String>, baseline: bool) {
        self.lock().scanning = true;
        let lan = system::lan();
        let gw = lan.as_ref().and_then(|l| l.gateway);
        let me_ip = lan.as_ref().map(|l| l.ip);
        let mut found = lan.as_ref().map(system::scan_subnet).unwrap_or_default();
        let found_gw = found.iter().find(|(ip, _)| Some(*ip) == gw).cloned();
        if let Some(l) = &lan {
            found.push((l.ip, l.mac.clone()));
        }
        let fresh_store = { self.lock().store.fresh };
        let t = store::now();
        let mut online = HashSet::new();
        let mut new_devices = Vec::new();
        for (ip, mac) in found {
            online.insert(mac.clone());
            let name = if Some(ip) == me_ip {
                std::env::var("COMPUTERNAME").ok()
            } else {
                system::reverse_dns(IpAddr::V4(ip))
            };
            let vendor = if system::is_randomized_mac(&mac) {
                Some("Private (randomized) MAC address".to_string())
            } else {
                oui.get(&mac[..8]).cloned()
            };
            let mut s = self.lock();
            let d = s.store.data.devices.entry(mac.clone()).or_insert_with(|| {
                new_devices.push(mac.clone());
                DeviceRecord { mac: mac.clone(), first_seen: t, ..Default::default() }
            });
            d.ip = ip.to_string();
            d.last_seen = t;
            if name.is_some() {
                d.name = name;
            }
            if vendor.is_some() {
                d.vendor = vendor;
            }
        }
        if let (Some(gw), Some((_, mac))) = (gw, found_gw) {
            self.check_gateway(&gw.to_string(), &mac);
        }
        let mut s = self.lock();
        let previous = std::mem::replace(&mut s.online, online);
        s.scanning = false;
        s.gateway = gw.map(|g| g.to_string());
        s.local_ip = me_ip.map(|g| g.to_string());
        s.store.data.last_scan = Some(t);
        s.store.mark_dirty();
        if !(baseline && fresh_store) {
            for mac in new_devices {
                let label = s.store.data.devices.get(&mac).map(|d| d.name.clone().unwrap_or_else(|| d.ip.clone())).unwrap_or_default();
                s.store.add_event(EventKind::NewDevice, None, format!("{label} ({mac})"));
                if s.store.data.settings.notifications && s.store.data.settings.notify_devices {
                    notify(&format!("New device joined your network: {label}"));
                }
            }
            // Known devices coming and going (previous is empty on the first scan).
            if s.store.data.settings.notify_device_presence && !previous.is_empty() {
                let label = |s: &Shared, mac: &str| {
                    s.store.data.devices.get(mac).map(|d| d.label.clone().or(d.name.clone()).unwrap_or_else(|| d.ip.clone())).unwrap_or_default()
                };
                let joined: Vec<String> = s.online.iter().filter(|m| !previous.contains(*m) && s.store.data.devices[*m].first_seen != t).cloned().collect();
                let left: Vec<String> = previous.iter().filter(|m| !s.online.contains(*m)).cloned().collect();
                for (macs, kind, verb) in [(joined, EventKind::DeviceJoined, "joined"), (left, EventKind::DeviceLeft, "left")] {
                    for mac in macs {
                        let l = label(&s, &mac);
                        s.store.add_event(kind, None, format!("{l} ({mac})"));
                        if s.store.data.settings.notifications && s.store.data.settings.notify_devices {
                            notify(&format!("{l} {verb} your network"));
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Main sampling loop

    fn run(self, repaint: impl Fn()) {
        unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 0);
        }
        let mut sampler = Sampler::new();
        let mut pid_cache: HashMap<u32, (String, String)> = HashMap::new(); // pid -> (exe, key)
        let mut checked: HashSet<String> = HashSet::new();
        let mut stats: HashMap<(IpAddr, u16, IpAddr, u16), [u64; 2]> = HashMap::new();
        let mut last_if: Option<(u64, u64)> = None;
        let mut last_save = Instant::now();
        let started = Instant::now();
        let admin = self.lock().admin;
        let quiet_until = if self.lock().store.fresh { Some(started + Duration::from_secs(20)) } else { None };
        let mut next = Instant::now();
        let adapters0 = net::adapters();
        crate::diag::log(&format!(
            "start v{} admin={admin} adapters=[{}]",
            env!("CARGO_PKG_VERSION"),
            adapters0.iter().map(|a| format!("{} ({})", a.description, a.kind)).collect::<Vec<_>>().join(", ")
        ));
        // EStats health counters, logged periodically: [enable ok, enable fail, read ok, read fail]
        let mut est = [0u64; 4];
        let mut est_bytes = [0u64; 2]; // attributed vs interface total since last report
        let mut last_report = Instant::now();
        let mut rdp_seen: HashSet<IpAddr> = HashSet::new();
        let mut geo = GeoCache::new();

        loop {
            next += Duration::from_secs(1);
            let t = store::now();

            // --- processes
            let names = net::process_names();
            pid_cache.retain(|pid, (exe, _)| names.get(pid) == Some(exe));
            let mut new_meta: Vec<(String, AppMeta)> = Vec::new();
            for (&pid, exe) in &names {
                if pid == 0 || pid_cache.contains_key(&pid) {
                    continue;
                }
                let (key, meta) = resolve(pid, exe, &mut checked);
                if let Some(m) = meta {
                    new_meta.push((key.clone(), m));
                }
                pid_cache.insert(pid, (exe.clone(), key));
            }

            // --- traffic
            let adapters = net::adapters();
            let if_in: u64 = adapters.iter().map(|a| a.in_octets).sum();
            let if_out: u64 = adapters.iter().map(|a| a.out_octets).sum();
            let (total_down, total_up) = match last_if {
                Some((i, o)) if if_in >= i && if_out >= o => (if_in - i, if_out - o),
                _ => (0, 0),
            };
            last_if = Some((if_in, if_out));

            let mut conns = net::tcp_connections();
            conns.extend(net::udp_endpoints());
            let listening: HashSet<u16> = conns.iter().filter(|c| c.state == TcpState::Listen).map(|c| c.local_port).collect();

            let mut apps_live: HashMap<String, AppLive> = HashMap::new();
            let mut per_app: HashMap<String, [u64; 2]> = HashMap::new();
            let mut per_host: HashMap<String, [u64; 2]> = HashMap::new();
            let mut per_flow: FlowAcc = HashMap::new();
            let mut lan = [0u64; 2];
            let mut attributed = [0u64; 2];
            let mut next_stats = HashMap::new();
            let mut first_contacts: Vec<(String, IpAddr)> = Vec::new();

            let (names_snapshot, dns_lookup, monitor_rdp, notify_on) = {
                let s = self.lock();
                let st = &s.store.data.settings;
                SNOOZE_UNTIL.store(st.snooze_until.unwrap_or(0), Ordering::Relaxed);
                (s.names.clone(), st.dns_lookup, st.monitor_rdp, st.notifications)
            };
            let mut to_resolve = Vec::new();
            let mut rdp_now: HashSet<IpAddr> = HashSet::new();

            for c in &conns {
                if c.pid == 0 {
                    continue;
                }
                let Some((_, key)) = pid_cache.get(&c.pid) else { continue };
                let remote_real = c.proto == Proto::Tcp && !net::is_local(&c.remote) && c.state != TcpState::Listen;
                let mut rate = [0u64; 2];
                let mut total = None;
                if admin && remote_real && matches!(c.state, TcpState::Established | TcpState::FinWait | TcpState::CloseWait) {
                    let k = c.key();
                    match stats.get(&k) {
                        None => {
                            if net::enable_tcp_stats(c) { est[0] += 1 } else { est[1] += 1 }
                            match net::tcp_stats(c) {
                                Some((i, o)) => {
                                    est[2] += 1;
                                    next_stats.insert(k, [i, o]);
                                }
                                None => est[3] += 1,
                            }
                        }
                        Some(prev) => match net::tcp_stats(c) {
                            Some((i, o)) => {
                                est[2] += 1;
                                rate = [i.saturating_sub(prev[0]), o.saturating_sub(prev[1])];
                                total = Some([i, o]);
                                next_stats.insert(k, [i, o]);
                            }
                            None => est[3] += 1,
                        },
                    }
                }
                let host = if remote_real { names_snapshot.get(&c.remote).cloned() } else { None };
                if remote_real && host.is_none() && !net::is_private(&c.remote) {
                    to_resolve.push(c.remote);
                }
                let inbound = remote_real && listening.contains(&c.local_port);
                if inbound && c.local_port == RDP_PORT && c.state == TcpState::Established {
                    rdp_now.insert(c.remote);
                }
                let entry = apps_live.entry(key.clone()).or_default();
                if remote_real {
                    if inbound { entry.in_conns += 1 } else { entry.out_conns += 1 }
                    first_contacts.push((key.clone(), c.remote));
                }
                if rate != [0, 0] {
                    entry.rate[0] += rate[0];
                    entry.rate[1] += rate[1];
                    let pa = per_app.entry(key.clone()).or_default();
                    pa[0] += rate[0];
                    pa[1] += rate[1];
                    let label = host.clone().unwrap_or_else(|| c.remote.to_string());
                    let ty = crate::ports::classify(c.local_port, c.remote_port, inbound, true);
                    accumulate_flow(&mut per_flow, key, &label, ty, &geo.get(c.remote), rate);
                    let ph = per_host.entry(label).or_default();
                    ph[0] += rate[0];
                    ph[1] += rate[1];
                    attributed[0] += rate[0];
                    attributed[1] += rate[1];
                    if net::is_private(&c.remote) {
                        lan[0] += rate[0];
                        lan[1] += rate[1];
                    }
                }
                entry.conns.push(view(c, host, rate, total, inbound));
            }
            stats = next_stats;
            if !dns_lookup {
                to_resolve.clear();
            }
            // One RDP event per remote address per session it stays connected.
            let rdp_new: Vec<IpAddr> = rdp_now.iter().filter(|ip| !rdp_seen.contains(ip)).copied().collect();
            rdp_seen = rdp_now;

            est_bytes[0] += attributed[0] + attributed[1];
            est_bytes[1] += total_down + total_up;
            let report_every = if started.elapsed() < Duration::from_secs(300) { 30 } else { 600 };
            if admin && last_report.elapsed() >= Duration::from_secs(report_every) {
                let share = if est_bytes[1] > 0 { est_bytes[0] as f64 / est_bytes[1] as f64 * 100.0 } else { 0.0 };
                crate::diag::log(&format!(
                    "estats enable ok/fail={}/{} read ok/fail={}/{} tracked={} attributed={:.0}% of {} bytes",
                    est[0], est[1], est[2], est[3], stats.len(), share, est_bytes[1]
                ));
                est = [0; 4];
                est_bytes = [0; 2];
                last_report = Instant::now();
            }
            let other = [total_down.saturating_sub(attributed[0]), total_up.saturating_sub(attributed[1])];
            if other != [0, 0] {
                per_app.insert(OTHER_KEY.into(), other);
                let e = apps_live.entry(OTHER_KEY.into()).or_default();
                e.rate = other;
            }
            let mut flows: HashMap<String, Vec<store::Flow>> = per_flow
                .into_iter()
                .map(|(app, m)| (app, m.into_iter().map(|((h, ty, cc), [d, u])| store::Flow { h, ty, cc, d, u }).collect()))
                .collect();
            if let Some(f) = residual_flow(other, admin) {
                flows.entry(OTHER_KEY.into()).or_default().push(f);
            }
            let total_down = total_down.max(attributed[0]);
            let total_up = total_up.max(attributed[1]);

            // --- hardware
            let pids: Vec<u32> = names.keys().copied().collect();
            let (sys, procs) = sampler.sample(&pids);
            let mut app_hw: HashMap<String, AppHw> = HashMap::new();
            for (pid, ps) in &procs {
                let Some((exe, key)) = pid_cache.get(pid) else { continue };
                let a = app_hw.entry(key.clone()).or_default();
                a.total.cpu += ps.cpu;
                a.total.gpu = a.total.gpu.max(ps.gpu);
                a.total.memory += ps.memory;
                a.total.disk += ps.disk;
                a.procs.push((*pid, exe.clone(), *ps));
            }

            // --- icon requests (shell icons need this COM-initialised thread)
            let requests: Vec<String> = {
                let mut s = self.lock();
                s.icon_requests.drain().collect()
            };
            let mut icons = Vec::new();
            for key in requests {
                let path = {
                    let s = self.lock();
                    s.meta.get(&key).and_then(|m| m.path.clone()).or_else(|| s.store.data.apps.get(&key).and_then(|a| a.path.clone()))
                };
                icons.push((key, path.and_then(|p| net::file_icon(&p))));
            }

            // --- publish
            let mut asks: Vec<String> = Vec::new();
            let mut snap = None;
            {
                let mut guard = self.lock();
                let s = &mut *guard;
                for (k, m) in new_meta {
                    if let Some(rec) = s.store.data.apps.get_mut(&k) {
                        if let (Some(old), Some(new)) = (&rec.version, &m.version) {
                            if old != new {
                                let subject = format!("{old} → {new}");
                                rec.version = m.version.clone();
                                s.store.add_event(EventKind::VersionChanged, Some(k.clone()), subject);
                                if s.store.data.settings.notifications && s.store.data.settings.notify_versions {
                                    notify(&format!("{} was updated to version {new}", m.name));
                                }
                            }
                        }
                    }
                    s.meta.insert(k, m);
                }
                for (k, icon) in icons {
                    s.icons.insert(k, icon);
                }
                for ip in to_resolve {
                    if !s.names.contains_key(&ip) {
                        s.names.insert(ip, ip.to_string()); // placeholder; resolver overwrites
                        let _ = self.dns_tx.send(ip);
                    }
                }

                // App records for anything with sockets.
                for key in apps_live.keys() {
                    if key == OTHER_KEY {
                        continue;
                    }
                    let meta = s.meta.get(key).cloned().unwrap_or_default();
                    let rec = s.store.data.apps.entry(key.clone()).or_insert_with(|| AppRecord {
                        name: meta.name.clone(),
                        path: meta.path.clone(),
                        first_seen: t,
                        ..Default::default()
                    });
                    rec.last_seen = t;
                    if rec.publisher.is_none() {
                        rec.publisher = meta.publisher.clone();
                    }
                    if rec.version.is_none() {
                        rec.version = meta.version.clone();
                    }
                    if !meta.name.is_empty() {
                        rec.name = meta.name.clone();
                    }
                }
                if monitor_rdp {
                    for ip in rdp_new {
                        s.store.add_event(EventKind::Rdp, None, ip.to_string());
                        if notify_on {
                            notify(&format!("Remote Desktop connection from {ip}"));
                        }
                    }
                }
                let quiet = quiet_until.map(|q| Instant::now() < q).unwrap_or(false);
                for (key, ip) in first_contacts {
                    let host = s.host_label(&ip);
                    let name = s.app_name(&key);
                    let settings = s.store.data.settings.clone();
                    let Some(rec) = s.store.data.apps.get_mut(&key) else { continue };
                    rec.note_host(&host);
                    if rec.first_net.is_none() {
                        rec.first_net = Some(t);
                        if !quiet {
                            if settings.firewall_mode == FirewallMode::AskToConnect && settings.firewall_enabled && !s.asks.contains(&key) {
                                s.asks.push(key.clone());
                                asks.push(key.clone());
                            }
                            s.store.add_event(EventKind::NewApp, Some(key.clone()), host.clone());
                            if settings.notifications && settings.notify_new_apps {
                                notify(&format!("{name} initiated its first network connection to {host}"));
                            }
                        }
                    }
                }

                let mut sample_apps: Vec<(String, [u64; 2])> = per_app.iter().map(|(k, v)| (k.clone(), *v)).collect();
                sample_apps.sort_by_key(|(_, v)| std::cmp::Reverse(v[0] + v[1]));
                let sample_flows: Vec<(String, store::Flow)> =
                    flows.iter().flat_map(|(app, list)| list.iter().map(move |f| (app.clone(), f.clone()))).collect();
                s.live.push_back(LiveSample { t, down: total_down, up: total_up, apps: sample_apps, flows: sample_flows });
                while s.live.len() > LIVE_SECONDS {
                    s.live.pop_front();
                }
                trim_live_flows(&mut s.live, LIVE_FLOW_SECONDS);
                s.store.add_usage(t, [total_down, total_up, lan[0], lan[1]], &per_app, &per_host, &flows);
                let cap = s.store.data.settings.daily_cap_mb * 1_000_000;
                let (used, period) = s.store.plan_usage();
                if cap > 0 && used >= cap && s.store.data.cap_alerted.as_deref() != Some(&period) {
                    s.store.data.cap_alerted = Some(period);
                    s.store.add_event(EventKind::DataCap, None, format!("{} MB", s.store.data.settings.daily_cap_mb));
                    if s.store.data.settings.notifications {
                        notify("You have reached your data plan limit.");
                    }
                }
                s.apps_live = apps_live;
                s.adapters = adapters;

                s.hw.history.push_back(sys);
                while s.hw.history.len() > HW_SECONDS {
                    s.hw.history.pop_front();
                }
                for (k, a) in &app_hw {
                    let h = s.hw.app_history.entry(k.clone()).or_default();
                    h.push_back(a.total);
                    while h.len() > HW_SECONDS {
                        h.pop_front();
                    }
                }
                s.hw.app_history.retain(|k, _| app_hw.contains_key(k));
                s.hw.apps = app_hw;

                if last_save.elapsed() >= SAVE_EVERY {
                    snap = s.store.snapshot();
                    last_save = Instant::now();
                }
            }
            // Written after the lock is released, so the UI never waits on the disk.
            if let Some((dir, gen, data)) = snap {
                Store::write_snapshot(&dir, gen, &data);
            }
            for key in asks {
                self.set_block(&key, Dir::Out, true);
                let name = self.lock().app_name(&key);
                if notify_on {
                    notify(&format!("{name} wants to connect to the internet. It is blocked until you allow it in Vantage."));
                }
            }
            repaint();

            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
    }
}

fn view(c: &Conn, host: Option<String>, rate: [u64; 2], total: Option<[u64; 2]>, inbound: bool) -> ConnView {
    ConnView {
        proto: c.proto,
        state: c.state,
        local: c.local,
        local_port: c.local_port,
        remote: c.remote,
        remote_port: c.remote_port,
        pid: c.pid,
        host,
        rate,
        total,
        inbound,
    }
}

fn pretty_exe(exe: &str) -> String {
    Path::new(exe).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| exe.to_string())
}

/// Maps a process to its app key, plus app metadata the first time a key is seen.
fn resolve(pid: u32, exe: &str, checked: &mut HashSet<String>) -> (String, Option<AppMeta>) {
    if pid == 4 || exe.eq_ignore_ascii_case("System") {
        let key = SYSTEM_KEY.to_string();
        let meta = checked.insert(key.clone()).then(|| AppMeta {
            name: "NT Kernel & System".into(),
            path: Some(r"C:\Windows\System32\ntoskrnl.exe".into()),
            publisher: Some("Microsoft Corporation".into()),
            version: None,
        });
        return (key, meta);
    }
    match net::process_path(pid) {
        Some(path) => {
            let key = path.to_lowercase();
            if !checked.insert(key.clone()) {
                return (key, None);
            }
            let v = net::version_info(&path);
            let name = v.description.clone().filter(|d| d.len() > 1).unwrap_or_else(|| pretty_exe(exe));
            (key, Some(AppMeta { name, path: Some(path), publisher: v.company, version: v.version }))
        }
        None => {
            let key = format!("exe:{}", exe.to_lowercase());
            let meta = checked.insert(key.clone()).then(|| AppMeta { name: exe.to_string(), ..Default::default() });
            (key, meta)
        }
    }
}

fn notify(body: &str) {
    if store::now() < SNOOZE_UNTIL.load(Ordering::Relaxed) {
        return;
    }
    let body = body.to_string();
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new().summary("Vantage").body(&body).show();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_merges() {
        let mut m: HashMap<String, HashMap<(String, String, String), [u64; 2]>> = HashMap::new();
        accumulate_flow(&mut m, "app", "example.com", "https", "US", [10, 2]);
        accumulate_flow(&mut m, "app", "example.com", "https", "US", [5, 3]);
        accumulate_flow(&mut m, "app", "other.com", "https", "US", [1, 1]);
        let a = &m["app"];
        assert_eq!(a.len(), 2);
        assert_eq!(a[&("example.com".into(), "https".into(), "US".into())], [15, 5]);
    }

    #[test]
    fn residual_only_when_admin() {
        assert_eq!(residual_flow([7, 3], false), None);
        let f = residual_flow([7, 3], true).expect("admin residual");
        assert_eq!(f, store::Flow { h: String::new(), ty: crate::ports::UNATTRIBUTED.into(), cc: "ZZ".into(), d: 7, u: 3 });
    }

    #[test]
    fn residual_zero_is_none() {
        assert_eq!(residual_flow([0, 0], true), None);
    }

    #[test]
    fn geo_cache_bounded() {
        let mut g = GeoCache::new();
        let mut calls = 0usize;
        for i in 0..=g.cap as u32 {
            let ip = IpAddr::from(std::net::Ipv4Addr::from(0x0b00_0000 + i));
            g.get_with(ip, |_| {
                calls += 1;
                "US".into()
            });
            assert!(g.map.len() <= g.cap);
        }
        assert_eq!(calls, g.cap + 1);
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert_eq!(g.get_with(ip, |_| "US".into()), "US");
        let before = calls;
        assert_eq!(
            g.get_with(ip, |_| {
                calls += 1;
                "XX".into()
            }),
            "US"
        );
        assert_eq!(calls, before);
    }

    #[test]
    fn live_flows_trimmed() {
        let f = store::Flow { h: "h".into(), ty: "https".into(), cc: "US".into(), d: 1, u: 1 };
        let mut live: VecDeque<LiveSample> = (0..10)
            .map(|t| LiveSample { t, down: 1, up: 1, apps: Vec::new(), flows: vec![("a".into(), f.clone())] })
            .collect();
        trim_live_flows(&mut live, 3);
        for (i, s) in live.iter().enumerate() {
            assert_eq!(s.flows.is_empty(), i < 7, "sample {i}");
        }
        trim_live_flows(&mut live, 300);
        assert_eq!(live.iter().filter(|s| !s.flows.is_empty()).count(), 3);
    }

    #[test]
    fn save_every_is_two_minutes() {
        assert_eq!(SAVE_EVERY, Duration::from_secs(120));
    }
}
