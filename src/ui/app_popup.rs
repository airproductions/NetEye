//! App details popup: opened from an app's name or icon, shows what NetEye knows about the app.

use std::io::{self, Read};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Align2, Context, CornerRadius, FontId, Id, Key, Order, Pos2, Rect, Sense, Stroke, Vec2};
use sha2::{Digest, Sha256};

use super::theme::{self, c, Glyph, Icons};
use super::Action;
use crate::monitor::{Monitor, Shared, OTHER_KEY, SYSTEM_KEY};
use crate::store::{AppRecord, Event};
use crate::system::Dir;

const SIZE: Vec2 = Vec2::new(470.0, 360.0);
const PAD: f32 = 14.0;
const BODY_H: f32 = 204.0;
const MENU_W: f32 = 190.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Info,
    Alerts,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Block,
    More,
    /// The ⋯ button beside the path.
    Path,
}

pub struct AppPopup {
    pub key: String,
    pub anchor: Pos2,
    pub tab: Tab,
    /// The open footer menu and the button it hangs from.
    menu: Option<(Menu, Rect)>,
}

impl AppPopup {
    pub fn new(key: String, anchor: Pos2) -> Self {
        Self { key, anchor, tab: Tab::Info, menu: None }
    }
}

/// Where the popup goes: right of and below the anchor, flipped when that would overflow,
/// then kept inside `screen` with an 8px margin.
pub fn place(anchor: Pos2, size: Vec2, screen: Rect) -> Rect {
    const MARGIN: f32 = 8.0;
    const GAP: f32 = 12.0;
    let mut x = anchor.x + GAP;
    if x + size.x > screen.right() - MARGIN {
        x = anchor.x - GAP - size.x;
    }
    let mut y = anchor.y + GAP;
    if y + size.y > screen.bottom() - MARGIN {
        y = anchor.y - GAP - size.y;
    }
    x = x.min(screen.right() - MARGIN - size.x).max(screen.left() + MARGIN);
    y = y.min(screen.bottom() - MARGIN - size.y).max(screen.top() + MARGIN);
    Rect::from_min_size(Pos2::new(x, y), size)
}

/// Every event about this app, newest first.
pub fn alerts_for<'a>(events: &'a [Event], key: &str) -> Vec<&'a Event> {
    let mut v: Vec<&Event> = events.iter().filter(|e| e.app.as_deref() == Some(key)).collect();
    v.sort_by(|a, b| b.t.cmp(&a.t));
    v
}

/// A web search for an app's file name ("what is foo.exe").
pub fn search_url(q: &str) -> String {
    let enc: String = q.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("https://duckduckgo.com/?q=%22{enc}%22")
}

pub fn vt_url(sha256_hex: &str) -> String {
    format!("https://www.virustotal.com/gui/file/{sha256_hex}")
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The Name / Path / Version / Publisher rows; missing values read "—", and a missing publisher "Unknown".
pub fn info_rows(rec: Option<&AppRecord>) -> Vec<(&'static str, String)> {
    let dash = || "—".to_string();
    let path = rec.and_then(|r| r.path.clone());
    let file = path.as_deref().and_then(|p| Path::new(p).file_name()).map(|f| f.to_string_lossy().to_string());
    vec![
        ("Name", file.unwrap_or_else(dash)),
        ("Path", path.unwrap_or_else(dash)),
        ("Version", rec.and_then(|r| r.version.clone()).unwrap_or_else(dash)),
        ("Publisher", rec.and_then(|r| r.publisher.clone()).unwrap_or_else(|| "Unknown".to_string())),
    ]
}

/// The `Action::Block` pushes needed to go from the current block state to the wanted one.
pub fn block_changes(cur: (bool, bool), want: (bool, bool)) -> Vec<(Dir, bool)> {
    let mut v = Vec::new();
    if cur.0 != want.0 {
        v.push((Dir::In, want.0));
    }
    if cur.1 != want.1 {
        v.push((Dir::Out, want.1));
    }
    v
}

fn when(t: i64) -> String {
    if t > 0 {
        theme::date_time(t)
    } else {
        "—".to_string()
    }
}

fn block_state(rec: Option<&AppRecord>) -> &'static str {
    match rec.map(|r| (r.block_in, r.block_out)) {
        Some((true, true)) => "Incoming and outgoing blocked",
        Some((true, false)) => "Incoming blocked",
        Some((false, true)) => "Outgoing blocked",
        _ => "Allowed",
    }
}

