mod backdrop;
mod firewall;
mod hardware;
mod log;
mod motion;
pub mod palette;
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
    page_changed: Instant,
    theme: Option<egui::Theme>,
    theme_fade: (palette::Palette, palette::Palette, Instant),
    quit: Arc<AtomicBool>,
    _tray: Option<tray_icon::TrayIcon>,
}

impl VantageApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        theme::setup(&cc.egui_ctx);
        let hwnd = cc.window_handle().ok().and_then(|h| match h.as_raw() {
            RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
            _ => None,
        });
        let ctx = cc.egui_ctx.clone();
        // Hidden in the tray or minimized there is nothing to redraw.
        let monitor = Monitor::start(move || {
            if hwnd.is_none_or(window_shown) {
                ctx.request_repaint();
            }
        });
        let quit = Arc::new(AtomicBool::new(false));
        let tray = build_tray(&cc.egui_ctx, hwnd, quit.clone());
        crate::diag::log(&format!("ui ready: tray={} hwnd={}", tray.is_some(), hwnd.is_some()));
        Self {
            monitor,
            page: start_page(),
            icons: Icons::default(),
            traffic: Default::default(),
            firewall: Default::default(),
            log: Default::default(),
            scanner: Default::default(),
            hardware: Default::default(),
            toast: None,
            page_changed: Instant::now() - std::time::Duration::from_secs(1),
            theme: None,
            theme_fade: (palette::dark(), palette::dark(), Instant::now()),
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

        self.update_theme(ctx);
        backdrop::paint_backdrop(&ctx.layer_painter(egui::LayerId::background()), ctx.screen_rect());
        let mut actions = Vec::new();
        {
            let monitor = self.monitor.clone();
            let mut guard = monitor.lock();
            let s = &mut *guard;
            if let Some(t) = s.toast.take() {
                self.toast = Some((t, Instant::now()));
            }
            let unread = s.store.data.events.iter().filter(|e| !e.read && e.kind.important()).count();

            self.rail(ctx, unread);
            self.header(ctx, s.internet, s.store.data.settings.firewall_enabled, s.admin);

            if !s.admin {
                egui::TopBottomPanel::top("admin")
                    .exact_height(44.0)
                    .frame(egui::Frame::default().inner_margin(egui::Margin { left: 20, right: 20, top: 0, bottom: 6 }))
                    .show(ctx, |ui| {
                        let bar = ui.max_rect();
                        let w = c::warn();
                        ui.painter().rect(
                            bar,
                            CornerRadius::same(10),
                            Color32::from_rgba_unmultiplied(w.r(), w.g(), w.b(), 28),
                            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(w.r(), w.g(), w.b(), 60)),
                            egui::StrokeKind::Inside,
                        );
                        ui.horizontal_centered(|ui| {
                            ui.add_space(12.0);
                            theme::glyph(ui.painter(), Rect::from_center_size(ui.cursor().min + Vec2::new(9.0, 19.0), Vec2::splat(18.0)), Glyph::Lock, c::warn());
                            ui.add_space(26.0);
                            theme::label(ui, "Limited mode: per-app traffic and silent firewall changes need administrator rights.", theme::regular(13.5), c::text());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(8.0);
                                if theme::primary_button(ui, "Restart as administrator").clicked() {
                                    actions.push(Action::RelaunchAdmin);
                                }
                            });
                        });
                    });
            }

            let t = self.page_changed.elapsed().as_secs_f32() / motion::NORMAL;
            let e = motion::ease_out(t);
            if t < 1.0 {
                ctx.request_repaint();
            }
            egui::CentralPanel::default().frame(egui::Frame::default().fill(Color32::TRANSPARENT)).show(ctx, |ui| {
                ui.set_opacity(e);
                ui.add_space(8.0 * (1.0 - e));
                match self.page {
                    Page::Traffic => self.traffic.show(ui, s, &mut self.icons),
                    Page::Firewall => self.firewall.show(ui, s, &mut self.icons, &mut actions),
                    Page::Log => self.log.show(ui, s, &mut self.icons),
                    Page::Scanner => self.scanner.show(ui, s, &mut actions),
                    Page::Hardware => self.hardware.show(ui, s, &mut self.icons),
                    Page::Settings => settings::show(ui, s, &mut actions),
                }
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
    /// Follows the Appearance setting: switches the palette (cross-fading over `motion::NORMAL`),
    /// egui's visuals and the native title bar. The first call applies the theme with no fade.
    fn update_theme(&mut self, ctx: &egui::Context) {
        let appearance = appearance_override().unwrap_or_else(|| self.monitor.lock().store.data.settings.appearance);
        let target = crate::store::resolve(appearance, ctx.system_theme());
        let pal_for = |t: egui::Theme| if t == egui::Theme::Light { palette::light() } else { palette::dark() };
        if self.theme != Some(target) {
            let first = self.theme.is_none();
            let from = if first { pal_for(target) } else { palette::current() };
            self.theme = Some(target);
            self.theme_fade = (from, pal_for(target), Instant::now());
            ctx.set_theme(target);
            // System keeps following the OS; a forced theme would hide later OS changes from `system_theme`
            let native = match appearance {
                crate::store::Appearance::System => egui::SystemTheme::SystemDefault,
                _ if target == egui::Theme::Light => egui::SystemTheme::Light,
                _ => egui::SystemTheme::Dark,
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(native));
            if first {
                palette::set(self.theme_fade.1);
                theme::apply_visuals(ctx, &self.theme_fade.1);
            }
        }
        let (from, to, at) = self.theme_fade;
        let t = at.elapsed().as_secs_f32() / motion::NORMAL;
        if t < 1.0 {
            let p = palette::lerp(&from, &to, motion::ease_out(t));
            palette::set(p);
            theme::apply_visuals(ctx, &p);
            ctx.request_repaint();
        } else if palette::current() != to {
            palette::set(to);
            theme::apply_visuals(ctx, &to);
        }
    }

    fn rail(&mut self, ctx: &egui::Context, unread: usize) {
        const PAGES: [(Page, Glyph, &str); 5] = [
            (Page::Traffic, Glyph::Traffic, "Traffic"),
            (Page::Firewall, Glyph::Shield, "Firewall"),
            (Page::Log, Glyph::Bell, "Activity"),
            (Page::Scanner, Glyph::Radar, "Scanner"),
            (Page::Hardware, Glyph::Chip, "Hardware"),
        ];
        egui::SidePanel::left("rail").exact_width(64.0).resizable(false).frame(egui::Frame::default()).show(ctx, |ui| {
            let full = ui.max_rect();
            let p = ui.painter().clone();
            let g = c::glass_top();
            p.rect_filled(full, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(g.r(), g.g(), g.b(), g.a() / 2));
            p.line_segment([full.right_top(), full.right_bottom()], Stroke::new(1.0_f32, c::glass_border()));

            let (logo, slots, gear) = rail_slots(full, PAGES.len());
            p.rect_filled(logo, CornerRadius::same(9), c::accent());
            theme::glyph(&p, logo.shrink(7.0), Glyph::Traffic, Color32::WHITE);

            let active_rect = if self.page == Page::Settings {
                Some(gear)
            } else {
                PAGES.iter().position(|(pg, ..)| *pg == self.page).map(|i| slots[i])
            };
            if let Some(ar) = active_rect {
                let y = motion::value(ctx, ui.id().with("rail_active"), ar.top());
                let hl = Rect::from_min_size(Pos2::new(ar.left(), y), ar.size());
                theme::glow(&p, hl, 12, 0.8);
                p.rect_filled(hl, CornerRadius::same(12), c::selected());
                p.rect_filled(Rect::from_min_size(Pos2::new(full.left(), y + 10.0), Vec2::new(3.0, 20.0)), CornerRadius::same(2), c::accent2());
            }

            let mut clicked = None;
            let item = |ui: &mut egui::Ui, r: Rect, name: &'static str, g: Glyph, active: bool, tip: String| {
                let resp = ui.interact(r, ui.id().with(name), Sense::click()).on_hover_text(tip);
                let t = motion::hover(ui, resp.id, resp.hovered());
                if !active && t > 0.0 {
                    p.rect_filled(r, CornerRadius::same(12), theme::lerp_color(Color32::TRANSPARENT, c::hover(), t));
                }
                let base = if active { c::accent2() } else { c::muted() };
                theme::glyph(&p, r.shrink(10.0), g, theme::lerp_color(base, c::text(), if active { 0.0 } else { t }));
                resp.clicked()
            };
            for (i, (pg, g, name)) in PAGES.iter().enumerate() {
                let tip = if *pg == Page::Log { format!("{name}: {}", unread_label(unread)) } else { name.to_string() };
                if item(ui, slots[i], name, *g, self.page == *pg, tip) {
                    clicked = Some(*pg);
                }
                if *pg == Page::Log && unread > 0 {
                    let d = Pos2::new(slots[i].right() - 8.0, slots[i].top() + 8.0);
                    ui.painter().circle_filled(d, 5.5, c::bg());
                    ui.painter().circle_filled(d, 4.0, c::bad());
                }
            }
            if item(ui, gear, "Settings", Glyph::Gear, self.page == Page::Settings, "Settings".to_string()) {
                clicked = Some(Page::Settings);
            }
            if let Some(pg) = clicked {
                if pg != self.page {
                    self.page = pg;
                    self.page_changed = Instant::now();
                    ctx.request_repaint();
                }
            }
        });
    }

    fn header(&self, ctx: &egui::Context, internet: Option<bool>, fw_on: bool, admin: bool) {
        egui::TopBottomPanel::top("header")
            .exact_height(56.0)
            .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(24, 0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    theme::label(ui, page_title(self.page), theme::bold(20.0), c::text());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (ig, ic, it) = match internet {
                            Some(true) => (Glyph::Wifi, c::good(), "Online"),
                            Some(false) => (Glyph::WifiOff, c::bad(), "Offline"),
                            None => (Glyph::Wifi, c::muted(), "Checking connection..."),
                        };
                        let (fc, ft) = if fw_on { (c::good(), "Firewall is on") } else { (c::faint(), "Firewall is off") };
                        let (ac, at) = if admin { (c::accent(), "Administrator: full monitoring") } else { (c::warn(), "Limited mode: not running as administrator") };
                        // right_to_left: first added is rightmost
                        status_chip(ui, Glyph::Lock, ac, at);
                        status_chip(ui, Glyph::Shield, fc, ft);
                        status_chip(ui, ig, ic, it);
                    });
                });
            });
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
        let off = 16.0 * (1.0 - motion::ease_out(age / motion::NORMAL));
        egui::Area::new("toast".into())
            .fixed_pos(Pos2::new(screen.right() - 24.0 + off, screen.bottom() - 24.0 + off))
            .pivot(Align2::RIGHT_BOTTOM)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                egui::Frame::default().fill(c::bg()).corner_radius(CornerRadius::same(14)).show(ui, |ui| {
                    theme::glass_frame().inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                        ui.set_max_width(420.0);
                        theme::label(ui, msg.clone(), theme::regular(14.0), c::text());
                    });
                });
            });
        ctx.request_repaint();
    }
}

