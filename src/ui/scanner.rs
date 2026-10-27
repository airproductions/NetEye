//! Network scanner: devices on the local network found by ARP sweeps.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::theme::{self, c, Glyph};
use super::Action;
use crate::monitor::Shared;
use crate::store::DeviceRecord;

const ROW_H: f32 = 64.0;

#[derive(Default)]
pub struct ScannerPage {
    search: String,
}

fn kind(d: &DeviceRecord, gateway: Option<&str>, local: Option<&str>) -> (Glyph, &'static str) {
    let vendor = d.vendor.clone().unwrap_or_default().to_lowercase();
    let name = d.name.clone().unwrap_or_default().to_lowercase();
    if Some(d.ip.as_str()) == gateway {
        return (Glyph::Router, "Router");
    }
    if Some(d.ip.as_str()) == local {
        return (Glyph::Computer, "This computer");
    }
    let has = |words: &[&str]| words.iter().any(|w| vendor.contains(w) || name.contains(w));
    if has(&["iphone", "android", "galaxy", "pixel", "phone"]) || vendor.contains("randomized") {
        (Glyph::Phone, "Phone")
    } else if has(&["tv", "roku", "chromecast", "streamer", "sonos", "partnertv", "fire", "sei robotics"]) {
        (Glyph::Tv, "Media")
    } else if has(&["access point", "router", "tp-link", "netgear", "ubiquiti", "eero", "arris", "vantiva", "technicolor", "sagemcom", "mikrotik"]) {
        (Glyph::Router, "Network")
    } else if has(&["desktop", "laptop", "intel", "dell", "lenovo", "hewlett", "gigabyte", "asustek", "micro-star", "apple"]) {
        (Glyph::Computer, "Computer")
    } else {
        (Glyph::Device, "Device")
    }
}

impl ScannerPage {
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, actions: &mut Vec<Action>) {
        let full = ui.max_rect();
        let (bar, _) = ui.allocate_exact_size(Vec2::new(full.width(), 72.0), Sense::hover());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, c::panel());
        let mut b = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(24.0, 16.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let adapter = s.adapters.first().map(|a| format!("{}  ·  {}", a.kind, a.description)).unwrap_or_else(|| "No active adapter".into());
        theme::pill(&mut b, &adapter, Some(Glyph::Scanner), false);
        b.add_space(6.0);
        if s.scanning {
            b.add(egui::Spinner::new().size(18.0).color(c::accent()));
            theme::label(&mut b, "Scanning your network…", theme::regular(13.5), c::muted());
        } else {
            if theme::primary_button(&mut b, "Scan").clicked() {
                actions.push(Action::Scan);
                s.scanning = true;
            }
            if let Some(t) = s.store.data.last_scan {
                theme::label(&mut b, format!("Scanned: {}", theme::date_time(t)), theme::regular(13.5), c::muted());
            }
        }
        b.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let auto = s.store.data.settings.auto_scan;
            if theme::switch(ui, auto).on_hover_text("Rescan every 5 minutes and alert on new devices").clicked() {
                s.store.data.settings.auto_scan = !auto;
                s.store.mark_dirty();
            }
            theme::label(ui, "Auto-scan", theme::semibold(13.5), c::text());
            ui.add_space(14.0);
            theme::search_box(ui, &mut self.search, 300.0, "Search devices");
        });

        let x0 = full.left() + 16.0;
        let w = full.width() - 32.0;
        let col = |f: f32| x0 + w * f;
        let cols = [col(0.0) + 6.0, col(0.035), col(0.10), col(0.27), col(0.46), col(0.56), col(0.69), col(0.85)];
        let headers = ["", "Type", "Name", "Description", "IP", "MAC address", "Last seen", "First seen"];
        let (hr, _) = ui.allocate_exact_size(Vec2::new(full.width(), 34.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(hr, CornerRadius::ZERO, c::panel());
        for (x, h) in cols.iter().zip(headers) {
            p.text(Pos2::new(*x, hr.center().y), Align2::LEFT_CENTER, h, theme::regular(13.0), c::muted());
        }
        p.circle_filled(Pos2::new(cols[0] + 6.0, hr.center().y), 4.0, c::good());
        p.line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, c::border()));

        let q = self.search.to_lowercase();
        let mut devices: Vec<DeviceRecord> = s
            .store
            .data
            .devices
            .values()
            .filter(|d| q.is_empty() || format!("{} {} {} {}", d.name.clone().unwrap_or_default(), d.vendor.clone().unwrap_or_default(), d.ip, d.mac).to_lowercase().contains(&q))
            .cloned()
            .collect();
        devices.sort_by_key(|d| (!s.online.contains(&d.mac), d.ip.split('.').map(|o| o.parse::<u32>().unwrap_or(0)).fold(0u32, |a, o| a * 256 + o)));

        if devices.is_empty() {
            let p = ui.painter_at(full);
            theme::empty_state(&p, full.center(), if s.scanning { "Looking for devices…" } else { "No devices found yet. Press Scan." });
            return;
        }
        let gw = s.gateway.clone();
        let me = s.local_ip.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, devices.len(), |ui, range| {
            for i in range {
                let d = &devices[i];
                let (r, resp) = ui.allocate_exact_size(Vec2::new(full.width(), ROW_H), Sense::hover());
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(r, CornerRadius::ZERO, c::hover());
                } else if i % 2 == 0 {
                    p.rect_filled(r, CornerRadius::ZERO, c::row_alt());
                }
                let y = r.center().y;
                let online = s.online.contains(&d.mac);
                if online {
                    p.circle_filled(Pos2::new(cols[0] + 6.0, y), 4.0, c::good());
                } else {
                    p.circle_stroke(Pos2::new(cols[0] + 6.0, y), 3.5, Stroke::new(1.2_f32, c::faint()));
                }
                let (g, kname) = kind(d, gw.as_deref(), me.as_deref());
                let ir = Rect::from_center_size(Pos2::new(cols[1] + 16.0, y), Vec2::splat(30.0));
                p.circle_stroke(ir.center(), 17.0, Stroke::new(1.0_f32, c::border()));
                theme::glyph(p, ir.shrink(5.0), g, if online { c::text() } else { c::muted() });
                ui.interact(ir, ui.id().with(("kind", i)), Sense::hover()).on_hover_text(kname);
                let p = ui.painter();
                let tc = if online { c::text() } else { c::muted() };
                let name = d.name.clone().unwrap_or_else(|| kname.to_string());
                theme::text(p, Pos2::new(cols[2], y), &name, theme::semibold(14.0), tc, cols[3] - cols[2] - 12.0);
                theme::text(p, Pos2::new(cols[3], y), d.vendor.as_deref().unwrap_or("Unknown vendor"), theme::regular(13.5), c::muted(), cols[4] - cols[3] - 12.0);
                theme::text(p, Pos2::new(cols[4], y), &d.ip, theme::regular(13.5), tc, cols[5] - cols[4] - 8.0);
                theme::text(p, Pos2::new(cols[5], y), &d.mac.to_uppercase(), theme::regular(13.5), tc, cols[6] - cols[5] - 8.0);
                theme::text(p, Pos2::new(cols[6], y), &if online { "Now".to_string() } else { theme::date_time(d.last_seen) }, theme::regular(13.5), c::muted(), cols[7] - cols[6] - 8.0);
                theme::text(p, Pos2::new(cols[7], y), &theme::date_time(d.first_seen), theme::regular(13.5), c::muted(), full.right() - cols[7] - 8.0);
            }
        });
    }
}
