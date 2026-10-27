//! Append-only diagnostics log at %APPDATA%\Vantage\vantage.log.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static PATH: OnceLock<PathBuf> = OnceLock::new();
const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub fn init(dir: &std::path::Path) {
    let path = dir.join("vantage.log");
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("vantage.old.log"));
    }
    let _ = PATH.set(path);
    std::panic::set_hook(Box::new(|info| log(&format!("PANIC: {info}"))));
}

pub fn log(msg: &str) {
    let Some(path) = PATH.get() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {msg}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}