fn page_title(p: Page) -> &'static str {
    match p {
        Page::Traffic => "Traffic",
        Page::Firewall => "Firewall",
        Page::Log => "Activity",
        Page::Scanner => "Scanner",
        Page::Hardware => "Hardware",
        Page::Settings => "Settings",
    }
}

fn unread_label(n: usize) -> String {
    match n {
        0 => "No new alerts".to_string(),
        1 => "1 new alert".to_string(),
        n => format!("{n} new alerts"),
    }
}

/// Logo rect, page slots and settings slot for the icon rail occupying `rect`.
fn rail_slots(rect: Rect, n_top: usize) -> (Rect, Vec<Rect>, Rect) {
    let x = rect.center().x - 20.0;
    let logo = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 32.0), Vec2::splat(34.0));
    let slots = (0..n_top).map(|i| Rect::from_min_size(Pos2::new(x, rect.top() + 64.0 + i as f32 * 50.0), Vec2::splat(40.0))).collect();
    let gear = Rect::from_min_size(Pos2::new(x, rect.bottom() - 14.0 - 40.0), Vec2::splat(40.0));
    (logo, slots, gear)
}

fn status_chip(ui: &mut egui::Ui, g: Glyph, col: Color32, tip: &str) {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
    let t = motion::hover(ui, resp.id, resp.hovered());
    ui.painter().rect(
        r,
        CornerRadius::same(14),
        theme::lerp_color(c::glass_top(), c::hover(), t),
        Stroke::new(1.0_f32, c::glass_border()),
        egui::StrokeKind::Inside,
    );
    theme::glyph(ui.painter(), r.shrink(6.0), g, col);
    resp.on_hover_text(tip);
    ui.add_space(4.0);
}

