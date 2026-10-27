//! The background engine. One thread samples connections, traffic and hardware every second
//! and publishes the results into `Shared`, which the UI reads.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::hw::{ProcSample, Sampler, SystemSample};
use crate::net::{self, Adapter, Conn, Proto, TcpState};
use crate::store::{self, AppRecord, DeviceRecord, EventKind, Store};
use crate::system::{self, Dir};

pub const LIVE_SECONDS: usize = 3600;
pub const HW_SECONDS: usize = 300;
pub const SYSTEM_KEY: &str = "system";
pub const OTHER_KEY: &str = "__other";

#[derive(Clone, Debug)]
pub struct LiveSample {
    pub t: i64,
    pub down: u64,
    pub up: u64,
    pub apps: Vec<(String, [u64; 2])>,
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
        m
    }

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn save(&self) {
        self.lock().store.save();
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
            let auto = self.lock().store.data.settings.auto_scan;
            if first || auto {
                self.scan(&oui, first);
            }
            first = false;
            for _ in 0..300 {
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
        let mut s = self.lock();
        s.online = online;
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
            let mut lan = [0u64; 2];
            let mut attributed = [0u64; 2];
            let mut next_stats = HashMap::new();
            let mut first_contacts: Vec<(String, IpAddr)> = Vec::new();

            let names_snapshot = { self.lock().names.clone() };
            let mut to_resolve = Vec::new();

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
                            s.store.add_event(EventKind::NewApp, Some(key.clone()), host.clone());
                            if settings.notifications && settings.notify_new_apps {
                                notify(&format!("{name} initiated its first network connection to {host}"));
                            }
                        }
                    }
                }

                let mut sample_apps: Vec<(String, [u64; 2])> = per_app.iter().map(|(k, v)| (k.clone(), *v)).collect();
                sample_apps.sort_by_key(|(_, v)| std::cmp::Reverse(v[0] + v[1]));
                s.live.push_back(LiveSample { t, down: total_down, up: total_up, apps: sample_apps });
                while s.live.len() > LIVE_SECONDS {
                    s.live.pop_front();
                }
                let today = s.store.add_usage(t, [total_down, total_up, lan[0], lan[1]], &per_app, &per_host);
                let cap = s.store.data.settings.daily_cap_mb * 1_000_000;
                let day = chrono::Local::now().format("%Y-%m-%d").to_string();
                if cap > 0 && today[0] + today[1] >= cap && s.store.data.cap_alerted.as_deref() != Some(&day) {
                    s.store.data.cap_alerted = Some(day);
                    s.store.add_event(EventKind::DataCap, None, format!("{} MB", s.store.data.settings.daily_cap_mb));
                    if s.store.data.settings.notifications {
                        notify("You have reached your daily data usage limit.");
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

                if last_save.elapsed() > Duration::from_secs(30) {
                    s.store.save();
                    last_save = Instant::now();
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
    let body = body.to_string();
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new().summary("Vantage").body(&body).show();
    });
}
