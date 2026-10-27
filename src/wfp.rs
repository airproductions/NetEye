//! Per-app blocking through the Windows Filtering Platform. WFP sits underneath both Windows
//! Firewall and third-party firewalls (Kaspersky, ESET…), so these filters are enforced whichever
//! firewall product is active. Filters are persistent: they survive reboots and app restarts.
//! Every call needs administrator rights.

use std::ptr::{null, null_mut};

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0, FwpmFilterDeleteByKey0, FwpmFilterGetByKey0, FwpmFreeMemory0,
    FwpmGetAppIdFromFileName0, FwpmSubLayerAdd0, FWPM_CONDITION_ALE_APP_ID, FWPM_FILTER0, FWPM_FILTER_CONDITION0,
    FWPM_FILTER_FLAG_PERSISTENT, FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6,
    FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6, FWPM_SUBLAYER0, FWPM_SUBLAYER_FLAG_PERSISTENT,
    FWP_ACTION_BLOCK, FWP_BYTE_BLOB, FWP_BYTE_BLOB_TYPE, FWP_MATCH_EQUAL, FWP_UINT8,
};

use crate::net::wide;
use crate::system::Dir;

const RPC_C_AUTHN_WINNT: u32 = 10;
const FWP_E_ALREADY_EXISTS: u32 = 0x8032_0009;
const FWP_E_FILTER_NOT_FOUND: u32 = 0x8032_0003;
const SUBLAYER: GUID = GUID::from_u128(0x5a1c7e3b_2d4f_4b8e_9c61_0f3e7a2b9d10);

fn layers(dir: Dir) -> [(usize, GUID); 2] {
    match dir {
        Dir::Out => [(0, FWPM_LAYER_ALE_AUTH_CONNECT_V4), (1, FWPM_LAYER_ALE_AUTH_CONNECT_V6)],
        Dir::In => [(2, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4), (3, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6)],
    }
}

