mod firewall;
mod hardware;
mod log;
mod scanner;
mod settings;
pub mod theme;
mod traffic;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::monitor::Monitor;
use crate::system::Dir;
use theme::{c, Glyph, Icons};

pub enum Action {
    Block(String, Dir, bool),
    FirewallEnabled(bool),
    Scan,
    RelaunchAdmin,
    OpenDataDir,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Traffic,
    Firewall,
    Log,
    Scanner,
    Hardware,
    Settings,
}

pub struct VantageApp {
    monitor: Monitor,
    page: Page,
    icons: Icons,
    traffic: traffic::TrafficPage,
    firewall: firewall::FirewallPage,
    log: log::LogPage,
    scanner: scanner::ScannerPage,
    hardware: hardware::HardwarePage,
    toast: Option<(String, Instant)>,
    quit: Arc<AtomicBool>,
    _tray: Option<tray_icon::TrayIcon>,
}

impl VantageApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        theme::setup(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let monitor = Monitor::start(move || ctx.request_repaint());
        let quit = Arc::new(AtomicBool::new(false));
        let hwnd = cc.window_handle().ok().and_then(|h| match h.as_raw() {
            RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
            _ => None,
        });
        let tray = build_tray(&cc.egui_ctx, hwnd, quit.clone());
        crate::diag::log(&format!("ui ready: tray={} hwnd={}", tray.is_some(), hwnd.is_some()));
        Self {
            monitor,
            page: Page::Traffic,
            icons: Icons::default(),
            traffic: Default::default(),
            firewall: Default::default(),
            log: Default::default(),
            scanner: Default::default(),
            hardware: Default::default(),
            toast: None,
            quit,
            _tray: tray,
        }
    }

    fn run_actions(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Block(key, dir, block) => self.monitor.set_block(&key, dir, block),
                Action::FirewallEnabled(on) => self.monitor.set_firewall_enabled(on),
                Action::Scan => self.monitor.request_scan(),
                Action::RelaunchAdmin => {
                    self.monitor.save();
                    if crate::net::relaunch_as_admin() {
                        std::process::exit(0);
                    }
                }
                Action::OpenDataDir => {
                    let dir = self.monitor.lock().store.dir.clone();
                    let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
                }
            }
        }
    }
}

impl eframe::App for VantageApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let quitting = self.quit.load(Ordering::SeqCst);
        let close_to_tray = self.monitor.lock().store.data.settings.close_to_tray && self._tray.is_some();
        if ctx.input(|i| i.viewport().close_requested()) && close_to_tray && !quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if quitting {
            self.monitor.save();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let mut actions = Vec::new();
        {
            let monitor = self.monitor.clone();
            let mut guard = monitor.lock();
            let s = &mut *guard;
            if let Some(t) = s.toast.take() {
                self.toast = Some((t, Instant::now()));
            }
            let unread = s.store.data.events.iter().filter(|e| !e.read && e.kind.important()).count();

            egui::TopBottomPanel::top("nav")
                .exact_height(78.0)
                .frame(egui::Frame::default().fill(c::PANEL).stroke(Stroke::new(1.0_f32, c::BORDER)))
                .show(ctx, |ui| self.nav(ui, unread, s.admin));

            if !s.admin {
                egui::TopBottomPanel::top("admin")
                    .exact_height(40.0)
                    .frame(egui::Frame::default().fill(Color32::from_rgb(0x2a, 0x24, 0x14)).inner_margin(egui::Margin::symmetric(18, 0)))
                    .show(ctx, |ui| {
                        ui.horizontal_centered(|ui| {
                            theme::glyph(ui.painter(), Rect::from_center_size(ui.cursor().min + Vec2::new(9.0, 20.0), Vec2::splat(18.0)), Glyph::Lock, c::WARN);
                            ui.add_space(26.0);
                            theme::label(
                                ui,
                                "Limited mode: per-app traffic and silent firewall changes need administrator rights.",
                                theme::regular(13.5),
                                Color32::from_rgb(0xf3, 0xd9, 0x9b),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if theme::primary_button(ui, "Restart as administrator").clicked() {
                                    actions.push(Action::RelaunchAdmin);
                                }
                            });
                        });
                    });
            }

            egui::CentralPanel::default().frame(egui::Frame::default().fill(c::BG)).show(ctx, |ui| match self.page {
                Page::Traffic => self.traffic.show(ui, s, &mut self.icons),
                Page::Firewall => self.firewall.show(ui, s, &mut self.icons, &mut actions),
                Page::Log => self.log.show(ui, s, &mut self.icons),
                Page::Scanner => self.scanner.show(ui, s, &mut actions),
                Page::Hardware => self.hardware.show(ui, s, &mut self.icons),
                Page::Settings => settings::show(ui, s, &mut actions),
            });
        }
        self.draw_toast(ctx);
        self.run_actions(actions);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.monitor.save();
    }
}

