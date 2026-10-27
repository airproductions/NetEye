//! Slower lookups that run on worker threads: firewall rules, LAN scanning, MAC vendors,
//! internet reachability and reverse DNS.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use windows_sys::Win32::NetworkManagement::IpHelper::{GetIpAddrTable, GetIpForwardTable, SendARP};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden(cmd: &str) -> Command {
    let mut c = Command::new(cmd);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

fn powershell(script: &str) -> std::io::Result<std::process::Output> {
    hidden("powershell.exe").args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]).output()
}

// ---------------------------------------------------------------------------------------------
// Firewall: block filters live in WFP (see wfp.rs). This wraps them with elevation.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    In,
    Out,
}

/// Applies a batch of block/unblock changes. When not elevated, re-runs this exe elevated in
/// helper mode (one UAC prompt per batch).
pub fn apply_firewall(changes: &[(String, Dir, bool)], admin: bool, close_open: bool) -> Result<(), String> {
    if changes.is_empty() {
        return Ok(());
    }
    if admin {
        return crate::wfp::apply(changes, close_open);
    }
    let batch = std::env::temp_dir().join(format!("vantage-fw-{}.txt", std::process::id()));
    let body: String = changes
        .iter()
        .map(|(p, d, b)| format!("{}\t{}\t{}\n", if *b { 1 } else { 0 }, if *d == Dir::In { "in" } else { "out" }, p))
        .collect();
    let body = format!("#close_open={}\n{body}", close_open as u8);
    std::fs::write(&batch, body).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let q = |s: &str| s.replace('\'', "''");
    let script = format!(
        "try {{ $p = Start-Process -FilePath '{}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList '--fw-batch','\"{}\"'; exit $p.ExitCode }} catch {{ Write-Output 'cancelled'; exit 1223 }}",
        q(&exe.display().to_string()),
        q(&batch.display().to_string())
    );
    let out = powershell(&script).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&batch);
    let out = out?;
    crate::diag::log(&format!("elevated firewall batch ({} changes) -> {}", changes.len(), out.status));
    if String::from_utf8_lossy(&out.stdout).contains("cancelled") {
        return Err("Administrator permission was declined.".into());
    }
    if !out.status.success() {
        return Err("The filtering engine rejected the change (see vantage.log).".into());
    }
    Ok(())
}

/// Entry point for `vantage.exe --fw-batch <file>` (runs elevated).
pub fn run_firewall_batch(file: &str) -> i32 {
    let Ok(text) = std::fs::read_to_string(file.trim_matches('"')) else { return 2 };
    let close_open = !text.contains("#close_open=0");
    let changes: Vec<(String, Dir, bool)> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            let block = p.next()? == "1";
            let dir = if p.next()? == "in" { Dir::In } else { Dir::Out };
            Some((p.next()?.to_string(), dir, block))
        })
        .collect();
    match crate::wfp::apply(&changes, close_open) {
        Ok(()) => 0,
        Err(e) => {
            crate::diag::log(&format!("firewall batch failed: {e}"));
            1
        }
    }
}

/// Deletes Windows Firewall rules left by early Vantage builds ("Vantage - …"). Requires admin.
pub fn remove_legacy_rules() -> usize {
    let Ok(o) = hidden("netsh.exe").args(["advfirewall", "firewall", "show", "rule", "name=all"]).output() else { return 0 };
    let text = String::from_utf8_lossy(&o.stdout);
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|l| l.strip_prefix("Rule Name:").map(|n| n.trim().to_string()))
        .filter(|n| n.starts_with("Vantage - "))
        .collect();
    names.dedup();
    let mut removed = 0;
    for n in &names {
        if hidden("netsh.exe").args(["advfirewall", "firewall", "delete", "rule", &format!("name={n}")]).output().map(|o| o.status.success()).unwrap_or(false) {
            removed += 1;
        }
    }
    removed
}

