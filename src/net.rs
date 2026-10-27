//! Thin wrappers over iphlpapi / kernel32 / shell32. Everything here is synchronous and cheap
//! enough to call once per second.

use std::collections::HashMap;
use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetExtendedTcpTable, GetExtendedUdpTable, GetIfTable2, GetPerTcp6ConnectionEStats,
    GetPerTcpConnectionEStats, SetPerTcp6ConnectionEStats, SetPerTcpConnectionEStats, SetTcpEntry, MIB_IF_ROW2,
    MIB_IF_TABLE2, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID, TcpConnectionEstatsData,
};
use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, SHGetFileInfoW, ShellExecuteW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO, SW_SHOWNORMAL};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynRcvd,
    Established,
    FinWait,
    CloseWait,
    Closing,
    TimeWait,
    Other,
}

impl TcpState {
    fn from(v: u32) -> Self {
        match v {
            1 => Self::Closed,
            2 => Self::Listen,
            3 => Self::SynSent,
            4 => Self::SynRcvd,
            5 => Self::Established,
            6 | 7 => Self::FinWait,
            8 => Self::CloseWait,
            9 | 10 => Self::Closing,
            11 => Self::TimeWait,
            _ => Self::Other,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Closed => "Closed",
            Self::Listen => "Listening",
            Self::SynSent => "Connecting",
            Self::SynRcvd => "Accepting",
            Self::Established => "Established",
            Self::FinWait => "Closing",
            Self::CloseWait => "Close wait",
            Self::Closing => "Closing",
            Self::TimeWait => "Time wait",
            Self::Other => "—",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Clone, Debug)]
pub struct Conn {
    pub proto: Proto,
    pub state: TcpState,
    pub local: IpAddr,
    pub local_port: u16,
    pub remote: IpAddr,
    pub remote_port: u16,
    pub pid: u32,
    /// Raw MIB_TCPROW / MIB_TCP6ROW, as needed by the EStats APIs.
    row: Vec<u8>,
}

impl Conn {
    pub fn key(&self) -> (IpAddr, u16, IpAddr, u16) {
        (self.local, self.local_port, self.remote, self.remote_port)
    }
}

fn read_table(f: impl Fn(*mut c_void, *mut u32) -> u32) -> Option<Vec<u8>> {
    let mut size: u32 = 16 * 1024;
    for _ in 0..4 {
        let mut buf = vec![0u8; size as usize];
        let rc = f(buf.as_mut_ptr() as *mut c_void, &mut size);
        if rc == 0 {
            return Some(buf);
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
        size += 4096;
    }
    None
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn port(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn v4(b: &[u8], o: usize) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(b[o], b[o + 1], b[o + 2], b[o + 3]))
}
fn v6(b: &[u8], o: usize) -> IpAddr {
    let mut a = [0u8; 16];
    a.copy_from_slice(&b[o..o + 16]);
    let ip = Ipv6Addr::from(a);
    match ip.to_ipv4_mapped() {
        Some(v4) => IpAddr::V4(v4),
        None => IpAddr::V6(ip),
    }
}

pub fn tcp_connections() -> Vec<Conn> {
    let mut out = Vec::new();
    if let Some(t) = read_table(|p, s| unsafe { GetExtendedTcpTable(p, s, 0, AF_INET, TCP_TABLE_OWNER_PID_ALL, 0) }) {
        let n = u32le(&t, 0) as usize;
        for i in 0..n {
            let o = 4 + i * 24;
            if o + 24 > t.len() {
                break;
            }
            out.push(Conn {
                proto: Proto::Tcp,
                state: TcpState::from(u32le(&t, o)),
                local: v4(&t, o + 4),
                local_port: port(&t, o + 8),
                remote: v4(&t, o + 12),
                remote_port: port(&t, o + 16),
                pid: u32le(&t, o + 20),
                row: t[o..o + 20].to_vec(),
            });
        }
    }
    if let Some(t) = read_table(|p, s| unsafe { GetExtendedTcpTable(p, s, 0, AF_INET6, TCP_TABLE_OWNER_PID_ALL, 0) }) {
        let n = u32le(&t, 0) as usize;
        for i in 0..n {
            // MIB_TCP6ROW_OWNER_PID: local[16] lscope lport remote[16] rscope rport state pid
            let o = 4 + i * 56;
            if o + 56 > t.len() {
                break;
            }
            let state = u32le(&t, o + 48);
            // MIB_TCP6ROW: State, Local[16], LScope, LPort, Remote[16], RScope, RPort
            let mut row = Vec::with_capacity(52);
            row.extend_from_slice(&state.to_le_bytes());
            row.extend_from_slice(&t[o..o + 48]);
            out.push(Conn {
                proto: Proto::Tcp,
                state: TcpState::from(state),
                local: v6(&t, o),
                local_port: port(&t, o + 20),
                remote: v6(&t, o + 24),
                remote_port: port(&t, o + 44),
                pid: u32le(&t, o + 52),
                row,
            });
        }
    }
    out
}

pub fn udp_endpoints() -> Vec<Conn> {
    let mut out = Vec::new();
    let unspec4 = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    if let Some(t) = read_table(|p, s| unsafe { GetExtendedUdpTable(p, s, 0, AF_INET, UDP_TABLE_OWNER_PID, 0) }) {
        let n = u32le(&t, 0) as usize;
        for i in 0..n {
            let o = 4 + i * 12;
            if o + 12 > t.len() {
                break;
            }
            out.push(Conn {
                proto: Proto::Udp,
                state: TcpState::Other,
                local: v4(&t, o),
                local_port: port(&t, o + 4),
                remote: unspec4,
                remote_port: 0,
                pid: u32le(&t, o + 8),
                row: Vec::new(),
            });
        }
    }
    if let Some(t) = read_table(|p, s| unsafe { GetExtendedUdpTable(p, s, 0, AF_INET6, UDP_TABLE_OWNER_PID, 0) }) {
        let n = u32le(&t, 0) as usize;
        for i in 0..n {
            let o = 4 + i * 28;
            if o + 28 > t.len() {
                break;
            }
            out.push(Conn {
                proto: Proto::Udp,
                state: TcpState::Other,
                local: v6(&t, o),
                local_port: port(&t, o + 20),
                remote: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                remote_port: 0,
                pid: u32le(&t, o + 24),
                row: Vec::new(),
            });
        }
    }
    out
}

/// Drops the program's open IPv4 TCP connections so a new block takes effect immediately
/// (WFP only filters new connections). Requires administrator.
pub fn kill_tcp_for_program(program: &str) -> usize {
    let mut killed = 0;
    for c in tcp_connections() {
        if !c.local.is_ipv4() || c.state != TcpState::Established || is_local(&c.remote) {
            continue;
        }
        if process_path(c.pid).map(|p| p.eq_ignore_ascii_case(program)).unwrap_or(false) {
            let mut row = c.row.clone();
            row[..4].copy_from_slice(&12u32.to_le_bytes()); // MIB_TCP_STATE_DELETE_TCB
            if unsafe { SetTcpEntry(row.as_ptr() as *const _) } == 0 {
                killed += 1;
            }
        }
    }
    killed
}

/// Turns on per-connection byte counting. Requires administrator.
pub fn enable_tcp_stats(c: &Conn) -> bool {
    let mut rw = [1u8];
    unsafe {
        let rc = if c.local.is_ipv4() {
            SetPerTcpConnectionEStats(c.row.as_ptr() as *const _, TcpConnectionEstatsData, rw.as_mut_ptr(), 0, 1, 0)
        } else {
            SetPerTcp6ConnectionEStats(c.row.as_ptr() as *const _, TcpConnectionEstatsData, rw.as_mut_ptr(), 0, 1, 0)
        };
        rc == 0
    }
}

/// Cumulative (in, out) payload bytes since collection was enabled.
pub fn tcp_stats(c: &Conn) -> Option<(u64, u64)> {
    let mut rod = [0u8; 96];
    let rc = unsafe {
        if c.local.is_ipv4() {
            GetPerTcpConnectionEStats(
                c.row.as_ptr() as *const _, TcpConnectionEstatsData,
                null_mut(), 0, 0, null_mut(), 0, 0, rod.as_mut_ptr(), 0, rod.len() as u32,
            )
        } else {
            GetPerTcp6ConnectionEStats(
                c.row.as_ptr() as *const _, TcpConnectionEstatsData,
                null_mut(), 0, 0, null_mut(), 0, 0, rod.as_mut_ptr(), 0, rod.len() as u32,
            )
        }
    };
    if rc != 0 {
        return None;
    }
    let out = u64::from_le_bytes(rod[0..8].try_into().unwrap());
    let inb = u64::from_le_bytes(rod[16..24].try_into().unwrap());
    Some((inb, out))
}

#[derive(Clone, Debug, Default)]
pub struct Adapter {
    pub index: u32,
    pub alias: String,
    pub description: String,
    pub kind: &'static str,
    pub speed: u64,
    pub mac: String,
    pub in_octets: u64,
    pub out_octets: u64,
}

/// Physical, connected, non-loopback, non-filter interfaces with their byte counters.
pub fn adapters() -> Vec<Adapter> {
    let mut out = Vec::new();
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = null_mut();
        if GetIfTable2(&mut table) != 0 || table.is_null() {
            return out;
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr() as *const MIB_IF_ROW2, n);
        for r in rows {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 1 != 0;
            let filter = flags & 2 != 0;
            if !hardware || filter || r.Type == 24 || r.OperStatus != 1 {
                continue;
            }
            let mac_len = (r.PhysicalAddressLength as usize).min(32);
            out.push(Adapter {
                index: r.InterfaceIndex,
                alias: from_wide(&r.Alias),
                description: from_wide(&r.Description),
                kind: match r.Type {
                    71 => "Wi-Fi",
                    6 => "Ethernet",
                    243 | 244 => "Cellular",
                    _ => "Network",
                },
                speed: r.ReceiveLinkSpeed,
                mac: r.PhysicalAddress[..mac_len].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"),
                in_octets: r.InOctets,
                out_octets: r.OutOctets,
            });
        }
        FreeMibTable(table as *const c_void);
    }
    out
}

/// pid -> executable file name for every running process.
pub fn process_names() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return map;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                map.insert(e.th32ProcessID, from_wide(&e.szExeFile));
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    map
}