// ---------------------------------------------------------------------------------------------
// VirusTotal

static VT_BUSY: AtomicBool = AtomicBool::new(false);

/// Hashes the file on a worker thread, then opens its VirusTotal page. A failure becomes a toast.
/// Nothing is uploaded: only the hash goes into the URL.
pub fn start_virustotal(monitor: Monitor, ctx: Context, path: String) {
    if VT_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        match sha256_file(Path::new(&path)) {
            Ok(hash) => {
                let _ = std::process::Command::new("explorer.exe").arg(vt_url(&hash)).spawn();
            }
            Err(e) => monitor.lock().toast = Some(format!("Could not read the file for VirusTotal: {e}")),
        }
        VT_BUSY.store(false, Ordering::SeqCst);
        ctx.request_repaint();
    });
}

// ---------------------------------------------------------------------------------------------
// Click target for an app's name and icon

/// Makes an app's icon and name one click target: hand cursor, underline on hover, and
/// `Action::OpenApp` on click. Returns true on click so the caller can also select the row.
#[allow(clippy::too_many_arguments)]
pub fn link(ui: &egui::Ui, id: Id, icon: Rect, text_pos: Pos2, name: &str, font: FontId, max_w: f32, key: &str, actions: &mut Vec<Action>) -> bool {
    let size = ui.painter().layout_no_wrap(name.to_string(), font, c::text()).size();
    let text = Rect::from_min_size(Pos2::new(text_pos.x, text_pos.y - size.y / 2.0), Vec2::new(size.x.min(max_w.max(1.0)), size.y));
    let hit = icon.union(text);
    let resp = ui.interact(hit, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let y = text.bottom() - 1.0;
        ui.painter().line_segment([Pos2::new(text.left(), y), Pos2::new(text.right(), y)], Stroke::new(1.0_f32, c::text()));
    }
    if resp.clicked() {
        actions.push(Action::OpenApp(key.to_string(), resp.interact_pointer_pos().unwrap_or(hit.center())));
        return true;
    }
    false
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// Draws the popup (and its footer menu) for the app it holds, re-reading the store every call.
pub fn show(ctx: &Context, slot: &mut Option<AppPopup>, s: &mut Shared, icons: &mut Icons, actions: &mut Vec<Action>) {
    let Some(pop) = slot.as_mut() else { return };
    let esc = ctx.input(|i| i.key_pressed(Key::Escape));
    let press = if ctx.input(|i| i.pointer.any_pressed()) { ctx.input(|i| i.pointer.interact_pos()) } else { None };

    let key = pop.key.clone();
    let name = s.app_name(&key);
    let rec = s.store.data.apps.get(&key).cloned();
    let n_alerts = s.store.data.events.iter().filter(|e| e.app.as_deref() == Some(key.as_str())).count();
    let alerts: Vec<(i64, String)> = if pop.tab == Tab::Alerts {
        let evs = s.store.data.events.make_contiguous();
        alerts_for(evs, &key).into_iter().map(|e| (e.t, super::log::event_text(e, &name))).collect()
    } else {
        Vec::new()
    };
    let path = rec.as_ref().and_then(|r| r.path.clone());
    let protected = path.as_deref().map(crate::system::is_protected).unwrap_or(false);
    let blockable = path.is_some() && key != SYSTEM_KEY && key != OTHER_KEY && !protected;
    let can_block = blockable && s.store.data.settings.firewall_enabled && !s.firewall_busy;
    let cur = rec.as_ref().map(|r| (r.block_in, r.block_out)).unwrap_or((false, false));

    let rect = place(pop.anchor, SIZE, ctx.screen_rect());
    let inner_w = SIZE.x - 2.0 * PAD - 2.0;
    let mut vt_click = false;
    let tab = &mut pop.tab;
    let menu = &mut pop.menu;
    let shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: c::shadow() };
    let area = egui::Area::new(Id::new("app_popup")).order(Order::Foreground).fixed_pos(rect.min).fade_in(false).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(c::panel())
            .stroke(Stroke::new(1.0_f32, c::border()))
            .corner_radius(CornerRadius::same(10))
            .shadow(shadow)
            .inner_margin(egui::Margin::same(PAD as i8))
            .show(ui, |ui| {
                ui.set_width(inner_w);
                ui.set_height(SIZE.y - 2.0 * PAD - 2.0);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 7.0);

                // header
                ui.horizontal(|ui| {
                    let (ir, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                    if key == OTHER_KEY {
                        theme::glyph(ui.painter(), ir.shrink(4.0), Glyph::Globe, c::muted());
                    } else {
                        icons.draw(ui, s, &key, ir, &name);
                    }
                    theme::label(ui, name.clone(), theme::semibold(16.0), c::text());
                });

                // tabs
                ui.horizontal(|ui| {
                    for (t, label) in [(Tab::Info, "Info".to_string()), (Tab::Alerts, format!("Alerts ({n_alerts})"))] {
                        let galley = ui.painter().layout_no_wrap(label, theme::semibold(13.5), c::text());
                        let (r, resp) = ui.allocate_exact_size(galley.size() + Vec2::new(12.0, 10.0), Sense::click());
                        let active = *tab == t;
                        let col = if active {
                            c::accent()
                        } else if resp.hovered() {
                            c::text()
                        } else {
                            c::muted()
                        };
                        let gp = r.center() - galley.size() / 2.0 - Vec2::new(0.0, 1.0);
                        ui.painter().galley(gp, galley, col);
                        if active {
                            ui.painter().rect_filled(Rect::from_min_max(Pos2::new(r.left(), r.bottom() - 2.0), r.right_bottom()), CornerRadius::same(1), c::accent());
                        }
                        if resp.clicked() {
                            *tab = t;
                        }
                    }
                });
                let (sep, _) = ui.allocate_exact_size(Vec2::new(inner_w, 1.0), Sense::hover());
                ui.painter().rect_filled(sep, CornerRadius::ZERO, c::border());

                // body
                let (body, _) = ui.allocate_exact_size(Vec2::new(inner_w, BODY_H), Sense::hover());
                let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(body));
                bui.set_clip_rect(body.intersect(ui.clip_rect()));
                if *tab == Tab::Info {
                    let mut rows = info_rows(rec.as_ref());
                    let r = rec.as_ref();
                    rows.push(("First seen", r.map(|r| when(r.first_seen)).unwrap_or_else(|| "—".into())));
                    rows.push(("Last seen", r.map(|r| when(r.last_seen)).unwrap_or_else(|| "—".into())));
                    rows.push(("First network use", r.and_then(|r| r.first_net).map(when).unwrap_or_else(|| "—".into())));
                    rows.push(("Downloaded", r.map(|r| theme::bytes(r.down)).unwrap_or_else(|| "—".into())));
                    rows.push(("Uploaded", r.map(|r| theme::bytes(r.up)).unwrap_or_else(|| "—".into())));
                    rows.push(("Firewall", block_state(r).to_string()));
                    bui.spacing_mut().item_spacing.y = 0.0;
                    for (label, value) in rows {
                        let (rr, resp) = bui.allocate_exact_size(Vec2::new(inner_w, 20.0), Sense::hover());
                        let is_path = label == "Path" && path.is_some();
                        let value_w = inner_w - 118.0 - if is_path { 30.0 } else { 0.0 };
                        let p = bui.painter();
                        p.text(Pos2::new(rr.left(), rr.center().y), Align2::LEFT_CENTER, label, theme::regular(12.5), c::muted());
                        let tr = theme::text(p, Pos2::new(rr.left() + 118.0, rr.center().y), &value, theme::regular(13.0), c::text(), value_w);
                        if label == "Publisher" && value == "Unknown" {
                            // no company name in the file: worth a second look
                            let wr = Rect::from_center_size(Pos2::new(tr.right() + 12.0, rr.center().y), Vec2::splat(14.0));
                            let (cx, top, bot) = (wr.center().x, wr.top() + 1.0, wr.bottom() - 1.0);
                            p.add(egui::Shape::convex_polygon(vec![Pos2::new(cx, top), Pos2::new(wr.right(), bot), Pos2::new(wr.left(), bot)], c::warn(), Stroke::NONE));
                            p.text(Pos2::new(cx, wr.center().y + 1.5), Align2::CENTER_CENTER, "!", theme::semibold(10.0), c::panel());
                            bui.interact(wr, Id::new("app_popup_unknown_pub"), Sense::hover()).on_hover_text("The file doesn't name a publisher");
                        }
                        if value.chars().count() > 40 {
                            resp.on_hover_text(value.clone());
                        }
                        if is_path {
                            // ⋯ in a circle: Copy Path / Open File Location
                            let br = Rect::from_center_size(Pos2::new(rr.right() - 11.0, rr.center().y), Vec2::splat(20.0));
                            let bresp = bui.interact(br, Id::new("app_popup_path_menu"), Sense::click());
                            let open = matches!(menu, Some((Menu::Path, _)));
                            let lit = open || bresp.hovered();
                            let ink = if lit { c::text() } else { c::muted() };
                            let p = bui.painter();
                            if lit {
                                p.circle_filled(br.center(), 9.5, c::hover());
                            }
                            p.circle_stroke(br.center(), 9.0, Stroke::new(1.2_f32, ink));
                            for dx in [-4.0, 0.0, 4.0] {
                                p.circle_filled(br.center() + Vec2::new(dx, 0.0), 1.3, ink);
                            }
                            if bresp.clicked() {
                                *menu = if open { None } else { Some((Menu::Path, br)) };
                            }
                            bresp.on_hover_text("Path options");
                        }
                    }
                } else if alerts.is_empty() {
                    bui.add_space(12.0);
                    theme::label(&mut bui, "No alerts for this app", theme::regular(13.5), c::muted());
                } else {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(&mut bui, 40.0, alerts.len(), |ui, range| {
                        for i in range {
                            let (t, text) = &alerts[i];
                            let (rr, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
                            let p = ui.painter();
                            p.text(Pos2::new(rr.left(), rr.top() + 11.0), Align2::LEFT_CENTER, theme::date_time(*t), theme::regular(12.0), c::muted());
                            theme::text(p, Pos2::new(rr.left(), rr.top() + 28.0), text, theme::regular(13.0), c::text(), rr.width() - 8.0);
                            resp.on_hover_text(text.clone());
                        }
                    });
                }

                // footer
                ui.horizontal(|ui| {
                    let (vt_ok, vt_tip) = match path.as_deref() {
                        None => (false, "No file path for this app"),
                        Some(p) if !Path::new(p).is_file() => (false, "The file was not found on disk"),
                        _ if VT_BUSY.load(Ordering::SeqCst) => (false, "Hashing the file…"),
                        _ => (true, ""),
                    };
                    let resp = ui.add_enabled_ui(vt_ok, |ui| theme::pill(ui, "VirusTotal", None, false)).inner;
                    if vt_ok {
                        vt_click = resp.clicked();
                        resp.on_hover_text("Hashes the file on this PC and opens its VirusTotal page. Nothing is uploaded.");
                    } else {
                        resp.on_disabled_hover_text(vt_tip);
                    }
                    for (m, label) in [(Menu::Block, "Block ▾"), (Menu::More, "More ▾")] {
                        let open = matches!(menu, Some((k, _)) if *k == m);
                        let resp = theme::pill(ui, label, None, open);
                        if resp.clicked() {
                            *menu = if open { None } else { Some((m, resp.rect)) };
                        }
                    }
                });
            });
    });
    let popup_layer = area.response.layer_id;

    // footer menu
    let mut menu_layer = None;
    let mut copied = false;
    let mut toggle_hidden = false;
    if let Some((m, anchor)) = pop.menu {
        let mut close = false;
        let items: Vec<(&str, bool, bool, Option<&str>)> = match m {
            Menu::Block => {
                let tip = (!can_block).then_some("This app can't be blocked right now.");
                [("Allow all", (false, false)), ("Block incoming", (true, false)), ("Block outgoing", (false, true)), ("Block both", (true, true))]
                    .into_iter()
                    .map(|(l, w)| (l, cur == w, can_block, tip))
                    .collect()
            }
            Menu::More => {
                let tip = path.is_none().then_some("No file path for this app");
                let hide = if rec.as_ref().is_some_and(|r| r.hidden) { "Show in app lists" } else { "Hide from app lists" };
                vec![("Open file location", false, path.is_some(), tip), ("Copy path", false, path.is_some(), tip), ("Search online", false, true, None), (hide, false, key != OTHER_KEY, None)]
            }
            Menu::Path => vec![("Copy Path", false, true, None), ("Open File Location", false, true, None)],
        };
        // the path menu drops below its ⋯ button; footer menus open upwards
        let (at, pivot) = if m == Menu::Path {
            (anchor.right_bottom() + Vec2::new(0.0, 4.0), Align2::RIGHT_TOP)
        } else {
            (anchor.left_top() - Vec2::new(0.0, 4.0), Align2::LEFT_BOTTOM)
        };
        let ar = egui::Area::new(Id::new("app_popup_menu"))
            .order(Order::Tooltip)
            .fixed_pos(at)
            .pivot(pivot)
            .fade_in(false)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(c::panel())
                    .stroke(Stroke::new(1.0_f32, c::border()))
                    .corner_radius(CornerRadius::same(8))
                    .shadow(shadow)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, (label, checked, enabled, tip)) in items.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(Vec2::new(MENU_W, 28.0), if *enabled { Sense::click() } else { Sense::hover() });
                            if *enabled && resp.hovered() {
                                ui.painter().rect_filled(r, CornerRadius::same(6), c::hover());
                            }
                            let col = if *enabled { c::text() } else { c::faint() };
                            if *checked {
                                ui.painter().text(Pos2::new(r.left() + 12.0, r.center().y), Align2::CENTER_CENTER, "✓", theme::semibold(13.0), c::accent());
                            }
                            ui.painter().text(Pos2::new(r.left() + 26.0, r.center().y), Align2::LEFT_CENTER, label, theme::regular(13.5), col);
                            if let (false, Some(tip)) = (*enabled, tip) {
                                resp.clone().on_hover_text(*tip);
                            }
                            if *enabled && resp.clicked() {
                                close = true;
                                match (m, i) {
                                    (Menu::Block, i) => {
                                        let want = [(false, false), (true, false), (false, true), (true, true)][i];
                                        for (dir, block) in block_changes(cur, want) {
                                            actions.push(Action::Block(key.clone(), dir, block));
                                        }
                                    }
                                    (Menu::More, 0) | (Menu::Path, 1) => {
                                        if let Some(p) = &path {
                                            let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{p}\"")).spawn();
                                        }
                                    }
                                    (Menu::More, 2) => {
                                        // the file name says more to a search engine than the display name
                                        let q = path.as_deref().and_then(|p| Path::new(p).file_name()).map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| name.clone());
                                        let _ = std::process::Command::new("explorer.exe").arg(search_url(&q)).spawn();
                                    }
                                    (Menu::More, 3) => toggle_hidden = true,
                                    (Menu::More, _) | (Menu::Path, _) => {
                                        if let Some(p) = &path {
                                            ctx.copy_text(p.clone());
                                            copied = true;
                                        }
                                    }
                                }
                            }
                        }
                    });
            });
        menu_layer = Some(ar.response.layer_id);
        if close {
            pop.menu = None;
        }
    }
    if copied {
        s.toast = Some("Path copied".into());
    }
    if toggle_hidden {
        if let Some(a) = s.store.data.apps.get_mut(&key) {
            a.hidden = !a.hidden;
            let msg = if a.hidden { format!("{name} is hidden from the app lists. Unhide it in Settings.") } else { format!("{name} is back in the app lists") };
            s.store.mark_dirty();
            s.toast = Some(msg);
        }
    }

    if vt_click {
        if let Some(p) = path {
            actions.push(Action::VirusTotal(p));
        }
    }

    // Esc closes the menu first, then the popup. A press on the page behind (or on nothing) closes it too.
    if esc {
        if pop.menu.take().is_none() {
            *slot = None;
        }
    } else if let Some(pos) = press {
        let layer = ctx.layer_id_at(pos);
        let inside_menu = layer.is_some() && layer == menu_layer;
        if layer == Some(popup_layer) {
            // a press in the popup but off the open menu's button dismisses the menu
            if pop.menu.is_some_and(|(_, b)| !b.contains(pos)) {
                pop.menu = None;
            }
        } else if !inside_menu {
            if layer.is_none_or(|l| l.order == Order::Background) {
                *slot = None;
            } else {
                pop.menu = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::EventKind;

    fn ev(id: u64, t: i64, app: Option<&str>) -> Event {
        Event { id, t, kind: EventKind::NewApp, app: app.map(String::from), subject: String::new(), read: false }
    }

    #[test]
    fn place_stays_inside() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 680.0));
        let size = Vec2::new(360.0, 300.0);
        let r = place(Pos2::new(1090.0, 670.0), size, screen);
        assert!(screen.shrink(8.0).contains_rect(r), "{r:?}");
        assert_eq!(r.size(), size);
        let r = place(Pos2::new(10.0, 10.0), size, screen);
        assert!(r.left() > 10.0 && r.top() > 10.0, "{r:?}");
        assert!(screen.shrink(8.0).contains_rect(r));
    }

    #[test]
    fn alerts_for_filters_and_orders() {
        let events = vec![ev(1, 1, Some("a")), ev(2, 2, Some("b")), ev(3, 3, Some("a")), ev(4, 4, None)];
        let got: Vec<i64> = alerts_for(&events, "a").iter().map(|e| e.t).collect();
        assert_eq!(got, vec![3, 1]);
        assert!(alerts_for(&events, "zzz").is_empty());
    }

    #[test]
    fn search_url_quotes_and_encodes() {
        assert_eq!(search_url("svc host.exe"), "https://duckduckgo.com/?q=%22svc%20host.exe%22");
    }

    #[test]
    fn vt_url_format() {
        assert_eq!(vt_url("ab12"), "https://www.virustotal.com/gui/file/ab12");
    }

    #[test]
    fn sha256_known_value() {
        let p = std::env::temp_dir().join(format!("neteye_sha_{}.bin", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        let h = sha256_file(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(sha256_file(&p).is_err());
    }

    #[test]
    fn info_rows_for_bare_record() {
        let want = vec![("Name", "—".to_string()), ("Path", "—".to_string()), ("Version", "—".to_string()), ("Publisher", "Unknown".to_string())];
        assert_eq!(info_rows(None), want);
        assert_eq!(info_rows(Some(&AppRecord::default())), want);
        let rec = AppRecord { path: Some("C:/a.exe".into()), version: Some("1.2".into()), publisher: Some("Acme".into()), ..Default::default() };
        assert_eq!(info_rows(Some(&rec)), vec![("Name", "a.exe".into()), ("Path", "C:/a.exe".into()), ("Version", "1.2".into()), ("Publisher", "Acme".into())]);
    }

    #[test]
    fn block_changes_only_differences() {
        assert!(block_changes((false, false), (false, false)).is_empty());
        let v = block_changes((false, true), (true, false));
        assert_eq!(v.len(), 2);
        assert!(matches!(v[0], (Dir::In, true)) && matches!(v[1], (Dir::Out, false)));
        let v = block_changes((true, true), (false, false));
        assert!(matches!(v[..], [(Dir::In, false), (Dir::Out, false)]));
    }
}