/// Deterministic filter key per (program, layer), so filters can be found and removed later
/// without keeping WFP's numeric ids around.
fn filter_key(program: &str, layer: usize) -> GUID {
    let fnv = |seed: u64, s: &str| {
        let mut h = seed;
        for b in s.bytes().chain([layer as u8]) {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
        h
    };
    let p = program.to_lowercase();
    let v = ((fnv(0xcbf2_9ce4_8422_2325, &p) as u128) << 64) | fnv(0x6c62_272e_07bb_0142, &p) as u128;
    GUID::from_u128((v & !0xf000_0000_0000_0000_0000u128) | 0x4000_0000_0000_0000_0000u128)
}

struct Engine(HANDLE);

impl Engine {
    fn open() -> Result<Self, String> {
        let mut h: HANDLE = null_mut();
        let rc = unsafe { FwpmEngineOpen0(null(), RPC_C_AUTHN_WINNT, null(), null(), &mut h) };
        if rc != 0 {
            return Err(format!("could not open the filtering engine (0x{rc:08x})"));
        }
        Ok(Self(h))
    }

    fn ensure_sublayer(&self) -> Result<(), String> {
        let name = wide("Vantage");
        let desc = wide("Per-app block rules created by Vantage");
        let mut sl: FWPM_SUBLAYER0 = unsafe { std::mem::zeroed() };
        sl.subLayerKey = SUBLAYER;
        sl.displayData.name = name.as_ptr() as *mut u16;
        sl.displayData.description = desc.as_ptr() as *mut u16;
        sl.flags = FWPM_SUBLAYER_FLAG_PERSISTENT;
        sl.weight = 0xffff;
        let rc = unsafe { FwpmSubLayerAdd0(self.0, &sl, null_mut()) };
        if rc != 0 && rc != FWP_E_ALREADY_EXISTS {
            return Err(format!("could not register the Vantage sublayer (0x{rc:08x})"));
        }
        Ok(())
    }

    fn block(&self, program: &str, dir: Dir) -> Result<(), String> {
        let wpath = wide(program);
        let mut app_id: *mut FWP_BYTE_BLOB = null_mut();
        let rc = unsafe { FwpmGetAppIdFromFileName0(wpath.as_ptr(), &mut app_id) };
        if rc != 0 {
            return Err(format!("could not resolve {program} (0x{rc:08x})"));
        }
        let file = std::path::Path::new(program).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        let mut result = Ok(());
        for (idx, layer) in layers(dir) {
            let key = filter_key(program, idx);
            unsafe { FwpmFilterDeleteByKey0(self.0, &key) };
            let name = wide(&format!("Vantage block {} {file}", if dir == Dir::In { "in" } else { "out" }));
            let desc = wide(program);
            let mut cond: FWPM_FILTER_CONDITION0 = unsafe { std::mem::zeroed() };
            cond.fieldKey = FWPM_CONDITION_ALE_APP_ID;
            cond.matchType = FWP_MATCH_EQUAL;
            cond.conditionValue.r#type = FWP_BYTE_BLOB_TYPE;
            cond.conditionValue.Anonymous.byteBlob = app_id;
            let mut f: FWPM_FILTER0 = unsafe { std::mem::zeroed() };
            f.filterKey = key;
            f.displayData.name = name.as_ptr() as *mut u16;
            f.displayData.description = desc.as_ptr() as *mut u16;
            f.flags = FWPM_FILTER_FLAG_PERSISTENT;
            f.layerKey = layer;
            f.subLayerKey = SUBLAYER;
            f.weight.r#type = FWP_UINT8;
            f.weight.Anonymous.uint8 = 15;
            f.numFilterConditions = 1;
            f.filterCondition = &mut cond;
            f.action.r#type = FWP_ACTION_BLOCK;
            let mut id = 0u64;
            let rc = unsafe { FwpmFilterAdd0(self.0, &f, null_mut(), &mut id) };
            if rc != 0 && rc != FWP_E_ALREADY_EXISTS {
                result = Err(format!("could not add block filter for {file} (0x{rc:08x})"));
            }
        }
        unsafe { FwpmFreeMemory0(&mut app_id as *mut _ as *mut *mut core::ffi::c_void) };
        result
    }

    fn unblock(&self, program: &str, dir: Dir) -> Result<(), String> {
        for (idx, _) in layers(dir) {
            let rc = unsafe { FwpmFilterDeleteByKey0(self.0, &filter_key(program, idx)) };
            if rc != 0 && rc != FWP_E_FILTER_NOT_FOUND {
                return Err(format!("could not remove block filter (0x{rc:08x})"));
            }
        }
        Ok(())
    }

    fn is_blocked(&self, program: &str, dir: Dir) -> Option<bool> {
        let mut f: *mut FWPM_FILTER0 = null_mut();
        let rc = unsafe { FwpmFilterGetByKey0(self.0, &filter_key(program, layers(dir)[0].0), &mut f) };
        match rc {
            0 => {
                unsafe { FwpmFreeMemory0(&mut f as *mut _ as *mut *mut core::ffi::c_void) };
                Some(true)
            }
            FWP_E_FILTER_NOT_FOUND => Some(false),
            _ => None,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { FwpmEngineClose0(self.0) };
    }
}

/// Applies (program, direction, block?) changes. Requires administrator.
pub fn apply(changes: &[(String, Dir, bool)], close_open: bool) -> Result<(), String> {
    let engine = Engine::open()?;
    engine.ensure_sublayer()?;
    for (program, dir, block) in changes {
        let r = if *block { engine.block(program, *dir) } else { engine.unblock(program, *dir) };
        if r.is_ok() && !*block {
            crate::diag::log(&format!("wfp verify after unblock {:?}: still blocked = {:?}", dir, engine.is_blocked(program, *dir)));
        }
        if r.is_ok() && *block && *dir == Dir::Out && close_open {
            let n = crate::net::kill_tcp_for_program(program);
            crate::diag::log(&format!("closed {n} open connections for {program}"));
        }
        crate::diag::log(&format!("wfp {} {:?} {program} -> {:?}", if *block { "block" } else { "unblock" }, dir, r));
        r?;
    }
    Ok(())
}

/// Current block state for each program, or None when the engine can't be queried (not elevated).
pub fn query(programs: &[String]) -> Option<Vec<(bool, bool)>> {
    let engine = Engine::open().ok()?;
    programs.iter().map(|p| Some((engine.is_blocked(p, Dir::In)?, engine.is_blocked(p, Dir::Out)?))).collect()
}