impl VantageApp {
    fn nav(&mut self, ui: &mut egui::Ui, unread: usize, admin: bool) {
        let full = ui.max_rect();
        let p = ui.painter().clone();

        // Brand
        let logo = Rect::from_min_size(Pos2::new(full.left() + 18.0, full.center().y - 15.0), Vec2::splat(30.0));
        p.rect_filled(logo, CornerRadius::same(8), c::ACCENT);
        theme::glyph(&p, logo.shrink(6.0), Glyph::Traffic, Color32::WHITE);
        p.text(Pos2::new(logo.right() + 10.0, full.center().y - 8.0), Align2::LEFT_CENTER, "Vantage", theme::semibold(17.0), c::TEXT);
        p.text(
            Pos2::new(logo.right() + 10.0, full.center().y + 11.0),
            Align2::LEFT_CENTER,
            if admin { "FULL MONITORING" } else { "LIMITED MODE" },
            theme::semibold(10.5),
            if admin { c::GOOD } else { c::WARN },
        );

        let tabs: [(Page, Glyph, &str, &str); 5] = [
            (Page::Traffic, Glyph::Traffic, "Traffic", "Monitor"),
            (Page::Firewall, Glyph::Shield, "Firewall", "Rules"),
            (Page::Log, Glyph::Log, "Activity", "Log"),
            (Page::Scanner, Glyph::Scanner, "Network", "Scanner"),
            (Page::Hardware, Glyph::Chip, "Hardware", "Resources"),
        ];
        let tab_w = 132.0;
        let total = tab_w * tabs.len() as f32 + 8.0 * (tabs.len() - 1) as f32;
        let mut x = (full.center().x - total / 2.0).max(full.left() + 190.0);
        for (page, g, l1, l2) in tabs {
            let r = Rect::from_min_size(Pos2::new(x, full.center().y - 25.0), Vec2::new(tab_w, 50.0));
            let resp = ui.interact(r, ui.id().with(l1), Sense::click());
            let active = self.page == page;
            if active {
                p.rect_filled(r, CornerRadius::same(10), c::SELECTED);
            } else if resp.hovered() {
                p.rect_filled(r, CornerRadius::same(10), c::HOVER);
            }
            let col = if active { Color32::from_rgb(0x9d, 0xbc, 0xff) } else { c::TEXT };
            theme::glyph(&p, Rect::from_center_size(Pos2::new(r.left() + 26.0, r.center().y), Vec2::splat(24.0)), g, col);
            p.text(Pos2::new(r.left() + 48.0, r.center().y - 8.0), Align2::LEFT_CENTER, l1, theme::semibold(13.5), col);
            p.text(Pos2::new(r.left() + 48.0, r.center().y + 9.0), Align2::LEFT_CENTER, l2, theme::semibold(13.5), col);
            if page == Page::Log && unread > 0 {
                let b = Pos2::new(r.left() + 36.0, r.top() + 12.0);
                let label = if unread > 99 { "99+".to_string() } else { unread.to_string() };
                let w = 10.0 + 7.0 * label.len() as f32;
                p.rect_filled(Rect::from_center_size(b, Vec2::new(w.max(18.0), 18.0)), CornerRadius::same(9), c::BAD);
                p.text(b, Align2::CENTER_CENTER, label, theme::semibold(11.0), Color32::WHITE);
            }
            if resp.clicked() {
                self.page = page;
            }
            x += tab_w + 8.0;
        }

        let gear = Rect::from_center_size(Pos2::new(full.right() - 34.0, full.center().y), Vec2::splat(38.0));
        let resp = ui.interact(gear, ui.id().with("settings"), Sense::click()).on_hover_text("Settings");
        let active = self.page == Page::Settings;
        if active || resp.hovered() {
            p.rect_filled(gear, CornerRadius::same(10), if active { c::SELECTED } else { c::HOVER });
        }
        theme::glyph(&p, gear.shrink(9.0), Glyph::Gear, if active { Color32::from_rgb(0x9d, 0xbc, 0xff) } else { c::MUTED });
        if resp.clicked() {
            self.page = Page::Settings;
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 4.5 {
            self.toast = None;
            return;
        }
        let alpha = ((4.5 - age) / 0.4).clamp(0.0, 1.0);
        let screen = ctx.screen_rect();
        egui::Area::new("toast".into())
            .fixed_pos(Pos2::new(screen.right() - 24.0, screen.bottom() - 24.0))
            .pivot(Align2::RIGHT_BOTTOM)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                egui::Frame::default()
                    .fill(c::RAISED)
                    .stroke(Stroke::new(1.0_f32, c::BORDER))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .shadow(egui::Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(140) })
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        theme::label(ui, msg.clone(), theme::regular(14.0), c::TEXT);
                    });
            });
        ctx.request_repaint();
    }
}

fn build_tray(ctx: &egui::Context, hwnd: Option<isize>, quit: Arc<AtomicBool>) -> Option<tray_icon::TrayIcon> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};

    let icon = tray_icon::Icon::from_rgba(theme::logo_rgba(32), 32, 32).ok()?;
    let menu = Menu::new();
    let open = MenuItem::new("Open Vantage", true, None);
    let exit = MenuItem::new("Quit", true, None);
    menu.append_items(&[&open, &PredefinedMenuItem::separator(), &exit]).ok()?;
    let (open_id, exit_id) = (open.id().clone(), exit.id().clone());

    let show = {
        let ctx = ctx.clone();
        move || {
            if let Some(h) = hwnd {
                unsafe {
                    ShowWindow(h as _, SW_SHOW);
                    ShowWindow(h as _, SW_RESTORE);
                    SetForegroundWindow(h as _);
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.request_repaint();
        }
    };
    let show2 = show.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } | TrayIconEvent::Click { button: MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e {
            show2();
        }
    }));
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == open_id {
            show();
        } else if e.id == exit_id {
            quit.store(true, Ordering::SeqCst);
            show();
        }
    }));
    TrayIconBuilder::new().with_icon(icon).with_tooltip("Vantage — network monitor").with_menu(Box::new(menu)).build().ok()
}
