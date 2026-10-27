//! Settings page.

use eframe::egui::{self, Vec2};

use super::theme::{self, c};
use super::Action;
use crate::monitor::Shared;
use crate::store::{self, Appearance, PlanPeriod, PlanTraffic};

/// Title and detail on the left, `right` laid out right-to-left on the right.
fn row(ui: &mut egui::Ui, title: &str, detail: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            theme::label(ui, title, theme::semibold(14.5), c::text());
            theme::label(ui, detail, theme::regular(13.0), c::muted());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(12.0);
}

fn toggle_row(ui: &mut egui::Ui, title: &str, detail: &str, value: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            theme::label(ui, title, theme::semibold(14.5), c::text());
            theme::label(ui, detail, theme::regular(13.0), c::muted());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::switch(ui, *value).clicked() {
                *value = !*value;
                changed = true;
            }
        });
    });
    ui.add_space(10.0);
    changed
}

pub fn show(ui: &mut egui::Ui, s: &mut Shared, actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let width = ui.available_width().min(760.0);
        let margin = ((ui.available_width() - width) / 2.0).max(16.0);
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                theme::label(ui, "Settings", theme::semibold(24.0), c::text());
                ui.add_space(18.0);
                let mut changed = false;
                let st = &mut s.store.data.settings;

                theme::label(ui, "APPEARANCE", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            theme::label(ui, "Theme", theme::semibold(14.5), c::text());
                            theme::label(ui, "System follows your Windows light or dark setting.", theme::regular(13.0), c::muted());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut sel = match st.appearance {
                                Appearance::System => 0,
                                Appearance::Dark => 1,
                                Appearance::Light => 2,
                            };
                            let before = sel;
                            theme::segmented_text(ui, &["System", "Dark", "Light"], &mut sel, None);
                            if sel != before {
                                st.appearance = [Appearance::System, Appearance::Dark, Appearance::Light][sel];
                                changed = true;
                            }
                        });
                    });
                    ui.add_space(12.0);
                    row(ui, "Speed units", "How upload and download speeds are shown.", |ui| {
                        let mut sel = st.speed_bits as usize;
                        theme::segmented_text(ui, &["Bytes/s", "Bits/s"], &mut sel, None);
                        if sel != st.speed_bits as usize {
                            st.speed_bits = sel == 1;
                            changed = true;
                        }
                    });
                });
                ui.add_space(20.0);

                theme::label(ui, "NOTIFICATIONS", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    changed |= toggle_row(ui, "Desktop notifications", "Show Windows notifications for the alerts below.", &mut st.notifications);
                    changed |= toggle_row(ui, "First network activity", "An app connects to the internet for the first time.", &mut st.notify_new_apps);
                    changed |= toggle_row(ui, "Internet connectivity", "Your internet connection drops or comes back.", &mut st.notify_internet);
                    changed |= toggle_row(ui, "New network devices", "A device joins your local network.", &mut st.notify_devices);
                    changed |= toggle_row(ui, "All device activity", "Also report known devices rejoining or leaving the network.", &mut st.notify_device_presence);
                    changed |= toggle_row(ui, "App updates", "A networked app's version changes.", &mut st.notify_versions);
                    let now = store::now();
                    let snoozed = st.snooze_until.filter(|t| *t > now);
                    let detail = match snoozed {
                        Some(t) => format!("Desktop notifications are paused until {}. Alerts are still logged.", theme::clock(t)),
                        None => "Pause desktop notifications for a while. Alerts are still logged.".to_string(),
                    };
                    row(ui, "Snooze", &detail, |ui| {
                        if snoozed.is_some() {
                            if theme::pill(ui, "Resume", None, false).clicked() {
                                st.snooze_until = None;
                                changed = true;
                            }
                        } else {
                            for (label, hours) in [("24 h", 24), ("8 h", 8), ("1 h", 1)] {
                                if theme::pill(ui, label, None, false).clicked() {
                                    st.snooze_until = Some(now + hours * 3600);
                                    changed = true;
                                }
                            }
                        }
                    });
                });
                ui.add_space(20.0);

                theme::label(ui, "SECURITY MONITORS", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    changed |= toggle_row(ui, "ARP spoofing detection", "Alert when your router's IP starts answering from a different device.", &mut st.monitor_arp);
                    changed |= toggle_row(ui, "DNS server settings", "Alert when the DNS servers Windows uses change.", &mut st.monitor_dns);
                    changed |= toggle_row(ui, "Proxy settings", "Alert when your proxy or auto-config script changes.", &mut st.monitor_proxy);
                    changed |= toggle_row(ui, "Remote Desktop (RDP)", "Alert when someone connects to this PC with Remote Desktop.", &mut st.monitor_rdp);
                });
                ui.add_space(20.0);

                theme::label(ui, "NETWORK SCANNER", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    row(ui, "Auto-scan", "Rescan the local network in the background and alert on new devices.", |ui| {
                        theme::label(ui, "min", theme::regular(13.5), c::muted());
                        changed |= ui.add_enabled(st.auto_scan, egui::DragValue::new(&mut st.scan_minutes).range(1..=1440)).changed();
                        theme::label(ui, "every", theme::regular(13.5), c::muted());
                        ui.add_space(8.0);
                        if theme::switch(ui, st.auto_scan).clicked() {
                            st.auto_scan = !st.auto_scan;
                            changed = true;
                        }
                    });
                });
                ui.add_space(20.0);

                theme::label(ui, "DATA PLAN", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            theme::label(ui, "Data limit", theme::semibold(14.5), c::text());
                            theme::label(ui, "Alert when usage in the period below passes this amount. 0 turns it off.", theme::regular(13.0), c::muted());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            theme::label(ui, "MB", theme::regular(13.5), c::muted());
                            let r = ui.add(egui::DragValue::new(&mut s.store.data.settings.daily_cap_mb).speed(50.0).range(0..=10_000_000));
                            changed |= r.changed();
                        });
                    });
                    ui.add_space(12.0);
                    let st = &mut s.store.data.settings;
                    row(ui, "Period", "The limit resets at the start of each day, week (Monday) or month.", |ui| {
                        let periods = [PlanPeriod::Day, PlanPeriod::Week, PlanPeriod::Month];
                        let mut sel = periods.iter().position(|p| *p == st.plan_period).unwrap_or(0);
                        theme::segmented_text(ui, &["Day", "Week", "Month"], &mut sel, None);
                        if periods[sel] != st.plan_period {
                            st.plan_period = periods[sel];
                            changed = true;
                        }
                    });
                    row(ui, "Traffic counted", "Which direction counts toward the limit.", |ui| {
                        let kinds = [PlanTraffic::Total, PlanTraffic::Download, PlanTraffic::Upload];
                        let mut sel = kinds.iter().position(|p| *p == st.plan_traffic).unwrap_or(0);
                        theme::segmented_text(ui, &["Total", "Download", "Upload"], &mut sel, None);
                        if kinds[sel] != st.plan_traffic {
                            st.plan_traffic = kinds[sel];
                            changed = true;
                        }
                    });
                    changed |= toggle_row(ui, "Exclude local network traffic", "Traffic to devices on your LAN doesn't count.", &mut st.plan_exclude_lan);
                    if st.daily_cap_mb > 0 {
                        let (used, _) = s.store.plan_usage();
                        let cap = s.store.data.settings.daily_cap_mb * 1_000_000;
                        theme::label(ui, format!("Used {} of {} ({:.0}%)", theme::bytes(used), theme::bytes(cap), used as f64 / cap as f64 * 100.0), theme::regular(13.0), c::muted());
                        ui.add_space(12.0);
                    }
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            theme::label(ui, "Reset usage history", theme::semibold(14.5), c::text());
                            theme::label(ui, "Clears all recorded traffic totals and graphs.", theme::regular(13.0), c::muted());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let id = ui.id().with("confirm-reset");
                            let armed = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
                            if armed {
                                if theme::pill(ui, "Cancel", None, false).clicked() {
                                    ui.data_mut(|d| d.insert_temp(id, false));
                                }
                                if theme::primary_button(ui, "Yes, reset").clicked() {
                                    s.store.reset_usage();
                                    s.toast = Some("Usage history cleared".into());
                                    ui.data_mut(|d| d.insert_temp(id, false));
                                }
                            } else if theme::pill(ui, "Reset…", None, false).clicked() {
                                ui.data_mut(|d| d.insert_temp(id, true));
                            }
                        });
                    });
                });
                let st = &mut s.store.data.settings;
                let mut clear_log = false;
                ui.add_space(20.0);

                theme::label(ui, "APP", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    changed |= toggle_row(ui, "Close open connections when blocking", "Cuts an app off immediately. Some apps (e.g. browsers) then need a restart after you unblock them.", &mut st.close_on_block);
                    changed |= toggle_row(ui, "Keep running in the tray", "Closing the window keeps monitoring in the background.", &mut st.close_to_tray);
                    let mut startup = st.run_on_startup;
                    if toggle_row(ui, "Run on startup", "Start Vantage when you sign in to Windows.", &mut startup) {
                        actions.push(Action::RunOnStartup(startup));
                    }
                    changed |= toggle_row(ui, "Look up host names", "Resolve remote IP addresses to names with reverse DNS.", &mut st.dns_lookup);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            theme::label(ui, "Administrator mode", theme::semibold(14.5), c::text());
                            let msg = if s.admin { "Running elevated: per-app traffic and silent firewall changes are on." } else { "Not elevated: traffic is shown as a total and firewall changes ask for permission." };
                            theme::label(ui, msg, theme::regular(13.0), c::muted());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if s.admin {
                                theme::label(ui, "Active", theme::semibold(13.5), c::good());
                            } else if theme::primary_button(ui, "Restart elevated").clicked() {
                                actions.push(Action::RelaunchAdmin);
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            theme::label(ui, "Clear activity log", theme::semibold(14.5), c::text());
                            theme::label(ui, "Deletes every alert. This also removes the markers on the graph and each app's Alerts history.", theme::regular(13.0), c::muted());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let id = ui.id().with("confirm-clear-log");
                            let armed = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
                            if armed {
                                if theme::pill(ui, "Cancel", None, false).clicked() {
                                    ui.data_mut(|d| d.insert_temp(id, false));
                                }
                                if theme::primary_button(ui, "Yes, clear").clicked() {
                                    clear_log = true;
                                    ui.data_mut(|d| d.insert_temp(id, false));
                                }
                            } else if theme::pill(ui, "Clear…", None, false).clicked() {
                                ui.data_mut(|d| d.insert_temp(id, true));
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            theme::label(ui, "Data folder", theme::semibold(14.5), c::text());
                            theme::label(ui, s.store.dir.display().to_string(), theme::regular(13.0), c::muted());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::pill(ui, "Open", None, false).clicked() {
                                actions.push(Action::OpenDataDir);
                            }
                        });
                    });
                });
                if clear_log {
                    s.store.data.events.clear();
                    s.store.mark_dirty();
                    s.toast = Some("Activity log cleared".into());
                }
                ui.add_space(20.0);
                theme::label(ui, format!("Vantage {} · Rust + egui", env!("CARGO_PKG_VERSION")), theme::regular(12.5), c::faint());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.hyperlink_to(egui::RichText::new("IP Geolocation by DB-IP").font(theme::regular(12.5)), "https://db-ip.com")
                        .on_hover_text("https://db-ip.com · CC BY 4.0");
                    theme::label(ui, " (CC BY 4.0) · Flags: flag-icons (MIT) · Font: Manrope (OFL)", theme::regular(12.5), c::faint());
                });
                ui.add_space(Vec2::splat(30.0).y);
                if changed {
                    s.store.mark_dirty();
                }
            });
        });
    });
}
