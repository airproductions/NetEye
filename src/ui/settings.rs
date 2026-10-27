//! Settings page.

use eframe::egui::{self, Vec2};

use super::theme::{self, c};
use super::Action;
use crate::monitor::Shared;
use crate::store::Appearance;

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
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
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
                    changed |= toggle_row(ui, "App updates", "A networked app's version changes.", &mut st.notify_versions);
                });
                ui.add_space(20.0);

                theme::label(ui, "DATA USAGE", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            theme::label(ui, "Daily data limit", theme::semibold(14.5), c::text());
                            theme::label(ui, "Alert when today's traffic passes this amount. 0 turns it off.", theme::regular(13.0), c::muted());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            theme::label(ui, "MB", theme::regular(13.5), c::muted());
                            let r = ui.add(egui::DragValue::new(&mut s.store.data.settings.daily_cap_mb).speed(50.0).range(0..=10_000_000));
                            changed |= r.changed();
                        });
                    });
                    ui.add_space(12.0);
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
                ui.add_space(20.0);

                theme::label(ui, "APP", theme::semibold(12.0), c::muted());
                ui.add_space(6.0);
                theme::card(ui, |ui| {
                    ui.set_width(width - 32.0);
                    changed |= toggle_row(ui, "Close open connections when blocking", "Cuts an app off immediately. Some apps (e.g. browsers) then need a restart after you unblock them.", &mut st.close_on_block);
                    changed |= toggle_row(ui, "Keep running in the tray", "Closing the window keeps monitoring in the background.", &mut st.close_to_tray);
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
                ui.add_space(20.0);
                theme::label(ui, format!("Vantage {} · Rust + egui", env!("CARGO_PKG_VERSION")), theme::regular(12.5), c::faint());
                ui.add_space(Vec2::splat(30.0).y);
                if changed {
                    s.store.mark_dirty();
                }
            });
        });
    });
}
