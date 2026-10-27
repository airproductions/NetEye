mod app_popup;
mod firewall;
mod hardware;
mod log;
mod motion;
pub mod palette;
mod scanner;
mod settings;
mod sort;
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
    FirewallMode(crate::store::FirewallMode),
    ApplyProfile(String),
    /// Ask to Connect answer: (app key, allow?)
    Answer(String, bool),
    RunOnStartup(bool),
    Scan,
    RelaunchAdmin,
    OpenDataDir,
    /// Open the app details popup for (app key, click position).
    OpenApp(String, Pos2),
    /// Hash this file and open its VirusTotal page.
    VirusTotal(String),
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
    app_popup: Option<app_popup::AppPopup>,
    page_changed: Instant,
    theme: Option<egui::Theme>,
    theme_fade: (palette::Palette, palette::Palette, Instant),
    /// Debug builds: (last log time, pass number then) for the frames-per-10s diag line.
    frame_log: (Instant, u64),
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
            app_popup: None,
            page_changed: Instant::now() - std::time::Duration::from_secs(1),
            theme: None,
            theme_fade: (palette::dark(), palette::dark(), Instant::now()),
            frame_log: (Instant::now(), 0),
            quit,
            _tray: tray,
        }
    }

    fn run_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Block(key, dir, block) => self.monitor.set_block(&key, dir, block),
                Action::FirewallEnabled(on) => self.monitor.set_firewall_enabled(on),
                Action::FirewallMode(m) => self.monitor.set_firewall_mode(m),
                Action::ApplyProfile(name) => self.monitor.apply_profile(&name),
                Action::Answer(key, allow) => self.monitor.answer_ask(&key, allow),
                Action::RunOnStartup(on) => {
                    let ok = crate::system::set_run_on_startup(on);
                    let mut s = self.monitor.lock();
                    if ok {
                        s.store.data.settings.run_on_startup = on;
                        s.store.mark_dirty();
                    } else {
                        s.toast = Some("Could not change the startup setting".into());
                    }
                }
                Action::Scan => self.monitor.request_scan(),
                Action::RelaunchAdmin => {
                    self.monitor.save();
                    if crate::net::relaunch_as_admin() {
                        std::process::exit(0);
                    }
                }
                Action::OpenApp(key, pos) => self.app_popup = Some(app_popup::AppPopup::new(key, pos)),
                Action::VirusTotal(path) => app_popup::start_virustotal(self.monitor.clone(), ctx.clone(), path),
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
        self.log_frames(ctx);
        let quitting = self.quit.load(Ordering::SeqCst);
        let (close_to_tray, appearance) = {
            let s = self.monitor.lock();
            (s.store.data.settings.close_to_tray && self._tray.is_some(), s.store.data.settings.appearance)
        };
        if ctx.input(|i| i.viewport().close_requested()) && close_to_tray && !quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if quitting {
            self.monitor.save();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        self.update_theme(ctx, appearance);
        let mut actions = Vec::new();
        {
            let monitor = self.monitor.clone();
            let mut guard = monitor.lock();
            let s = &mut *guard;
            if let Some(t) = s.toast.take() {
                self.toast = Some((t, Instant::now()));
            }
            let unread = s.store.data.events.iter().filter(|e| !e.read && e.kind.important()).count();
            theme::set_speed_bits(s.store.data.settings.speed_bits);

            egui::TopBottomPanel::top("nav")
                .exact_height(78.0)
                .frame(egui::Frame::default().fill(c::panel()).stroke(Stroke::new(1.0_f32, c::border())))
                .show(ctx, |ui| self.nav(ui, unread, s.admin));

            if !s.admin {
                egui::TopBottomPanel::top("admin")
                    .exact_height(40.0)
                    .frame(egui::Frame::default().fill(Color32::from_rgb(0x2a, 0x24, 0x14)).inner_margin(egui::Margin::symmetric(18, 0)))
                    .show(ctx, |ui| {
                        ui.horizontal_centered(|ui| {
                            theme::glyph(ui.painter(), Rect::from_center_size(ui.cursor().min + Vec2::new(9.0, 20.0), Vec2::splat(18.0)), Glyph::Lock, c::warn());
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

            if let Some(key) = s.asks.first().cloned() {
                let name = s.app_name(&key);
                let more = s.asks.len() - 1;
                egui::TopBottomPanel::top("ask")
                    .exact_height(46.0)
                    .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(18, 0)))
                    .show(ctx, |ui| {
                        let bar = ui.max_rect().expand2(Vec2::new(18.0, 0.0));
                        let a = c::accent();
                        ui.painter().rect(bar, CornerRadius::ZERO, theme::with_alpha(a, 28), Stroke::new(1.0_f32, theme::with_alpha(a, 70)), egui::StrokeKind::Inside);
                        ui.horizontal_centered(|ui| {
                            ui.add_space(12.0);
                            let ir = Rect::from_center_size(ui.cursor().min + Vec2::new(10.0, 23.0), Vec2::splat(20.0));
                            self.icons.draw(ui, s, &key, ir, &name);
                            ui.add_space(28.0);
                            theme::label(ui, format!("{name} wants to connect to the internet."), theme::semibold(13.5), c::text());
                            let tail = if more > 0 { format!("Blocked until you decide · {more} more waiting") } else { "Blocked until you decide.".to_string() };
                            theme::label(ui, tail, theme::regular(13.5), c::muted());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(8.0);
                                if theme::pill(ui, "Keep blocked", None, false).clicked() {
                                    actions.push(Action::Answer(key.clone(), false));
                                }
                                if theme::primary_button(ui, "Allow").clicked() {
                                    actions.push(Action::Answer(key.clone(), true));
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
            egui::CentralPanel::default().frame(egui::Frame::default().fill(c::bg())).show(ctx, |ui| {
                ui.set_opacity(e);
                ui.add_space(8.0 * (1.0 - e));
                match self.page {
                    Page::Traffic => self.traffic.show(ui, s, &mut self.icons, &mut actions),
                    Page::Firewall => self.firewall.show(ui, s, &mut self.icons, &mut actions),
                    Page::Log => self.log.show(ui, s, &mut self.icons, &mut actions),
                    Page::Scanner => self.scanner.show(ui, s, &mut actions),
                    Page::Hardware => self.hardware.show(ui, s, &mut self.icons),
                    Page::Settings => settings::show(ui, s, &mut actions),
                }
            });
            app_popup::show(ctx, &mut self.app_popup, s, &mut self.icons, &mut actions);
        }
        self.draw_toast(ctx);
        self.run_actions(ctx, actions);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.monitor.save();
    }
}

impl VantageApp {
    /// Follows the Appearance setting: switches the palette (cross-fading over `motion::NORMAL`),
    /// egui's visuals and the native title bar. The first call applies the theme with no fade.
    fn update_theme(&mut self, ctx: &egui::Context, appearance: crate::store::Appearance) {
        let appearance = appearance_override().unwrap_or(appearance);
        let target = crate::store::resolve(appearance, ctx.system_theme());
        let pal_for = |t: egui::Theme| if t == egui::Theme::Light { palette::light() } else { palette::dark() };
        if self.theme != Some(target) {
            let first = self.theme.is_none();
            let from = if first { pal_for(target) } else { palette::current() };
            self.theme = Some(target);
            self.theme_fade = (from, pal_for(target), theme_fade_start(first, Instant::now()));
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

    fn nav(&mut self, ui: &mut egui::Ui, unread: usize, admin: bool) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        let p = ui.painter().clone();

        // Brand
        let logo = Rect::from_min_size(Pos2::new(full.left() + 18.0, full.center().y - 15.0), Vec2::splat(30.0));
        p.rect_filled(logo, CornerRadius::same(8), c::accent());
        theme::glyph(&p, logo.shrink(6.0), Glyph::Traffic, Color32::WHITE);
        p.text(Pos2::new(logo.right() + 10.0, full.center().y - 8.0), Align2::LEFT_CENTER, "Vantage", theme::semibold(17.0), c::text());
        p.text(
            Pos2::new(logo.right() + 10.0, full.center().y + 11.0),
            Align2::LEFT_CENTER,
            if admin { "FULL MONITORING" } else { "LIMITED MODE" },
            theme::semibold(10.5),
            if admin { c::good() } else { c::warn() },
        );

        const TABS: [(Page, Glyph, &str, &str); 5] = [
            (Page::Traffic, Glyph::Traffic, "Traffic", "Monitor"),
            (Page::Firewall, Glyph::Shield, "Firewall", "Rules"),
            (Page::Log, Glyph::Bell, "Activity", "Log"),
            (Page::Scanner, Glyph::Radar, "Network", "Scanner"),
            (Page::Hardware, Glyph::Chip, "Hardware", "Resources"),
        ];
        let rects = tab_rects(full, TABS.len());

        // The highlight glides between tabs; the gear has its own.
        if let Some(i) = TABS.iter().position(|t| t.0 == self.page) {
            let x = motion::value(&ctx, ui.id().with("nav_active"), rects[i].left());
            p.rect_filled(Rect::from_min_size(Pos2::new(x, rects[i].top()), rects[i].size()), CornerRadius::same(10), c::selected());
        }

        let mut clicked = None;
        for (i, (page, g, l1, l2)) in TABS.iter().enumerate() {
            let r = rects[i];
            let mut resp = ui.interact(r, ui.id().with(*l1), Sense::click());
            if *page == Page::Log {
                resp = resp.on_hover_text(unread_label(unread));
            }
            let active = self.page == *page;
            let t = motion::hover(ui, resp.id, resp.hovered());
            if !active && t > 0.0 {
                p.rect_filled(r, CornerRadius::same(10), theme::lerp_color(Color32::TRANSPARENT, c::hover(), t));
            }
            let col = if active { c::accent() } else { c::text() };
            theme::glyph(&p, Rect::from_center_size(Pos2::new(r.left() + 26.0, r.center().y), Vec2::splat(24.0)), *g, col);
            p.text(Pos2::new(r.left() + 48.0, r.center().y - 8.0), Align2::LEFT_CENTER, l1, theme::semibold(13.5), col);
            p.text(Pos2::new(r.left() + 48.0, r.center().y + 9.0), Align2::LEFT_CENTER, l2, theme::semibold(13.5), col);
            if *page == Page::Log && unread > 0 {
                let b = Pos2::new(r.left() + 36.0, r.top() + 12.0);
                let label = if unread > 99 { "99+".to_string() } else { unread.to_string() };
                let w = 10.0 + 7.0 * label.len() as f32;
                p.rect_filled(Rect::from_center_size(b, Vec2::new(w.max(18.0), 18.0)), CornerRadius::same(9), c::bad());
                p.text(b, Align2::CENTER_CENTER, label, theme::semibold(11.0), Color32::WHITE);
            }
            if resp.clicked() {
                clicked = Some(*page);
            }
        }

        let gear = Rect::from_center_size(Pos2::new(full.right() - 34.0, full.center().y), Vec2::splat(38.0));
        let resp = ui.interact(gear, ui.id().with("settings"), Sense::click()).on_hover_text("Settings");
        let active = self.page == Page::Settings;
        let t = motion::hover(ui, resp.id, resp.hovered());
        if active {
            p.rect_filled(gear, CornerRadius::same(10), c::selected());
        } else if t > 0.0 {
            p.rect_filled(gear, CornerRadius::same(10), theme::lerp_color(Color32::TRANSPARENT, c::hover(), t));
        }
        theme::glyph(&p, gear.shrink(9.0), Glyph::Gear, if active { c::accent() } else { theme::lerp_color(c::muted(), c::text(), t) });
        if resp.clicked() {
            clicked = Some(Page::Settings);
        }

        if let Some(pg) = clicked {
            if pg != self.page {
                self.page = pg;
                self.page_changed = Instant::now();
                ctx.request_repaint();
            }
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        let Some(next) = toast_repaint(age) else {
            self.toast = None;
            return;
        };
        let alpha = ((TOAST_LIFE - age) / TOAST_FADE).clamp(0.0, 1.0);
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
        // every frame only while it slides in or fades out; one wake-up at the fade while it holds
        if next.is_zero() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(next);
        }
    }

    /// Debug builds log how many frames ran in each 10 s window, to check idle repaint cost.
    fn log_frames(&mut self, ctx: &egui::Context) {
        if !cfg!(debug_assertions) {
            return;
        }
        let (since, pass) = self.frame_log;
        if since.elapsed().as_secs() >= 10 {
            let now = ctx.cumulative_pass_nr();
            crate::diag::log(&format!("frames: {} in {:.1}s on {}", now - pass, since.elapsed().as_secs_f32(), page_title(self.page)));
            self.frame_log = (Instant::now(), now);
        }
    }
}

/// Toast lifetime and its fade-out at the end, in seconds.
const TOAST_LIFE: f32 = 4.5;
const TOAST_FADE: f32 = 0.4;

/// When a toast of this age next needs a frame: every frame (ZERO) while it slides in or fades out,
/// once at the start of the fade while it holds, and never after it has gone.
fn toast_repaint(age: f32) -> Option<std::time::Duration> {
    let fade_at = TOAST_LIFE - TOAST_FADE;
    if age > TOAST_LIFE {
        None
    } else if age < motion::NORMAL || age >= fade_at {
        Some(std::time::Duration::ZERO)
    } else {
        Some(std::time::Duration::from_secs_f32(fade_at - age))
    }
}

/// Start time of the palette cross-fade. The first theme is applied directly, so its fade is already over.
fn theme_fade_start(first: bool, now: Instant) -> Instant {
    if first {
        now.checked_sub(std::time::Duration::from_secs_f32(motion::NORMAL * 2.0)).unwrap_or(now)
    } else {
        now
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

/// The `n` top tabs (132x50, 8px apart), centred in the bar but never left of the brand block.
fn tab_rects(full: Rect, n: usize) -> Vec<Rect> {
    const W: f32 = 132.0;
    const GAP: f32 = 8.0;
    let total = W * n as f32 + GAP * n.saturating_sub(1) as f32;
    let x0 = (full.center().x - total / 2.0).max(full.left() + 190.0);
    (0..n).map(|i| Rect::from_min_size(Pos2::new(x0 + i as f32 * (W + GAP), full.center().y - 25.0), Vec2::new(W, 50.0))).collect()
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
    fn tab_rects_centered() {
        let full = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 78.0));
        let rects = tab_rects(full, 5);
        assert_eq!(rects.len(), 5);
        assert_eq!(rects[0].left(), (1440.0 - (5.0 * 132.0 + 4.0 * 8.0)) / 2.0);
        assert_eq!(rects[0].size(), Vec2::new(132.0, 50.0));
        assert_eq!(rects[0].center().y, 39.0);
        for w in rects.windows(2) {
            assert_eq!(w[1].left() - w[0].right(), 8.0);
        }
    }

    #[test]
    fn tab_rects_clamp_left() {
        let full = Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 78.0));
        let rects = tab_rects(full, 5);
        assert!(rects[0].left() >= 190.0);
    }

    #[test]
    fn toast_repaints_every_frame_while_sliding_in() {
        assert_eq!(toast_repaint(0.0), Some(std::time::Duration::ZERO));
        assert_eq!(toast_repaint(motion::NORMAL * 0.9), Some(std::time::Duration::ZERO));
    }

    #[test]
    fn toast_hold_waits_for_fade() {
        let d = toast_repaint(1.0).unwrap();
        let want = TOAST_LIFE - TOAST_FADE - 1.0;
        assert!((d.as_secs_f32() - want).abs() < 1e-3, "{d:?}");
        assert!(toast_repaint(motion::NORMAL + 0.01).unwrap() > std::time::Duration::from_secs(3));
    }

    #[test]
    fn toast_repaints_every_frame_while_fading() {
        assert_eq!(toast_repaint(TOAST_LIFE - TOAST_FADE), Some(std::time::Duration::ZERO));
        assert_eq!(toast_repaint(TOAST_LIFE - 0.01), Some(std::time::Duration::ZERO));
    }

    #[test]
    fn toast_gone_needs_no_frames() {
        assert_eq!(toast_repaint(TOAST_LIFE + 0.01), None);
    }

    #[test]
    fn first_theme_has_no_fade() {
        let now = Instant::now();
        let start = theme_fade_start(true, now);
        assert!(now.duration_since(start).as_secs_f32() >= motion::NORMAL);
        assert_eq!(theme_fade_start(false, now), now);
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