pub fn process_path(pid: u32) -> Option<String> {
    if pid == 0 || pid == 4 {
        return None;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut size) != 0;
        CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..size as usize]))
    }
}

#[derive(Clone, Debug, Default)]
pub struct VersionInfo {
    pub description: Option<String>,
    pub company: Option<String>,
    pub version: Option<String>,
}

/// FileDescription, CompanyName and version from the executable's version resource.
pub fn version_info(path: &str) -> VersionInfo {
    unsafe {
        let wpath = wide(path);
        let size = GetFileVersionInfoSizeW(wpath.as_ptr(), null_mut());
        if size == 0 {
            return VersionInfo::default();
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wpath.as_ptr(), 0, size, data.as_mut_ptr() as *mut c_void) == 0 {
            return VersionInfo::default();
        }
        let mut ptr: *mut c_void = null_mut();
        let mut len: u32 = 0;
        let mut langs = vec![(0x0409u16, 0x04b0u16)];
        if VerQueryValueW(data.as_ptr() as *const c_void, wide("\\VarFileInfo\\Translation").as_ptr(), &mut ptr, &mut len) != 0
            && len >= 4
        {
            let pairs = std::slice::from_raw_parts(ptr as *const u16, (len / 2) as usize);
            langs.insert(0, (pairs[0], pairs[1]));
        }
        let query = |field: &str| -> Option<String> {
            for (lang, cp) in &langs {
                let key = wide(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\{field}"));
                let mut p: *mut c_void = null_mut();
                let mut l: u32 = 0;
                if VerQueryValueW(data.as_ptr() as *const c_void, key.as_ptr(), &mut p, &mut l) != 0 && l > 1 {
                    let s = from_wide(std::slice::from_raw_parts(p as *const u16, l as usize));
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
            None
        };
        let version = query("ProductVersion")
            .or_else(|| query("FileVersion"))
            .map(|v| v.split_whitespace().next().unwrap_or("").replace(", ", ".").replace(',', "."));
        VersionInfo { description: query("FileDescription"), company: query("CompanyName"), version }
    }
}

/// RGBA shell icon for a file, at the size Windows hands out (32 px at 100% scaling, 48 at 150%).
pub fn file_icon(path: &str) -> Option<(usize, usize, Vec<u8>)> {
    unsafe {
        let wpath = wide(path);
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let r = SHGetFileInfoW(wpath.as_ptr(), 0, &mut info, std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
        if r == 0 || info.hIcon.is_null() {
            return None;
        }
        let hicon = info.hIcon;
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(hicon, &mut ii) == 0 {
            DestroyIcon(hicon);
            return None;
        }
        // the "large" icon follows the display scale, so read its real size; reading a 48 px
        // bitmap as 32 px wide shears and crops it
        let mut bm: BITMAP = std::mem::zeroed();
        let sized = !ii.hbmColor.is_null() && GetObjectW(ii.hbmColor, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut BITMAP as *mut c_void) != 0;
        let (w, h) = if sized && (1..=256).contains(&bm.bmWidth) && (1..=256).contains(&bm.bmHeight) { (bm.bmWidth as usize, bm.bmHeight as usize) } else { (0, 0) };
        if w == 0 {
            // monochrome icon (no colour bitmap) or an odd size: let the letter tile stand in
            for b in [ii.hbmColor, ii.hbmMask] {
                if !b.is_null() {
                    DeleteObject(b);
                }
            }
            DestroyIcon(hicon);
            return None;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let dc = GetDC(null_mut());
        let mut px = vec![0u8; w * h * 4];
        let got = GetDIBits(dc, ii.hbmColor, 0, h as u32, px.as_mut_ptr() as *mut c_void, &mut bmi, DIB_RGB_COLORS);
        let mut mask = vec![0u8; w * h * 4];
        let has_mask = !ii.hbmMask.is_null()
            && GetDIBits(dc, ii.hbmMask, 0, h as u32, mask.as_mut_ptr() as *mut c_void, &mut bmi, DIB_RGB_COLORS) != 0;
        ReleaseDC(null_mut(), dc);
        if !ii.hbmColor.is_null() {
            DeleteObject(ii.hbmColor);
        }
        if !ii.hbmMask.is_null() {
            DeleteObject(ii.hbmMask);
        }
        DestroyIcon(hicon);
        if got == 0 {
            return None;
        }
        let any_alpha = px.chunks(4).any(|c| c[3] != 0);
        for (i, c) in px.chunks_mut(4).enumerate() {
            c.swap(0, 2); // BGRA -> RGBA
            if !any_alpha {
                c[3] = if has_mask && mask[i * 4] != 0 { 0 } else { 255 };
            }
        }
        Some((w, h, px))
    }
}

/// Passed to the elevated copy so it waits for this one to exit instead of deferring to it.
pub const RESTART_ARG: &str = "--restarted";

/// Relaunch this executable elevated. Returns false if the user declined UAC.
pub fn relaunch_as_admin() -> bool {
    let exe = std::env::current_exe().ok().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let (verb, file, params) = (wide("runas"), wide(&exe), wide(RESTART_ARG));
    let r = unsafe { ShellExecuteW(null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), null(), SW_SHOWNORMAL) };
    r as isize > 32
}

pub fn is_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_loopback() || a.is_unspecified(),
        IpAddr::V6(a) => a.is_loopback() || a.is_unspecified(),
    }
}

pub fn is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_private() || a.is_link_local(),
        IpAddr::V6(a) => (a.segments()[0] & 0xfe00) == 0xfc00 || (a.segments()[0] & 0xffc0) == 0xfe80,
    }
}
