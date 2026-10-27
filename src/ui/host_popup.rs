//! Usage view popups: a host (region, IP with Copy / Check, per-app ↓/↑) and a country (its hosts).

use std::collections::HashMap;
use std::net::IpAddr;

use eframe::egui::{self, Align2, Context, CornerRadius, Id, Key, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use super::app_popup;
use super::flags;
use super::theme::{self, c, Glyph, Icons};
use super::traffic_data::{Agg, Row};
use crate::geo;
use crate::monitor::Shared;

const HOST_SIZE: Vec2 = Vec2::new(440.0, 300.0);
const COUNTRY_SIZE: Vec2 = Vec2::new(560.0, 340.0);
const PAD: f32 = 14.0;
const ROW_H: f32 = 30.0;
const NUM_W: f32 = 84.0;
const MENU_W: f32 = 200.0;

#[derive(Clone, PartialEq, Debug)]
pub enum Detail {
    /// A Hosts row, by its `Row.key`.
    Host(String),
    /// A Countries row, by country code.
    Country(String),
}

#[derive(Clone)]
struct Open {
    what: Detail,
    at: Pos2,
    /// The IP menu's ⋯ button while that menu is open.
    menu: Option<Rect>,
}

fn id() -> Id {
    Id::new("usage_view.detail")
}

/// Opens the popup for `what` next to `at`.
pub fn open(ctx: &Context, what: Detail, at: Pos2) {
    ctx.data_mut(|d| d.insert_temp(id(), Open { what, at, menu: None }));
}

pub fn close(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Open>(id()));
}

/// The IPs a host name was seen at: the host itself when it is an address, else every IP whose
/// resolved name is the host, in address order.
pub fn ips_for(host: &str, names: &HashMap<IpAddr, String>) -> Vec<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return vec![ip];
    }
    let mut v: Vec<IpAddr> = names.iter().filter(|(_, n)| n.as_str() == host).map(|(ip, _)| *ip).collect();
    v.sort();
    v
}

pub fn vt_ip_url(ip: &IpAddr) -> String {
    format!("https://www.virustotal.com/gui/ip-address/{ip}")
}

/// Hosts rows in one country, in `Agg` order.
pub fn hosts_in<'a>(agg: &'a Agg, cc: &str) -> Vec<&'a Row> {
    agg.hosts.iter().filter(|r| r.cc.as_deref() == Some(cc)).collect()
}

fn frame(ui: &mut Ui, size: Vec2, add: impl FnOnce(&mut Ui)) {
    let shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: c::shadow() };
    egui::Frame::NONE
        .fill(c::panel())
        .stroke(Stroke::new(1.0_f32, c::border()))
        .corner_radius(CornerRadius::same(10))
        .shadow(shadow)
        .inner_margin(egui::Margin::same(PAD as i8))
        .show(ui, |ui| {
            ui.set_width(size.x - 2.0 * PAD - 2.0);
            ui.set_height(size.y - 2.0 * PAD - 2.0);
            add(ui);
        });
}

fn flag_or_globe(ui: &Ui, center: Pos2, cc: Option<&str>) {
    match cc {
        Some(cc) => flags::draw(ui, Rect::from_center_size(center, flags::SIZE), cc),
        None => theme::glyph(ui.painter(), Rect::from_center_size(center, Vec2::splat(16.0)), Glyph::Globe, c::muted()),
    }
}

/// Column header: a name on the left and ↓ / ↑ on the right.
fn table_head(ui: &mut Ui, name: &str) {
    let w = ui.available_width();
    let (hr, _) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::hover());
    let p = ui.painter();
    p.text(Pos2::new(hr.left() + 28.0, hr.center().y), Align2::LEFT_CENTER, name, theme::semibold(12.5), c::muted());
    theme::text_right(p, Pos2::new(hr.right() - NUM_W - 6.0, hr.center().y), "↓", theme::semibold(12.5), c::muted());
    theme::text_right(p, Pos2::new(hr.right() - 6.0, hr.center().y), "↑", theme::semibold(12.5), c::muted());
    p.line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, c::border()));
}

fn numbers(ui: &Ui, r: Rect, down: u64, up: u64) {
    let p = ui.painter();
    let font = theme::regular(13.0);
    theme::text_right(p, Pos2::new(r.right() - NUM_W - 6.0, r.center().y), &theme::bytes(down), font.clone(), c::text());
    theme::text_right(p, Pos2::new(r.right() - 6.0, r.center().y), &theme::bytes(up), font, c::text());
}

