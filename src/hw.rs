//! Hardware resource sampling: system-wide counters via PDH, per-process CPU / memory / disk
//! via the process APIs, per-process GPU via the "GPU Engine" PDH counter set.

use std::collections::HashMap;
use std::ptr::null_mut;
use std::time::Instant;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    GetProcessIoCounters, GetProcessTimes, OpenProcess, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::net::wide;

const PDH_MORE_DATA: u32 = 0x800007D2;
const PDH_FMT_NOCAP100: u32 = 0x0000_8000;

#[derive(Clone, Copy, Default, Debug)]
pub struct ProcSample {
    pub cpu: f32,      // percent of whole machine
    pub gpu: f32,      // percent
    pub memory: u64,   // working set bytes
    pub disk: f64,     // bytes/sec (read + write)
}

#[derive(Clone, Copy, Default, Debug)]
pub struct SystemSample {
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_read: f64,
    pub disk_write: f64,
    pub gpu: f32,
    pub gpu_mem: u64,
}

struct Counter(isize);

pub struct Sampler {
    query: isize,
    cpu: Option<Counter>,
    disk_read: Option<Counter>,
    disk_write: Option<Counter>,
    gpu_engine: Option<Counter>,
    gpu_mem: Option<Counter>,
    prev: HashMap<u32, (u64, u64)>, // pid -> (cpu 100ns ticks, io bytes)
    last: Instant,
    ncpu: f64,
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

impl Sampler {
    pub fn new() -> Self {
        let mut query: isize = 0;
        unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) };
        let add = |path: &str| -> Option<Counter> {
            if query == 0 {
                return None;
            }
            let mut c: isize = 0;
            let w = wide(path);
            (unsafe { PdhAddEnglishCounterW(query, w.as_ptr(), 0, &mut c) } == 0).then_some(Counter(c))
        };
        let cpu = add("\\Processor Information(_Total)\\% Processor Utility").or_else(|| add("\\Processor(_Total)\\% Processor Time"));
        let s = Self {
            cpu,
            disk_read: add("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec"),
            disk_write: add("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec"),
            gpu_engine: add("\\GPU Engine(*)\\Utilization Percentage"),
            gpu_mem: add("\\GPU Adapter Memory(*)\\Dedicated Usage"),
            query,
            prev: HashMap::new(),
            last: Instant::now(),
            ncpu: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64,
        };
        if s.query != 0 {
            unsafe { PdhCollectQueryData(s.query) };
        }
        s
    }

    fn value(&self, c: &Option<Counter>) -> f64 {
        let Some(c) = c else { return 0.0 };
        unsafe {
            let mut v: PDH_FMT_COUNTERVALUE = std::mem::zeroed();
            if PdhGetFormattedCounterValue(c.0, PDH_FMT_DOUBLE | PDH_FMT_NOCAP100, null_mut(), &mut v) == 0 {
                v.Anonymous.doubleValue
            } else {
                0.0
            }
        }
    }

    fn array(&self, c: &Option<Counter>) -> Vec<(String, f64)> {
        let Some(c) = c else { return Vec::new() };
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            let rc = PdhGetFormattedCounterArrayW(c.0, PDH_FMT_DOUBLE, &mut size, &mut count, null_mut());
            if rc != PDH_MORE_DATA || size == 0 {
                return Vec::new();
            }
            let mut buf = vec![0u8; size as usize + 64];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(c.0, PDH_FMT_DOUBLE, &mut size, &mut count, items) != 0 {
                return Vec::new();
            }
            (0..count as usize)
                .map(|i| {
                    let it = &*items.add(i);
                    let mut len = 0;
                    while *it.szName.add(len) != 0 {
                        len += 1;
                    }
                    let name = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, len));
                    (name, it.FmtValue.Anonymous.doubleValue)
                })
                .collect()
        }
    }

    /// Samples everything. `pids` are the processes to measure individually.
    pub fn sample(&mut self, pids: &[u32]) -> (SystemSample, HashMap<u32, ProcSample>) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().max(0.001);
        self.last = now;
        if self.query != 0 {
            unsafe { PdhCollectQueryData(self.query) };
        }

        let mut sys = SystemSample {
            cpu: self.value(&self.cpu).clamp(0.0, 100.0) as f32,
            disk_read: self.value(&self.disk_read),
            disk_write: self.value(&self.disk_write),
            ..Default::default()
        };
        unsafe {
            let mut m: MEMORYSTATUSEX = std::mem::zeroed();
            m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut m) != 0 {
                sys.mem_total = m.ullTotalPhys;
                sys.mem_used = m.ullTotalPhys - m.ullAvailPhys;
            }
        }
        sys.gpu_mem = self.array(&self.gpu_mem).iter().map(|(_, v)| *v as u64).sum();

        // GPU: per pid, sum per engine type, then take the busiest engine type (Task Manager's method).
        let mut per_pid: HashMap<u32, HashMap<String, f64>> = HashMap::new();
        let mut per_engine: HashMap<String, f64> = HashMap::new();
        for (name, v) in self.array(&self.gpu_engine) {
            let pid = name.split('_').nth(1).and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
            let eng = name.rsplit("engtype_").next().unwrap_or("").to_string();
            *per_pid.entry(pid).or_default().entry(eng.clone()).or_default() += v;
            *per_engine.entry(eng).or_default() += v;
        }
        sys.gpu = per_engine.values().cloned().fold(0.0, f64::max).min(100.0) as f32;

        let mut procs = HashMap::with_capacity(pids.len());
        let mut next_prev = HashMap::with_capacity(pids.len());
        for &pid in pids {
            if pid == 0 {
                continue;
            }
            unsafe {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if h.is_null() {
                    continue;
                }
                let mut s = ProcSample::default();
                let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = std::mem::zeroed();
                let mut cpu_ticks = 0;
                if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0 {
                    cpu_ticks = ft(k) + ft(u);
                }
                let mut io: IO_COUNTERS = std::mem::zeroed();
                let mut io_bytes = 0;
                if GetProcessIoCounters(h, &mut io) != 0 {
                    io_bytes = io.ReadTransferCount + io.WriteTransferCount;
                }
                let mut mc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                mc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                if K32GetProcessMemoryInfo(h, &mut mc, mc.cb) != 0 {
                    s.memory = mc.WorkingSetSize as u64;
                }
                CloseHandle(h);
                if let Some(&(pc, pio)) = self.prev.get(&pid) {
                    let dcpu = cpu_ticks.saturating_sub(pc) as f64 / 1e7; // seconds of CPU
                    s.cpu = ((dcpu / dt / self.ncpu) * 100.0).clamp(0.0, 100.0) as f32;
                    s.disk = io_bytes.saturating_sub(pio) as f64 / dt;
                }
                if let Some(engines) = per_pid.get(&pid) {
                    s.gpu = engines.values().cloned().fold(0.0, f64::max).min(100.0) as f32;
                }
                next_prev.insert(pid, (cpu_ticks, io_bytes));
                procs.insert(pid, s);
            }
        }
        self.prev = next_prev;
        (sys, procs)
    }
}
