//! Authenticode check: is a file signed by a publisher Windows trusts? Either with a signature
//! inside the file, or listed in one of Windows' catalog files (how most built-in Windows
//! programs are signed). Revocation isn't checked, so this never goes online.

use std::os::windows::io::AsRawHandle;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Security::Cryptography::Catalog::{
    CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2, CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext, CryptCATAdminReleaseContext,
};
use windows_sys::Win32::Security::WinTrust::{
    WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};

use crate::net::wide;

pub fn is_signed(path: &str) -> bool {
    embedded(path) || in_catalog(path)
}

/// A valid, trusted signature inside the file itself.
fn embedded(path: &str) -> bool {
    let wpath = wide(path);
    let mut file = WINTRUST_FILE_INFO { cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32, pcwszFilePath: wpath.as_ptr(), hFile: null_mut(), pgKnownSubject: null_mut() };
    // SAFETY: every pointer in `data` points at locals that outlive both calls; the second call
    // releases the state the first one opened.
    unsafe {
        let mut data: WINTRUST_DATA = std::mem::zeroed();
        data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        data.dwUIChoice = WTD_UI_NONE;
        data.fdwRevocationChecks = WTD_REVOKE_NONE;
        data.dwUnionChoice = WTD_CHOICE_FILE;
        data.Anonymous.pFile = &mut file;
        data.dwStateAction = WTD_STATEACTION_VERIFY;
        data.dwProvFlags = WTD_REVOCATION_CHECK_NONE | WTD_CACHE_ONLY_URL_RETRIEVAL;
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let rc = WinVerifyTrust(null_mut(), &mut action, &mut data as *mut WINTRUST_DATA as *mut _);
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(null_mut(), &mut action, &mut data as *mut WINTRUST_DATA as *mut _);
        rc == 0
    }
}

/// Listed in a catalog Windows has installed (catalogs are verified when they're installed).
fn in_catalog(path: &str) -> bool {
    // newer catalogs use SHA-256, older ones SHA-1 (the default when no algorithm is named)
    let sha256 = wide("SHA256");
    for alg in [sha256.as_ptr(), null()] {
        let Ok(file) = std::fs::File::open(path) else { return false };
        let handle = file.as_raw_handle();
        // SAFETY: the admin context and catalog context are released before leaving the block;
        // `hash` is sized from the first call.
        unsafe {
            let mut admin: isize = 0;
            if CryptCATAdminAcquireContext2(&mut admin, null(), alg, null(), 0) == 0 {
                continue;
            }
            let mut size: u32 = 0;
            CryptCATAdminCalcHashFromFileHandle2(admin, handle, &mut size, null_mut(), 0);
            let mut found = false;
            if size > 0 && size <= 64 {
                let mut hash = vec![0u8; size as usize];
                if CryptCATAdminCalcHashFromFileHandle2(admin, handle, &mut size, hash.as_mut_ptr(), 0) != 0 {
                    let cat = CryptCATAdminEnumCatalogFromHash(admin, hash.as_ptr(), size, 0, null_mut());
                    if cat != 0 {
                        found = true;
                        CryptCATAdminReleaseCatalogContext(admin, cat, 0);
                    }
                }
            }
            CryptCATAdminReleaseContext(admin, 0);
            if found {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_programs_are_signed() {
        // notepad is catalog- or embedded-signed on every supported Windows
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        assert!(is_signed(&format!(r"{root}\System32\notepad.exe")));
        assert!(!is_signed(r"C:\definitely\not\here.exe"));
    }

    #[test]
    fn a_fresh_file_is_unsigned() {
        let p = std::env::temp_dir().join(format!("neteye-unsigned-{}.exe", std::process::id()));
        std::fs::write(&p, b"MZ not really a program").unwrap();
        assert!(!is_signed(&p.to_string_lossy()));
        let _ = std::fs::remove_file(p);
    }
}
