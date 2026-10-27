//! Network scanner: devices on the local network found by ARP sweeps.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::sort::{self, SortSpec};
use super::theme::{self, c, Glyph};
use super::Action;
use crate::monitor::Shared;
use crate::store::DeviceRecord;

const ROW_H: f32 = 64.0;
/// Online devices first, then by IP.
const DEFAULT_SORT: SortSpec = SortSpec { col: 0, desc: false };
const COLS: u8 = 8;

/// One scanner table row with the values the columns sort on.
struct DevRow {
    d: DeviceRecord,
    online: bool,
    kind: &'static str,
    name: String,
}

fn sort_devices(rows: &mut [DevRow], spec: SortSpec) {
    sort::apply(
        rows,
        spec,
        |a, b, col| match col {
            0 => (!a.online).cmp(&!b.online).then_with(|| sort::cmp_ip(&a.d.ip, &b.d.ip)),
            1 => sort::cmp_text(a.kind, b.kind),
            2 => sort::cmp_text(&a.name, &b.name),
            3 => sort::cmp_text(a.d.vendor.as_deref().unwrap_or(""), b.d.vendor.as_deref().unwrap_or("")),
            4 => sort::cmp_ip(&a.d.ip, &b.d.ip),
            5 => sort::cmp_text(&a.d.mac, &b.d.mac),
            // Online devices read "Now", so they count as the most recent.
            6 => (a.online, a.d.last_seen).cmp(&(b.online, b.d.last_seen)),
            7 => a.d.first_seen.cmp(&b.d.first_seen),
            _ => std::cmp::Ordering::Equal,
        },
        |r| r.name.as_str(),
    );
}

#[derive(Default)]
pub struct ScannerPage {
    search: String,
    /// (mac, text) of the device being renamed.
    editing: Option<(String, String)>,
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
        // which network to scan: Automatic, or one adapter's subnet
        let mut options = vec!["Automatic network".to_string()];
        options.extend(s.lans.iter().map(|l| l.label()));
        let pick = &s.store.data.settings.scan_adapter;
        let mut sel = s.lans.iter().position(|l| &l.mac == pick).map_or(0, |i| i + 1);
        if !pick.is_empty() && sel == 0 {
            // the saved adapter is gone (unplugged, Wi-Fi off): say so instead of silently switching
            options.push("Saved network (not connected)".into());
            sel = options.len() - 1;
        }
        let refs: Vec<&str> = options.iter().map(String::as_str).collect();
        theme::glyph(b.painter(), Rect::from_center_size(b.cursor().min + Vec2::new(9.0, 18.0), Vec2::splat(16.0)), Glyph::Scanner, c::muted());
        b.add_space(22.0);
        let before = sel;
        if theme::dropdown(&mut b, "scan-network", &refs, &mut sel) && sel != before {
            let mac = if sel == 0 { String::new() } else { s.lans.get(sel - 1).map(|l| l.mac.clone()).unwrap_or_else(|| pick.clone()) };
            s.store.data.settings.scan_adapter = mac;
            s.store.mark_dirty();
        }
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