/// Shared or critical Windows processes that must never be blocked.
pub fn is_protected(program: &str) -> bool {
    let p = program.to_lowercase();
    let sys = [r"\windows\system32\", r"\windows\syswow64\"];
    let critical = ["svchost.exe", "services.exe", "lsass.exe", "wininit.exe", "winlogon.exe", "csrss.exe", "smss.exe", "ntoskrnl.exe", "dwm.exe"];
    sys.iter().any(|d| p.contains(d)) && critical.iter().any(|c| p.ends_with(c))
}

/// Display name of the firewall registered with Windows Security Center (e.g. Kaspersky).
pub fn firewall_product() -> Option<String> {
    let out = powershell("Get-CimInstance -Namespace root/SecurityCenter2 -ClassName FirewallProduct -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty displayName").ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

// ---------------------------------------------------------------------------------------------
// LAN scanning

/// The physical LAN this machine sits on. Uses the physical adapter rather than the default
/// route, so a VPN doesn't turn the scan into a sweep of the tunnel's subnet.
#[derive(Clone, Debug)]
pub struct Lan {
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub mac: String,
}

fn ipv4_table<F: Fn(*mut u8, *mut u32) -> u32>(f: F) -> Vec<u8> {
    let mut size: u32 = 0;
    f(std::ptr::null_mut(), &mut size);
    if size == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; size as usize + 1024];
    size = buf.len() as u32;
    if f(buf.as_mut_ptr(), &mut size) != 0 {
        return Vec::new();
    }
    buf
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn v4(b: &[u8], o: usize) -> Ipv4Addr {
    Ipv4Addr::new(b[o], b[o + 1], b[o + 2], b[o + 3])
}

pub fn lan() -> Option<Lan> {
    let addrs = ipv4_table(|p, s| unsafe { GetIpAddrTable(p as *mut _, s, 0) });
    let routes = ipv4_table(|p, s| unsafe { GetIpForwardTable(p as *mut _, s, 0) });
    let mut best: Option<Lan> = None;
    for a in crate::net::adapters() {
        let mut ip_mask = None;
        if addrs.len() >= 4 {
            for i in 0..u32le(&addrs, 0) as usize {
                let o = 4 + i * 24; // MIB_IPADDRROW: addr, index, mask, bcast, reasm, unused, type
                if o + 24 > addrs.len() {
                    break;
                }
                let ip = v4(&addrs, o);
                if u32le(&addrs, o + 4) == a.index && !ip.is_unspecified() && !ip.is_link_local() {
                    ip_mask = Some((ip, v4(&addrs, o + 8)));
                }
            }
        }
        let Some((ip, mask)) = ip_mask else { continue };
        let mut gateway = None;
        if routes.len() >= 4 {
            for i in 0..u32le(&routes, 0) as usize {
                let o = 4 + i * 56; // MIB_IPFORWARDROW: dest, mask, policy, nexthop, ifindex, ...
                if o + 56 > routes.len() {
                    break;
                }
                if u32le(&routes, o) == 0 && u32le(&routes, o + 16) == a.index {
                    let hop = v4(&routes, o + 12);
                    if !hop.is_unspecified() {
                        gateway = Some(hop);
                    }
                }
            }
        }
        let lan = Lan { ip, mask, gateway, mac: a.mac.clone() };
        if best.is_none() || (best.as_ref().unwrap().gateway.is_none() && lan.gateway.is_some()) {
            best = Some(lan);
        }
    }
    best
}

fn as_ipaddr(ip: Ipv4Addr) -> u32 {
    u32::from_ne_bytes(ip.octets())
}

fn send_arp(ip: Ipv4Addr, src: Ipv4Addr) -> Option<String> {
    let mut mac = [0u8; 8];
    let mut len: u32 = 6;
    let rc = unsafe { SendARP(as_ipaddr(ip), as_ipaddr(src), mac.as_mut_ptr() as *mut _, &mut len) };
    if rc != 0 || len < 6 {
        return None;
    }
    Some(mac[..6].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"))
}

/// ARP-sweeps the LAN (at most the /24 around us). Returns (ip, mac) for each host that answered.
pub fn scan_subnet(lan: &Lan) -> Vec<(Ipv4Addr, String)> {
    let me = u32::from(lan.ip);
    let mask = u32::from(lan.mask) | 0xffff_ff00;
    let net = me & mask;
    let hosts = !mask;
    let targets: Vec<Ipv4Addr> = (1..hosts).map(|h| Ipv4Addr::from(net | h)).filter(|ip| *ip != lan.ip).collect();
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for chunk in targets.chunks(8) {
            let results = &results;
            s.spawn(move || {
                for ip in chunk {
                    if let Some(mac) = send_arp(*ip, lan.ip) {
                        results.lock().unwrap().push((*ip, mac));
                    }
                }
            });
        }
    });
    let mut v = results.into_inner().unwrap();
    v.sort_by_key(|(ip, _)| ip.octets());
    v
}

pub fn reverse_dns(ip: IpAddr) -> Option<String> {
    let name = dns_lookup::lookup_addr(&ip).ok()?;
    (name != ip.to_string() && !name.is_empty()).then_some(name)
}

pub fn internet_reachable() -> bool {
    ["1.1.1.1:443", "8.8.8.8:443", "9.9.9.9:443"].iter().any(|a| {
        let addr: SocketAddr = a.parse().unwrap();
        TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()
    })
}

// ---------------------------------------------------------------------------------------------
// MAC vendor lookup from the IEEE registry (downloaded once, cached on disk)

pub fn is_randomized_mac(mac: &str) -> bool {
    u8::from_str_radix(mac.get(0..2).unwrap_or("00"), 16).map(|b| b & 0x02 != 0).unwrap_or(false)
}

pub fn load_oui(dir: &Path) -> HashMap<String, String> {
    // Wireshark's "manuf" database: "60:D8:A4<TAB>Short<TAB>Long Vendor Name"
    let file: PathBuf = dir.join("manuf.txt");
    if !file.exists() {
        let script = format!(
            "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing -Uri 'https://www.wireshark.org/download/automated/data/manuf' -OutFile '{}'",
            file.display().to_string().replace('\'', "''")
        );
        let _ = powershell(&script);
    }
    let mut map = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(&file) {
        for line in text.lines() {
            let mut parts = line.split('\t');
            let (Some(prefix), Some(short)) = (parts.next(), parts.next()) else { continue };
            let prefix = prefix.trim();
            if prefix.len() == 8 {
                let vendor = parts.next().unwrap_or(short).trim();
                let vendor = if vendor.is_empty() { short.trim() } else { vendor };
                map.insert(prefix.to_lowercase(), vendor.to_string());
            }
        }
    }
    map
}