/// Draws the open popup, if any. Esc or a press outside it closes it.
pub fn show(ui: &mut Ui, agg: &Agg, s: &mut Shared, icons: &mut Icons) {
    let ctx = ui.ctx().clone();
    let Some(mut st) = ctx.data(|d| d.get_temp::<Open>(id())) else { return };
    let press = if ctx.input(|i| i.pointer.any_pressed()) { ctx.input(|i| i.pointer.interact_pos()) } else { None };
    let esc = ctx.input(|i| i.key_pressed(Key::Escape));

    let mut next: Option<Detail> = None;
    let mut copy: Option<String> = None;
    let mut menu_layer = None;
    let layer = match st.what.clone() {
        Detail::Host(key) => {
            let Some(row) = agg.hosts.iter().find(|r| r.key == key) else {
                close(&ctx);
                return;
            };
            let ips = ips_for(&row.label, &s.names);
            let rect = app_popup::place(st.at, HOST_SIZE, ctx.screen_rect());
            let area = egui::Area::new(Id::new("host_popup")).order(Order::Foreground).fixed_pos(rect.min).fade_in(false).show(&ctx, |ui| {
                frame(ui, HOST_SIZE, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                    ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
                        flag_or_globe(ui, ir.center(), row.cc.as_deref());
                        theme::label(ui, row.label.clone(), theme::semibold(15.5), c::text());
                    });
                    let w = ui.available_width();
                    let (sep, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
                    ui.painter().rect_filled(sep, CornerRadius::ZERO, c::border());

                    let region = row.cc.as_deref().map(geo::name).unwrap_or_else(|| "Unknown".into());
                    let ip_text = match ips.len() {
                        0 => "—".to_string(),
                        1 => ips[0].to_string(),
                        n => format!("{}  (+{} more)", ips[0], n - 1),
                    };
                    for (label, value) in [("Region", region), ("IP", ip_text)] {
                        let (rr, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::hover());
                        let p = ui.painter();
                        p.text(Pos2::new(rr.left(), rr.center().y), Align2::LEFT_CENTER, label, theme::regular(13.0), c::muted());
                        theme::text(p, Pos2::new(rr.left() + 90.0, rr.center().y), &value, theme::regular(13.5), c::text(), w - 130.0);
                        if label == "IP" && ips.len() > 1 {
                            resp.on_hover_text(ips.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join("\n"));
                        }
                        if label == "IP" && !ips.is_empty() {
                            // ⋯ in a circle: Copy IP / Check IP
                            let text_w = ui.painter().layout_no_wrap(value.clone(), theme::regular(13.5), c::text()).size().x.min(w - 130.0);
                            let br = Rect::from_center_size(Pos2::new(rr.left() + 90.0 + text_w + 18.0, rr.center().y), Vec2::splat(20.0));
                            let bresp = ui.interact(br, Id::new("host_popup_ip_menu"), Sense::click());
                            let lit = st.menu.is_some() || bresp.hovered();
                            let ink = if lit { c::text() } else { c::muted() };
                            let p = ui.painter();
                            if lit {
                                p.circle_filled(br.center(), 9.5, c::hover());
                            }
                            p.circle_stroke(br.center(), 9.0, Stroke::new(1.2_f32, ink));
                            for dx in [-4.0, 0.0, 4.0] {
                                p.circle_filled(br.center() + Vec2::new(dx, 0.0), 1.3, ink);
                            }
                            if bresp.clicked() {
                                st.menu = if st.menu.is_some() { None } else { Some(br) };
                            }
                            bresp.on_hover_text("IP options");
                        }
                    }
                    ui.add_space(4.0);
                    table_head(ui, "App");
                    if row.app_bytes.is_empty() {
                        ui.add_space(10.0);
                        theme::label(ui, "No per-app detail for this host", theme::regular(13.0), c::muted());
                    } else {
                        egui::ScrollArea::vertical().id_salt("host_popup_apps").auto_shrink([false, false]).show_rows(ui, ROW_H, row.app_bytes.len(), |ui, range| {
                            for (k, v) in &row.app_bytes[range] {
                                let (rr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
                                let name = s.app_name(k);
                                icons.draw(ui, s, k, Rect::from_center_size(Pos2::new(rr.left() + 10.0, rr.center().y), Vec2::splat(18.0)), &name);
                                theme::text(ui.painter(), Pos2::new(rr.left() + 28.0, rr.center().y), &name, theme::regular(13.5), c::text(), rr.width() - 2.0 * NUM_W - 40.0);
                                numbers(ui, rr, v[0], v[1]);
                            }
                        });
                    }
                });
            });
            if let (Some(btn), Some(ip)) = (st.menu, ips.first()) {
                let mut pick = None;
                let ar = egui::Area::new(Id::new("host_popup_menu")).order(Order::Tooltip).fixed_pos(btn.left_bottom() + Vec2::new(0.0, 4.0)).fade_in(false).show(&ctx, |ui| {
                    egui::Frame::NONE.fill(c::panel()).stroke(Stroke::new(1.0_f32, c::border())).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, label) in ["Copy IP", "Check IP on VirusTotal"].into_iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(MENU_W, 28.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, CornerRadius::same(6), c::hover());
                            }
                            ui.painter().text(Pos2::new(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, label, theme::regular(13.5), c::text());
                            if resp.clicked() {
                                pick = Some(i);
                            }
                        }
                    });
                });
                menu_layer = Some(ar.response.layer_id);
                match pick {
                    Some(0) => {
                        copy = Some(ip.to_string());
                        st.menu = None;
                    }
                    Some(_) => {
                        let _ = std::process::Command::new("explorer.exe").arg(vt_ip_url(ip)).spawn();
                        st.menu = None;
                    }
                    None => {}
                }
            }
            area.response.layer_id
        }
        Detail::Country(cc) => {
            let Some(country) = agg.countries.iter().find(|r| r.key == cc) else {
                close(&ctx);
                return;
            };
            let hosts = hosts_in(agg, &cc);
            let rect = app_popup::place(st.at, COUNTRY_SIZE, ctx.screen_rect());
            let area = egui::Area::new(Id::new("country_popup")).order(Order::Foreground).fixed_pos(rect.min).fade_in(false).show(&ctx, |ui| {
                frame(ui, COUNTRY_SIZE, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
                    ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
                        flag_or_globe(ui, ir.center(), country.cc.as_deref());
                        let n = hosts.len();
                        theme::label(ui, format!("{} ({n} host{})", country.label, if n == 1 { "" } else { "s" }), theme::semibold(15.5), c::text());
                    });
                    table_head(ui, "Host");
                    let all = std::iter::once(None).chain(hosts.iter().map(|h| Some(*h)));
                    let rows: Vec<Option<&Row>> = all.collect();
                    egui::ScrollArea::vertical().id_salt("country_popup_hosts").auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len(), |ui, range| {
                        for r in &rows[range] {
                            let (rr, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), if r.is_some() { Sense::click() } else { Sense::hover() });
                            if resp.hovered() {
                                ui.painter().rect_filled(rr, CornerRadius::same(4), c::hover());
                            }
                            flag_or_globe(ui, Pos2::new(rr.left() + 10.0, rr.center().y), country.cc.as_deref());
                            let (label, down, up) = match r {
                                None => ("All", country.down, country.up),
                                Some(h) => (h.label.as_str(), h.down, h.up),
                            };
                            let mut x = rr.right() - 2.0 * NUM_W - 16.0;
                            if let Some(h) = r {
                                for k in h.apps.iter().rev() {
                                    let ar = Rect::from_min_size(Pos2::new(x - 16.0, rr.center().y - 8.0), Vec2::splat(16.0));
                                    let name = s.app_name(k);
                                    icons.draw(ui, s, k, ar, &name);
                                    x = ar.left() - 4.0;
                                }
                            }
                            let font = if r.is_none() { theme::semibold(13.5) } else { theme::regular(13.5) };
                            theme::text(ui.painter(), Pos2::new(rr.left() + 28.0, rr.center().y), label, font, c::text(), x - rr.left() - 34.0);
                            numbers(ui, rr, down, up);
                            if let (Some(h), true) = (r, resp.clicked()) {
                                next = Some(Detail::Host(h.key.clone()));
                            }
                        }
                    });
                });
            });
            area.response.layer_id
        }
    };

    if let Some(text) = copy {
        ctx.copy_text(text);
        s.toast = Some("IP copied".into());
    }
    if let Some(what) = next {
        st = Open { what, at: st.at, menu: None };
        ctx.data_mut(|d| d.insert_temp(id(), st));
        return;
    }
    let outside = press.is_some_and(|pos| {
        let hit = ctx.layer_id_at(pos);
        hit != Some(layer) && (menu_layer.is_none() || hit != menu_layer)
    });
    if esc {
        if st.menu.take().is_none() {
            close(&ctx);
            return;
        }
    } else if outside {
        close(&ctx);
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id(), st));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ips_for_host_name_and_literal() {
        let mut names = HashMap::new();
        names.insert("1.1.1.2".parse::<IpAddr>().unwrap(), "x.com".to_string());
        names.insert("1.1.1.1".parse::<IpAddr>().unwrap(), "x.com".to_string());
        names.insert("9.9.9.9".parse::<IpAddr>().unwrap(), "y.com".to_string());
        assert_eq!(ips_for("x.com", &names), vec!["1.1.1.1".parse::<IpAddr>().unwrap(), "1.1.1.2".parse().unwrap()]);
        assert_eq!(ips_for("104.26.5.15", &names), vec!["104.26.5.15".parse::<IpAddr>().unwrap()]);
        assert!(ips_for("none.com", &names).is_empty());
    }

    #[test]
    fn vt_ip_url_format() {
        assert_eq!(vt_ip_url(&"8.8.8.8".parse().unwrap()), "https://www.virustotal.com/gui/ip-address/8.8.8.8");
    }

    #[test]
    fn hosts_in_filters_by_country() {
        let agg = Agg {
            hosts: vec![
                Row { key: "a|US".into(), cc: Some("US".into()), ..Default::default() },
                Row { key: "b|DE".into(), cc: Some("DE".into()), ..Default::default() },
                Row { key: "c|US".into(), cc: Some("US".into()), ..Default::default() },
                Row { key: "d|".into(), cc: None, ..Default::default() },
            ],
            ..Default::default()
        };
        assert_eq!(hosts_in(&agg, "US").iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["a|US", "c|US"]);
    }
}