        let mut spec = s.store.data.settings.sort_for("scanner", DEFAULT_SORT);
        if spec.col >= COLS {
            spec = DEFAULT_SORT;
        }
        let x0 = full.left() + 16.0;
        let w = full.width() - 32.0;
        let col = |f: f32| x0 + w * f;
        let cols = [col(0.0) + 6.0, col(0.035), col(0.10), col(0.27), col(0.46), col(0.56), col(0.69), col(0.85)];
        let headers = ["", "Type", "Name", "Description", "IP", "MAC address", "Last seen", "First seen"];
        let (hr, _) = ui.allocate_exact_size(Vec2::new(full.width(), 34.0), Sense::hover());
        ui.painter().rect_filled(hr, CornerRadius::ZERO, c::panel());
        let mut sort_changed = false;
        for (i, h) in headers.iter().enumerate() {
            let right = if i + 1 < cols.len() { cols[i + 1] - 8.0 } else { full.right() - 8.0 };
            let rect = Rect::from_min_max(Pos2::new(cols[i], hr.top()), Pos2::new(right.max(cols[i] + 16.0), hr.bottom()));
            // The status column has no label; a dot stands in for it.
            let label = if i == 0 { "●" } else { h };
            sort_changed |= sort::header(ui, rect, label, i as u8, &mut spec, false, DEFAULT_SORT, Align2::LEFT_CENTER);
        }
        if sort_changed {
            s.store.data.settings.set_sort("scanner", spec);
            s.store.mark_dirty();
        }
        ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, c::border()));

        let q = self.search.to_lowercase();
        let gw = s.gateway.clone();
        let me = s.local_ip.clone();
        let mut devices: Vec<DevRow> = s
            .store
            .data
            .devices
            .values()
            .filter(|d| q.is_empty() || format!("{} {} {} {} {}", d.label.clone().unwrap_or_default(), d.name.clone().unwrap_or_default(), d.vendor.clone().unwrap_or_default(), d.ip, d.mac).to_lowercase().contains(&q))
            .map(|d| {
                let kname = kind(d, gw.as_deref(), me.as_deref()).1;
                DevRow { online: s.online.contains(&d.mac), kind: kname, name: d.label.clone().or(d.name.clone()).unwrap_or_else(|| kname.to_string()), d: d.clone() }
            })
            .collect();
        sort_devices(&mut devices, spec);

        if devices.is_empty() {
            let p = ui.painter_at(full);
            theme::empty_state(&p, full.center(), if s.scanning { "Looking for devices…" } else { "No devices found yet. Press Scan." });
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, devices.len(), |ui, range| {
            for i in range {
                let row = &devices[i];
                let d = &row.d;
                let (r, resp) = ui.allocate_exact_size(Vec2::new(full.width(), ROW_H), Sense::hover());
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(r, CornerRadius::ZERO, c::hover());
                } else if i % 2 == 0 {
                    p.rect_filled(r, CornerRadius::ZERO, c::row_alt());
                }
                let y = r.center().y;
                let online = row.online;
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
                let tc = if online { c::text() } else { c::muted() };
                let name = row.name.clone();
                let nr = Rect::from_min_max(Pos2::new(cols[2] - 4.0, y - 14.0), Pos2::new(cols[3] - 12.0, y + 14.0));
                match self.editing.as_mut().filter(|(mac, _)| *mac == d.mac) {
                    Some((mac, text)) => {
                        let r = ui.put(nr, egui::TextEdit::singleline(text).font(theme::semibold(14.0)).hint_text("Device name"));
                        r.request_focus();
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.editing = None;
                        } else if r.lost_focus() {
                            let label = text.trim().to_string();
                            if let Some(dev) = s.store.data.devices.get_mut(mac.as_str()) {
                                dev.label = (!label.is_empty()).then_some(label);
                                s.store.mark_dirty();
                            }
                            self.editing = None;
                        }
                    }
                    None => {
                        let p = ui.painter();
                        theme::text(p, Pos2::new(cols[2], y), &name, theme::semibold(14.0), tc, cols[3] - cols[2] - 12.0);
                        let tip = match &d.name {
                            Some(n) if d.label.is_some() => format!("Double-click to rename (discovered as {n})"),
                            _ => "Double-click to rename".to_string(),
                        };
                        if ui.interact(nr, ui.id().with(("name", i)), Sense::click()).on_hover_text(tip).double_clicked() {
                            self.editing = Some((d.mac.clone(), d.label.clone().or(d.name.clone()).unwrap_or_default()));
                        }
                    }
                }
                let p = ui.painter();
                theme::text(p, Pos2::new(cols[3], y), d.vendor.as_deref().unwrap_or("Unknown vendor"), theme::regular(13.5), c::muted(), cols[4] - cols[3] - 12.0);
                theme::text(p, Pos2::new(cols[4], y), &d.ip, theme::regular(13.5), tc, cols[5] - cols[4] - 8.0);
                theme::text(p, Pos2::new(cols[5], y), &d.mac.to_uppercase(), theme::regular(13.5), tc, cols[6] - cols[5] - 8.0);
                theme::text(p, Pos2::new(cols[6], y), &if online { "Now".to_string() } else { theme::date_time(d.last_seen) }, theme::regular(13.5), c::muted(), cols[7] - cols[6] - 8.0);
                theme::text(p, Pos2::new(cols[7], y), &theme::date_time(d.first_seen), theme::regular(13.5), c::muted(), full.right() - cols[7] - 8.0);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(online: bool, ip: &str) -> DevRow {
        DevRow { d: DeviceRecord { ip: ip.into(), mac: ip.into(), ..Default::default() }, online, kind: "Device", name: ip.into() }
    }

    #[test]
    fn scanner_default_matches_today() {
        let mut rows = vec![row(true, "192.168.1.102"), row(false, "192.168.1.5"), row(true, "192.168.1.9")];
        sort_devices(&mut rows, DEFAULT_SORT);
        let ips: Vec<&str> = rows.iter().map(|r| r.d.ip.as_str()).collect();
        assert_eq!(ips, ["192.168.1.9", "192.168.1.102", "192.168.1.5"]);
    }

    #[test]
    fn scanner_ip_column_sorts_numerically_and_reverses() {
        let mut rows = vec![row(true, "192.168.1.102"), row(false, "192.168.1.5"), row(true, "192.168.1.9")];
        sort_devices(&mut rows, SortSpec { col: 4, desc: true });
        let ips: Vec<&str> = rows.iter().map(|r| r.d.ip.as_str()).collect();
        assert_eq!(ips, ["192.168.1.102", "192.168.1.9", "192.168.1.5"]);
    }
}
