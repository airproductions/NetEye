#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diag;
mod geo;
mod hw;
mod monitor;
mod net;
mod ports;
mod sign;
mod store;
mod system;
mod ui;
mod wfp;

use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--fw-batch" {
        let _store = store::Store::load(); // initialises the diagnostics log
        std::process::exit(system::run_firewall_batch(&args[2]));
    }
    // after "Restart as administrator" the old copy may still hold the mutex for a moment
    let restarting = args.iter().any(|a| a == net::RESTART_ARG);
    if !single_instance(restarting) {
        return Ok(());
    }
    let icon = egui::IconData { rgba: ui::theme::logo_rgba(64), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NetEye")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1100.0, 680.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native("NetEye", options, Box::new(|cc| Ok(Box::new(ui::NetEyeApp::new(cc)))))
}

/// Returns false (after focusing the running window) when NetEye is already running.
/// With `wait`, first gives the previous copy up to 10 s to exit (elevated restart).
fn single_instance(wait: bool) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
    let name = net::wide(r"Local\NetEyeNetworkMonitor");
    unsafe {
        // Leaked on purpose: the mutex must live as long as the process.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if GetLastError() != ERROR_ALREADY_EXISTS {
                return true;
            }
            if !wait || std::time::Instant::now() >= deadline {
                break;
            }
            // let go of the old copy's mutex so it disappears when that copy exits, then retry
            CloseHandle(mutex);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let title = net::wide("NetEye");
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
    false
}
