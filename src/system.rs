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
    let body: String = changes
        .iter()
        .map(|(p, d, b)| format!("{}\t{}\t{}\n", if *b { 1 } else { 0 }, if *d == Dir::In { "in" } else { "out" }, p))
        .collect();
    elevated_batch(&format!("#close_open={}\n{body}", close_open as u8), changes.len())
}

/// Turns the "Block All" filter (every outgoing connection except loopback) on or off.
pub fn set_block_all(on: bool, admin: bool) -> Result<(), String> {
    if admin {
        return crate::wfp::block_all(on);
    }
    elevated_batch(&format!("#close_open=0\n#block_all={}\n", on as u8), 1)
}

/// Runs `neteye.exe --fw-batch` elevated on a batch file holding `body` (one UAC prompt).
fn elevated_batch(body: &str, count: usize) -> Result<(), String> {
    let batch = std::env::temp_dir().join(format!("neteye-fw-{}.txt", std::process::id()));
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
    crate::diag::log(&format!("elevated firewall batch ({count} changes) -> {}", out.status));
    if String::from_utf8_lossy(&out.stdout).contains("cancelled") {
        return Err("Administrator permission was declined.".into());
    }
    if !out.status.success() {
        return Err("The filtering engine rejected the change (see neteye.log).".into());
    }
    Ok(())
}

/// Entry point for `neteye.exe --fw-batch <file>` (runs elevated).
pub fn run_firewall_batch(file: &str) -> i32 {
    let Ok(text) = std::fs::read_to_string(file.trim_matches('"')) else { return 2 };
    let close_open = !text.contains("#close_open=0");
    for (tag, on) in [("#block_all=1", true), ("#block_all=0", false)] {
        if text.contains(tag) {
            if let Err(e) = crate::wfp::block_all(on) {
                crate::diag::log(&format!("block-all batch failed: {e}"));
                return 1;
            }
        }
    }
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
    // NetEye itself: blocking it would cut its own checks off
    if std::env::current_exe().is_ok_and(|me| me.to_string_lossy().to_lowercase() == p) {
        return true;
    }
    // Microsoft Defender's engine and services
    let defender = ["msmpeng.exe", "nissrv.exe", "mpdefendercoreservice.exe", "securityhealthservice.exe", "mpcmdrun.exe"];
    if p.contains(r"\windows defender\") && defender.iter().any(|d| p.ends_with(d)) {
        return true;
    }
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
    /// The adapter's description and kind, for the network picker.
    pub name: String,
    pub kind: &'static str,
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

/// The network to scan: the adapter with MAC `pick` when it has one, else the best guess
/// (the first adapter with a default gateway).
pub fn lan_for(pick: &str) -> Option<Lan> {
    let all = lans();
    if !pick.is_empty() {
        if let Some(l) = all.iter().find(|l| l.mac == pick) {
            return Some(l.clone());
        }
    }
    best_lan(all)
}

pub fn lan() -> Option<Lan> {
    best_lan(lans())
}

fn best_lan(all: Vec<Lan>) -> Option<Lan> {
    let mut best: Option<Lan> = None;
    for lan in all {
        if best.is_none() || (best.as_ref().unwrap().gateway.is_none() && lan.gateway.is_some()) {
            best = Some(lan);
        }
    }
    best
}

/// Every physical adapter with an IPv4 network, for the scanner's network picker.
pub fn lans() -> Vec<Lan> {
    let addrs = ipv4_table(|p, s| unsafe { GetIpAddrTable(p as *mut _, s, 0) });
    let routes = ipv4_table(|p, s| unsafe { GetIpForwardTable(p as *mut _, s, 0) });
    let mut out = Vec::new();
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
        out.push(Lan { ip, mask, gateway, mac: a.mac.clone(), name: a.description.clone(), kind: a.kind });
    }
    out
}

impl Lan {
    /// "Wi-Fi · Intel AX211 · 192.168.1.0/24"
    pub fn label(&self) -> String {
        let bits = u32::from(self.mask).count_ones();
        let net = Ipv4Addr::from(u32::from(self.ip) & u32::from(self.mask));
        format!("{} · {} · {net}/{bits}", self.kind, self.name)
    }
}

fn as_ipaddr(ip: Ipv4Addr) -> u32 {
    u32::from_ne_bytes(ip.octets())
}

/// MAC address that answers ARP for `ip`, asked from our LAN address.
pub fn arp(lan: &Lan, ip: Ipv4Addr) -> Option<String> {
    send_arp(ip, lan.ip)
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

/// DNS servers Windows is configured to use, in order.
pub fn dns_servers() -> Vec<String> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetNetworkParams, FIXED_INFO_W2KSP1, IP_ADDR_STRING};
    let mut size: u32 = 0;
    unsafe { GetNetworkParams(std::ptr::null_mut(), &mut size) };
    if size == 0 {
        return Vec::new();
    }
    // u64 backing keeps the struct's pointers aligned
    let mut buf = vec![0u64; size as usize / 8 + 1];
    if unsafe { GetNetworkParams(buf.as_mut_ptr() as *mut FIXED_INFO_W2KSP1, &mut size) } != 0 {
        return Vec::new();
    }
    let info = unsafe { &*(buf.as_ptr() as *const FIXED_INFO_W2KSP1) };
    let mut out = Vec::new();
    let mut node: *const IP_ADDR_STRING = &info.DnsServerList;
    while !node.is_null() {
        let n = unsafe { &*node };
        let raw: Vec<u8> = n.IpAddress.String.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
        let ip = String::from_utf8_lossy(&raw).to_string();
        if !ip.is_empty() {
            out.push(ip);
        }
        node = n.Next;
    }
    out
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const INET_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

