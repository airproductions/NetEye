//! Demo mode (`neteye.exe --demo`): every page filled with made-up but plausible data, for
//! screenshots and screen recordings. It runs on its own data folder in %TEMP%, never saves,
//! never touches the firewall or the registry, and doesn't watch the real network.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::hw::{ProcSample, SystemSample};
use crate::monitor::{AppHw, AppLive, ConnView, LiveSample, Monitor, Shared, HW_SECONDS, LIVE_SECONDS};
use crate::net::{Adapter, Proto, TcpState};
use crate::store::{self, AppRecord, DeviceRecord, EventKind, Flow, MINUTE};

static ON: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ON.store(true, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Small deterministic PRNG (xorshift64*), so the demo looks the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A demo app: display name, publisher, version, exe path (real paths get real icons),
/// average download in KB/s while busy, upload share, hosts as (name, ip, remote port, udp).
struct App {
    name: &'static str,
    publisher: &'static str,
    version: &'static str,
    path: &'static str,
    kbps: f64,
    up: f64,
    hosts: &'static [(&'static str, &'static str, u16, bool)],
}

const APPS: &[App] = &[
    App { name: "Firefox", publisher: "Mozilla Corporation", version: "132.0.1", path: r"C:\Program Files\Mozilla Firefox\firefox.exe", kbps: 420.0, up: 0.08,
        hosts: &[("www.youtube.com", "142.250.185.78", 443, false), ("github.com", "140.82.121.4", 443, false), ("en.wikipedia.org", "185.15.59.224", 443, false), ("www.bbc.co.uk", "212.58.244.20", 443, false), ("www.reddit.com", "151.101.1.140", 443, false), ("news.ycombinator.com", "209.216.230.207", 443, false)] },
    App { name: "Microsoft Edge", publisher: "Microsoft Corporation", version: "130.0.2849.80", path: r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", kbps: 160.0, up: 0.1,
        hosts: &[("www.bing.com", "13.107.21.200", 443, false), ("outlook.office.com", "52.97.146.162", 443, false), ("www.amazon.co.jp", "52.119.161.5", 443, false)] },
    App { name: "Steam", publisher: "Valve Corporation", version: "10.0.0.1", path: r"C:\Program Files (x86)\Steam\steam.exe", kbps: 900.0, up: 0.02,
        hosts: &[("cdn.akamai.steamstatic.com", "2.16.38.160", 443, false), ("steamcommunity.com", "23.33.233.30", 443, false), ("valve518.steamcontent.com", "155.133.229.4", 443, false)] },
    App { name: "Spotify", publisher: "Spotify Ltd", version: "1.2.48.405", path: r"C:\Users\Public\Spotify\Spotify.exe", kbps: 40.0, up: 0.05,
        hosts: &[("audio-ak.spotifycdn.com", "23.48.168.41", 443, false), ("spclient.wg.spotify.com", "35.186.224.25", 443, false)] },
    App { name: "Discord", publisher: "Discord Inc.", version: "1.0.9168", path: r"C:\Users\Public\Discord\Discord.exe", kbps: 60.0, up: 0.4,
        hosts: &[("gateway.discord.gg", "162.159.135.232", 443, false), ("media.discordapp.net", "162.159.130.233", 443, false), ("frankfurt9921.discord.media", "66.22.221.15", 50005, true)] },
    App { name: "Zoom", publisher: "Zoom Video Communications, Inc.", version: "6.2.6", path: r"C:\Users\Public\Zoom\Zoom.exe", kbps: 180.0, up: 0.8,
        hosts: &[("zoom.us", "170.114.52.2", 443, false), ("zoomam118mmr.am1.zoom.us", "144.195.19.5", 8801, true)] },
    App { name: "OneDrive", publisher: "Microsoft Corporation", version: "24.201.1006", path: r"C:\Users\Public\OneDrive\OneDrive.exe", kbps: 30.0, up: 1.6,
        hosts: &[("onedrive.live.com", "13.107.42.13", 443, false), ("api.onedrive.com", "13.107.42.12", 443, false)] },
    App { name: "Windows Update", publisher: "Microsoft Corporation", version: "10.0.22631", path: r"C:\Windows\System32\svchost.exe", kbps: 260.0, up: 0.01,
        hosts: &[("dl.delivery.mp.microsoft.com", "8.249.235.254", 443, false), ("time.windows.com", "51.145.123.29", 123, true)] },
    App { name: "Windows Explorer", publisher: "Microsoft Corporation", version: "10.0.22631", path: r"C:\Windows\explorer.exe", kbps: 6.0, up: 0.2,
        hosts: &[("arc.msn.com", "20.42.65.85", 443, false)] },
    App { name: "NVIDIA App", publisher: "NVIDIA Corporation", version: "11.0.1.163", path: r"C:\Program Files\NVIDIA Corporation\NVIDIA app\CEF\NVIDIA app.exe", kbps: 12.0, up: 0.3,
        hosts: &[("gfwsl.geforce.com", "8.43.85.13", 443, false)] },
    App { name: "Telegram", publisher: "Telegram FZ-LLC", version: "5.6.3", path: r"C:\Users\Public\Telegram\Telegram.exe", kbps: 22.0, up: 0.3,
        hosts: &[("telegram.org", "149.154.167.99", 443, false), ("web.telegram.org", "149.154.167.220", 443, false)] },
    App { name: "Visual Studio Code", publisher: "Microsoft Corporation", version: "1.95.2", path: r"C:\Users\Public\VSCode\Code.exe", kbps: 18.0, up: 0.2,
        hosts: &[("marketplace.visualstudio.com", "13.107.6.175", 443, false), ("api.github.com", "140.82.121.6", 443, false)] },
    App { name: "Epic Games Launcher", publisher: "Epic Games, Inc.", version: "17.3.0", path: r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe", kbps: 120.0, up: 0.03,
        hosts: &[("download.epicgames.com", "104.18.6.71", 443, false), ("epicgames-download1.akamaized.net", "23.62.230.88", 443, false)] },
    App { name: "WeChat", publisher: "Tencent", version: "3.9.12", path: r"C:\Users\Public\WeChat\WeChat.exe", kbps: 9.0, up: 0.4,
        hosts: &[("szshort.weixin.qq.com", "109.244.217.18", 443, false), ("dns.weixin.qq.com", "183.3.234.152", 443, false)] },
    App { name: "Dropbox", publisher: "Dropbox, Inc.", version: "212.4.5767", path: r"C:\Program Files (x86)\Dropbox\Client\Dropbox.exe", kbps: 15.0, up: 0.9,
        hosts: &[("client.dropbox.com", "162.125.21.3", 443, false), ("content.dropboxapi.com", "162.125.6.20", 443, false)] },
    App { name: "Yandex Music", publisher: "YANDEX LLC", version: "5.25.1", path: r"C:\Users\Public\Yandex\Music.exe", kbps: 25.0, up: 0.05,
        hosts: &[("music.yandex.ru", "213.180.204.89", 443, false)] },
    App { name: "UOL Mail", publisher: "UOL", version: "2.4", path: r"C:\Users\Public\UOL\mail.exe", kbps: 4.0, up: 0.3,
        hosts: &[("email.uol.com.br", "200.147.36.51", 443, false)] },
    App { name: "ABC iview", publisher: "Australian Broadcasting Corporation", version: "8.1", path: r"C:\Users\Public\ABC\iview.exe", kbps: 70.0, up: 0.02,
        hosts: &[("iview.abc.net.au", "203.2.218.214", 443, false)] },
];

/// Local hour of day with minutes as a fraction (14.5 = 2:30 PM).
fn local_hour(t: i64) -> f64 {
    use chrono::Timelike;
    let l = chrono::DateTime::from_timestamp(t, 0).unwrap_or_default().with_timezone(&chrono::Local);
    l.hour() as f64 + l.minute() as f64 / 60.0
}

fn key(a: &App) -> String {
    a.path.to_lowercase()
}

/// How busy the network is at a local hour: low overnight, peaks in the evening.
fn diurnal(hour: f64) -> f64 {
    0.12 + 0.88 * (((hour - 20.0) / 24.0 * std::f64::consts::TAU).cos() * 0.5 + 0.5).powf(1.6)
}

/// Country for a demo IP, looked up once (the history seeding asks thousands of times).
fn country(ip: &str) -> String {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    c.entry(ip.to_string()).or_insert_with(|| ip.parse::<IpAddr>().map(crate::geo::country_of).unwrap_or_else(|_| crate::geo::UNKNOWN.to_string())).clone()
}

/// One stretch of traffic: per app, per host bytes for `secs` seconds at `level` (0..1).
fn traffic(rng: &mut Rng, secs: f64, level: f64, busy: &[f64]) -> (HashMap<String, [u64; 2]>, HashMap<String, [u64; 2]>, HashMap<String, Vec<Flow>>) {
    let (mut apps, mut hosts, mut flows) = (HashMap::new(), HashMap::new(), HashMap::new());
    for (i, a) in APPS.iter().enumerate() {
        let on = busy[i] * level;
        if on < 0.02 {
            continue;
        }
        let down = (a.kbps * 1000.0 * secs * on * (0.5 + rng.f())) as u64;
        let up = (down as f64 * a.up * (0.6 + 0.8 * rng.f())) as u64;
        let mut list = Vec::new();
        let n = a.hosts.len();
        for (j, (h, ip, port, udp)) in a.hosts.iter().enumerate() {
            // the first host carries most of an app's traffic
            let share = if n == 1 { 1.0 } else if j == 0 { 0.55 } else { 0.45 / (n - 1) as f64 };
            let (d, u) = ((down as f64 * share) as u64, (up as f64 * share) as u64);
            let cc = country(ip);
            list.push(Flow { h: h.to_string(), ty: crate::ports::classify(50_000, *port, false, !udp).to_string(), cc, d, u });
            let e = hosts.entry(h.to_string()).or_insert([0, 0]);
            e[0] += d;
            e[1] += u;
        }
        apps.insert(key(a), [down, up]);
        flows.insert(key(a), list);
    }
    (apps, hosts, flows)
}

fn sum(apps: &HashMap<String, [u64; 2]>) -> [u64; 2] {
    apps.values().fold([0, 0], |s, v| [s[0] + v[0], s[1] + v[1]])
}

/// Fills `s` with history, apps, devices and alerts, as if NetEye had run for a month.
pub fn seed(s: &mut Shared) {
    let now = store::now();
    let mut rng = Rng(0x5eed_1e7e_0f_2026);
    s.admin = true;
    s.internet = Some(true);
    s.gateway = Some("192.168.1.1".into());
    s.local_ip = Some("192.168.1.23".into());
    s.adapters = vec![Adapter { index: 7, alias: "Wi-Fi".into(), description: "Intel(R) Wi-Fi 6E AX211 160MHz".into(), kind: "Wi-Fi", speed: 1_200_000_000, mac: "a4:c3:f0:12:7b:9e".into(), in_octets: 0, out_octets: 0 }];
    let st = &mut s.store.data.settings;
    st.daily_cap_mb = 50_000;
    st.plan_period = store::PlanPeriod::Month;
    st.firewall_enabled = true;
    st.notifications = false;

    for a in APPS {
        s.store.data.apps.insert(
            key(a),
            AppRecord {
                name: a.name.into(),
                path: Some(a.path.into()),
                publisher: Some(a.publisher.into()),
                version: Some(a.version.into()),
                first_seen: now - 29 * 86_400 - (rng.next() % 86_400) as i64,
                last_seen: now,
                first_net: Some(now - 29 * 86_400),
                hosts: a.hosts.iter().map(|h| h.0.to_string()).collect(),
                ..Default::default()
            },
        );
    }
    // a couple of rules so the firewall page isn't all green
    if let Some(r) = s.store.data.apps.get_mut(&key(&APPS[13])) {
        r.block_out = true;
    }
    if let Some(r) = s.store.data.apps.get_mut(&key(&APPS[9])) {
        r.block_in = true;
    }

    // 32 days of history: 5-minute steps, then every minute for the last day
    let mut busy: Vec<f64> = APPS.iter().map(|_| rng.f()).collect();
    let mut t = now - 32 * 86_400;
    t -= t.rem_euclid(5 * MINUTE);
    while t < now {
        let step = if now - t > 86_400 { 5 * MINUTE } else { MINUTE };
        for b in busy.iter_mut() {
            *b = (*b + (rng.f() - 0.5) * 0.25).clamp(0.0, 1.0);
        }
        let hour = local_hour(t);
        let (apps, hosts, flows) = traffic(&mut rng, step as f64, diurnal(hour), &busy);
        let tot = sum(&apps);
        let lan = [tot[0] / 40, tot[1] / 25];
        s.store.add_usage(t, [tot[0], tot[1], lan[0], lan[1]], &apps, &hosts, &flows);
        t += step;
    }

    // the last five minutes of live samples, so the graph starts full
    for i in (1..=300).rev() {
        push_live(s, &mut rng, now - i, &mut busy, false);
    }

    // LAN devices
    let devs = [
        ("60:d8:a4:a4:e6:3e", "192.168.1.1", "Router", "Vantiva Internet Gateway", "Router"),
        ("a4:c3:f0:12:7b:9e", "192.168.1.23", "This computer", "Intel Corporate", "Computer"),
        ("bc:c7:46:a6:3a:3c", "192.168.1.130", "Living room PC", "Hon Hai Precision", "Computer"),
        ("c2:59:1c:9b:99:f3", "192.168.1.102", "Galaxy S24", "Samsung, Galaxy S24", "Phone"),
        ("62:3d:87:d0:40:9d", "192.168.1.211", "iPhone", "Apple, iPhone", "Phone"),
        ("9a:58:ad:ba:8f:ef", "192.168.1.181", "Streaming dongle", "Google, Chromecast", "Media"),
        ("2c:d8:ae:31:dc:e4", "192.168.1.185", "Bedroom TV", "SEI Robotics, PartnerTV Box", "Media"),
        ("f4:f5:d8:2a:11:07", "192.168.1.150", "Office printer", "Hewlett Packard", "Device"),
        ("3c:6a:d2:90:5e:44", "192.168.1.160", "Doorbell", "Ring LLC", "Device"),
    ];
    for (i, (mac, ip, name, vendor, kind)) in devs.iter().enumerate() {
        s.store.data.devices.insert(
            mac.to_string(),
            DeviceRecord {
                mac: mac.to_string(),
                ip: ip.to_string(),
                name: Some(name.to_string()),
                vendor: Some(vendor.to_string()),
                label: None,
                trusted: i < 6,
                kind: Some(kind.to_string()),
                first_seen: now - (20 - i as i64) * 86_400,
                last_seen: now - if i == 7 { 3 * 3600 } else { 0 },
            },
        );
        if i != 7 {
            s.online.insert(mac.to_string());
        }
    }
    s.store.data.last_scan = Some(now - 4 * 60);

    // alerts over the last few days, newest last
    let k = |i: usize| Some(key(&APPS[i]));
    let events: [(i64, EventKind, Option<String>, &str); 14] = [
        (3 * 86_400 + 4_000, EventKind::NewApp, k(15), "music.yandex.ru"),
        (2 * 86_400 + 9_000, EventKind::VersionChanged, k(0), "131.0.3 → 132.0.1"),
        (2 * 86_400 + 2_000, EventKind::NewDevice, None, "Doorbell (3c:6a:d2:90:5e:44)"),
        (86_400 + 30_000, EventKind::InternetDown, None, ""),
        (86_400 + 29_700, EventKind::InternetUp, None, ""),
        (86_400 + 12_000, EventKind::Firewall, k(13), "Outgoing blocked"),
        (86_400 + 3_000, EventKind::NewAdapter, None, "WireGuard Tunnel"),
        (40_000, EventKind::CameraUsed, None, "Zoom.exe"),
        (32_000, EventKind::NewApp, k(5), "zoom.us"),
        (20_000, EventKind::DeviceJoined, None, "Galaxy S24 (c2:59:1c:9b:99:f3)"),
        (9_000, EventKind::PublisherChanged, k(16), "UOL → Unknown Publisher Ltd"),
        (5_400, EventKind::HostsChanged, None, "entries were added, removed or edited"),
        (1_800, EventKind::DataCap, None, "75% of 50000 MB"),
        (300, EventKind::NewApp, k(17), "iview.abc.net.au"),
    ];
    for (ago, kind, app, subject) in events {
        s.store.add_event(kind, app, subject.to_string());
        if let Some(e) = s.store.data.events.front_mut() {
            e.t = now - ago;
            e.read = ago > 86_400;
        }
    }
}

/// One live second: rates, connections and (optionally) the history bucket for it.
fn push_live(s: &mut Shared, rng: &mut Rng, t: i64, busy: &mut [f64], record: bool) {
    for b in busy.iter_mut() {
        *b = (*b + (rng.f() - 0.5) * 0.08).clamp(0.0, 1.0);
    }
    let burst = if rng.f() < 0.03 { 3.0 + 4.0 * rng.f() } else { 1.0 };
    let (apps, hosts, flows) = traffic(rng, 1.0, (diurnal(local_hour(t)) * burst).min(4.0), busy);
    let tot = sum(&apps);
    let mut sample_apps: Vec<(String, [u64; 2])> = apps.iter().map(|(k, v)| (k.clone(), *v)).collect();
    sample_apps.sort_by_key(|(_, v)| std::cmp::Reverse(v[0] + v[1]));
    let sample_flows: Vec<(String, Flow)> = flows.iter().flat_map(|(a, l)| l.iter().map(move |f| (a.clone(), f.clone()))).collect();
    s.live.push_back(LiveSample { t, down: tot[0], up: tot[1], apps: sample_apps, flows: sample_flows });
    while s.live.len() > LIVE_SECONDS {
        s.live.pop_front();
    }
    if record {
        s.store.add_usage(t, [tot[0], tot[1], tot[0] / 40, tot[1] / 25], &apps, &hosts, &flows);
    }

    // connections and per-app rates for the lists, the firewall page and the globe
    let mut live: HashMap<String, AppLive> = HashMap::new();
    for (i, a) in APPS.iter().enumerate() {
        if busy[i] < 0.15 {
            continue;
        }
        let rate = apps.get(&key(a)).copied().unwrap_or([0, 0]);
        let mut conns = Vec::new();
        for (j, (h, ip, port, udp)) in a.hosts.iter().enumerate() {
            let Ok(remote) = ip.parse::<IpAddr>() else { continue };
            s.names.insert(remote, h.to_string());
            let share = if j == 0 { 0.6 } else { 0.4 / a.hosts.len().max(2) as f64 };
            conns.push(ConnView {
                proto: if *udp { Proto::Udp } else { Proto::Tcp },
                state: TcpState::Established,
                local: "192.168.1.23".parse().unwrap(),
                local_port: 50_000 + (i * 10 + j) as u16,
                remote,
                remote_port: *port,
                pid: 4_000 + i as u32 * 16,
                host: Some(h.to_string()),
                rate: [(rate[0] as f64 * share) as u64, (rate[1] as f64 * share) as u64],
                total: None,
                inbound: false,
            });
        }
        let n = conns.len();
        live.insert(key(a), AppLive { conns, rate, in_conns: 0, out_conns: n });
    }
    s.apps_live = live;

    // hardware: system and per-app samples
    let cpu = (8.0 + 30.0 * busy.iter().sum::<f64>() / busy.len() as f64 + 6.0 * rng.f()) as f32;
    let sys = SystemSample { cpu, mem_used: 11_800_000_000 + (rng.next() % 600_000_000), mem_total: 32_000_000_000, disk_read: rng.f() * 4e6, disk_write: rng.f() * 2e6, gpu: (5.0 + 20.0 * rng.f()) as f32, gpu_mem: 2_400_000_000 };
    s.hw.history.push_back(sys);
    while s.hw.history.len() > HW_SECONDS {
        s.hw.history.pop_front();
    }
    for (i, a) in APPS.iter().enumerate() {
        let p = ProcSample {
            cpu: (busy[i] * 6.0 * rng.f()) as f32,
            gpu: if i == 2 || i == 12 { (busy[i] * 30.0) as f32 } else { 0.0 },
            memory: 80_000_000 + (a.kbps * 1_500_000.0) as u64 + (rng.next() % 40_000_000),
            disk: busy[i] * rng.f() * 2e5,
        };
        let exe = a.path.rsplit('\\').next().unwrap_or(a.path).to_string();
        s.hw.apps.insert(key(a), AppHw { total: p, procs: vec![(4_000 + i as u32 * 16, exe, p)] });
        let h = s.hw.app_history.entry(key(a)).or_default();
        h.push_back(p);
        while h.len() > HW_SECONDS {
            h.pop_front();
        }
    }
}

/// The demo's main loop: one live second at a time, plus icons for apps that ask for one.
pub fn run(m: Monitor, repaint: impl Fn()) {
    let mut rng = Rng(0x0dd_ba11_2026);
    let mut busy: Vec<f64> = APPS.iter().map(|_| rng.f()).collect();
    loop {
        let t = store::now();
        let requests: Vec<String> = m.lock().icon_requests.drain().collect();
        let mut icons = Vec::new();
        for k in requests {
            let path = m.lock().store.data.apps.get(&k).and_then(|a| a.path.clone());
            icons.push((k, path.filter(|p| std::path::Path::new(p).exists()).and_then(|p| crate::net::file_icon(&p))));
        }
        {
            let mut s = m.lock();
            for (k, ic) in icons {
                s.icons.insert(k, ic);
            }
            if s.live.back().map(|l| l.t) != Some(t) {
                push_live(&mut s, &mut rng, t, &mut busy, true);
            }
        }
        repaint();
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_millis());
        std::thread::sleep(Duration::from_millis(u64::from(1000 - ms.min(999))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_fills_every_page() {
        let mut s = Shared::new(crate::store::Store::demo());
        let t0 = std::time::Instant::now();
        seed(&mut s);
        let d = &s.store.data;
        assert_eq!(d.apps.len(), APPS.len());
        assert!(d.hours.len() >= 24 * 31, "a month of hourly history");
        assert!(d.minutes.len() >= 24 * 60 - 1, "a day of minute history");
        assert!(d.daily.len() >= 31);
        assert_eq!(s.live.len(), 300);
        assert!(!s.apps_live.is_empty() && s.apps_live.values().all(|a| !a.conns.is_empty()));
        assert!(d.events.len() >= 14 && d.devices.len() == 9);
        assert_eq!(s.hw.history.len(), 300);
        // hosts resolve to several countries, so the globe has something to show
        let countries: std::collections::HashSet<String> = d.hours.iter().flat_map(|b| b.strs.iter().filter(|x| x.len() == 2).cloned()).collect();
        assert!(countries.len() >= 6, "{countries:?}");
        // the demo store never writes
        assert!(s.store.snapshot().is_none());
        assert!(t0.elapsed().as_secs() < 20, "seeding took {:?}", t0.elapsed());
    }

    #[test]
    fn diurnal_peaks_in_the_evening() {
        assert!(diurnal(20.0) > diurnal(4.0) * 3.0);
        assert!((0.0..=1.0).contains(&diurnal(13.0)));
    }

    #[test]
    fn traffic_splits_across_hosts_and_reconciles() {
        let mut rng = Rng(1);
        let busy = vec![1.0; APPS.len()];
        let (apps, hosts, flows) = traffic(&mut rng, 60.0, 1.0, &busy);
        assert_eq!(apps.len(), APPS.len());
        let flow_down: u64 = flows.values().flatten().map(|f| f.d).sum();
        let host_down: u64 = hosts.values().map(|v| v[0]).sum();
        assert_eq!(flow_down, host_down);
        assert!(flow_down <= sum(&apps)[0]);
    }
}