/// Debug builds can force the theme (VANTAGE_APPEARANCE=dark|light) for screenshots.
fn appearance_override() -> Option<crate::store::Appearance> {
    use crate::store::Appearance;
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("VANTAGE_APPEARANCE").unwrap_or_default().as_str() {
        "dark" => Some(Appearance::Dark),
        "light" => Some(Appearance::Light),
        _ => None,
    }
}

/// Debug builds can open on a given page (VANTAGE_START_PAGE=firewall|log|scanner|hardware|settings) for screenshots.
fn start_page() -> Page {
    if !cfg!(debug_assertions) {
        return Page::Traffic;
    }
    match std::env::var("VANTAGE_START_PAGE").unwrap_or_default().as_str() {
        "firewall" => Page::Firewall,
        "log" => Page::Log,
        "scanner" => Page::Scanner,
        "hardware" => Page::Hardware,
        "settings" => Page::Settings,
        _ => Page::Traffic,
    }
}

fn window_shown(hwnd: isize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
    unsafe { IsWindowVisible(hwnd as _) != 0 && IsIconic(hwnd as _) == 0 }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rail_slots_fit_min_height() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(64.0, 680.0 - 32.0));
        let (_logo, slots, settings) = rail_slots(rect, 5);
        assert!(settings.top() >= slots.last().unwrap().bottom() + 4.0);
        assert!(settings.bottom() <= rect.bottom());
    }

    #[test]
    fn rail_slots_count() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(64.0, 648.0));
        let (_, slots, _) = rail_slots(rect, 5);
        assert_eq!(slots.len(), 5);
        assert_eq!(slots[0].size(), Vec2::splat(40.0));
        assert_eq!(slots[0].top(), 64.0);
        assert_eq!(slots[1].top() - slots[0].bottom(), 10.0);
    }

    #[test]
    fn unread_label_plural() {
        assert_eq!(unread_label(0), "No new alerts");
        assert_eq!(unread_label(1), "1 new alert");
        assert_eq!(unread_label(7), "7 new alerts");
    }

    #[test]
    fn page_titles() {
        assert_eq!(page_title(Page::Log), "Activity");
        assert_eq!(page_title(Page::Settings), "Settings");
    }
}