fn reg_string(key: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let (k, v) = (crate::net::wide(key), crate::net::wide(value));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut size) };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Windows Firewall is switched off on at least one profile (domain, private or public).
pub fn windows_firewall_off() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    const BASE: &str = r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\";
    ["DomainProfile", "StandardProfile", "PublicProfile"].iter().any(|p| {
        let (k, v) = (crate::net::wide(&format!("{BASE}{p}")), crate::net::wide("EnableFirewall"));
        let mut d: u32 = 1;
        let mut size = 4u32;
        let rc = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, k.as_ptr(), v.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut d as *mut u32 as *mut _, &mut size) };
        rc == 0 && d == 0
    })
}

/// SHA-256 of the hosts file, or None when it can't be read.
pub fn hosts_file_hash() -> Option<String> {
    use sha2::{Digest, Sha256};
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let bytes = std::fs::read(root.join(r"System32\drivers\etc\hosts")).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes one alert to the Windows Event Log (Application log, source "NetEye").
pub fn write_event_log(msg: &str, warning: bool) {
    use windows_sys::Win32::System::EventLog::{DeregisterEventSource, RegisterEventSourceW, ReportEventW, EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE};
    let (src, text) = (crate::net::wide("NetEye"), crate::net::wide(msg));
    unsafe {
        let h = RegisterEventSourceW(std::ptr::null(), src.as_ptr());
        if h.is_null() {
            return;
        }
        let strings = [text.as_ptr()];
        let kind = if warning { EVENTLOG_WARNING_TYPE } else { EVENTLOG_INFORMATION_TYPE };
        ReportEventW(h, kind, 0, 1000, std::ptr::null_mut(), 1, 0, strings.as_ptr(), std::ptr::null());
        DeregisterEventSource(h);
    }
}

fn reg_dword(key: &str, value: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let (k, v) = (crate::net::wide(key), crate::net::wide(value));
    let mut d: u32 = 0;
    let mut size = 4u32;
    let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut d as *mut u32 as *mut _, &mut size) };
    (rc == 0).then_some(d)
}

/// The current user's proxy configuration as one comparable line ("off" when none is set).
pub fn proxy_settings() -> String {
    let mut parts = Vec::new();
    if reg_dword(INET_KEY, "ProxyEnable").unwrap_or(0) != 0 {
        parts.push(format!("proxy {}", reg_string(INET_KEY, "ProxyServer").unwrap_or_default()));
    }
    if let Some(pac) = reg_string(INET_KEY, "AutoConfigURL").filter(|s| !s.is_empty()) {
        parts.push(format!("script {pac}"));
    }
    if parts.is_empty() { "off".into() } else { parts.join(", ") }
}

/// Adds or removes NetEye from the current user's startup programs.
pub fn set_run_on_startup(on: bool) -> bool {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let (k, v) = (crate::net::wide(RUN_KEY), crate::net::wide("NetEye"));
    // drop the entry the app registered under its old name
    let legacy = crate::net::wide("Vantage");
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), legacy.as_ptr()) };
    if !on {
        let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr()) };
        return rc == 0 || rc == 2; // ERROR_FILE_NOT_FOUND: already gone
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let cmd = crate::net::wide(&format!("\"{}\"", exe.display()));
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), REG_SZ, cmd.as_ptr() as *const _, (cmd.len() * 2) as u32) == 0 }
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

#[cfg(test)]
mod lan_tests {
    use super::*;

    #[test]
    fn defender_and_core_processes_are_protected() {
        assert!(is_protected(r"C:\ProgramData\Microsoft\Windows Defender\Platform\4.18.2\MsMpEng.exe"));
        assert!(is_protected(r"C:\Windows\System32\svchost.exe"));
        assert!(!is_protected(r"C:\Program Files\Mozilla Firefox\firefox.exe"));
        assert!(!is_protected(r"C:\Users\x\Downloads\MsMpEng.exe"), "only Defender's own folder");
    }

    #[test]
    fn lan_label_shows_subnet() {
        let l = Lan { ip: Ipv4Addr::new(192, 168, 1, 23), mask: Ipv4Addr::new(255, 255, 255, 0), gateway: None, mac: "m".into(), name: "Intel AX211".into(), kind: "Wi-Fi" };
        assert_eq!(l.label(), "Wi-Fi · Intel AX211 · 192.168.1.0/24");
    }
}
