//! Firewall: every app that has touched the network, with click-to-block for each direction.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::app_popup;
use super::sort::{self, SortSpec};
use super::theme::{self, c, Glyph, Icons};
use super::Action;
use crate::monitor::{ConnView, Shared, OTHER_KEY, SYSTEM_KEY};
use crate::net::Proto;
use crate::store::{FirewallMode, Profile};
use crate::system::Dir;

const MODES: [FirewallMode; 3] = [FirewallMode::ClickToBlock, FirewallMode::AskToConnect, FirewallMode::BlockAll];
const MODE_NAMES: [&str; 3] = ["Click to Block", "Ask to Connect", "Block All"];

const ROW_H: f32 = 38.0;
const SUB_H: f32 = 28.0;

#[derive(Default)]
pub struct FirewallPage {
    search: String,
    expanded: HashSet<String>,
    hide_inactive: bool,
    hide_active: bool,
    /// Name being typed for a new profile, while the save field is open.
    new_profile: Option<String>,
}

/// No saved sort: Active by traffic then name, Inactive by last seen. No header shows an arrow.
const NATURAL: SortSpec = SortSpec { col: u8::MAX, desc: false };

/// The sortable fields of one app row.
struct FwRow {
    key: String,
    name: String,
    publisher: Option<String>,
    version: Option<String>,
    block_in: bool,
    block_out: bool,
    hosts: usize,
    down: u64,
    up: u64,
    traffic: u64,
    last_seen: i64,
}

/// Sorts one group (active or inactive) on its own; rows without a publisher or version go last.
fn sort_group(rows: &mut [FwRow], spec: SortSpec, live: bool) {
    if spec.col == u8::MAX {
        rows.sort_by(|a, b| {
            let o = if live { b.traffic.cmp(&a.traffic) } else { b.last_seen.cmp(&a.last_seen) };
            o.then_with(|| sort::cmp_text(&a.name, &b.name))
        });
        return;
    }
    sort::apply(
        rows,
        spec,
        |a, b, col| match col {
            0 => sort::cmp_text(&a.name, &b.name),
            1 => (!a.block_in).cmp(&!b.block_in),
            2 => (!a.block_out).cmp(&!b.block_out),
            3 => sort::cmp_opt(a.publisher.as_deref().map(str::to_lowercase), b.publisher.as_deref().map(str::to_lowercase), spec.desc),
            4 => sort::cmp_opt(a.version.as_deref().map(str::to_lowercase), b.version.as_deref().map(str::to_lowercase), spec.desc),
            5 => a.hosts.cmp(&b.hosts),
            6 => a.down.cmp(&b.down),
            7 => a.up.cmp(&b.up),
            8 => a.traffic.cmp(&b.traffic),
            _ => std::cmp::Ordering::Equal,
        },
        |r| r.name.as_str(),
    );
}

struct Cols {
    app: f32,
    inc: f32,
    out: f32,
    publisher: f32,
    version: f32,
    hosts: f32,
    down: f32,
    up: f32,
    spark: f32,
    end: f32,
}

impl Cols {
    fn new(left: f32, width: f32) -> Self {
        let fixed = 290.0 + 110.0 + 110.0 + 190.0 + 120.0 + 86.0 + 86.0 + 90.0;
        let hosts_w = (width - fixed - 20.0).max(160.0);
        let app = left + 14.0;
        let inc = app + 290.0;
        let out = inc + 110.0;
        let publisher = out + 118.0;
        let version = publisher + 190.0;
        let hosts = version + 120.0;
        let down = hosts + hosts_w + 70.0;
        let up = down + 86.0;
        let spark = up + 16.0;
        Self { app, inc, out, publisher, version, hosts, down, up, spark, end: spark + 70.0 }
    }
}

impl FirewallPage {
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons, actions: &mut Vec<Action>) {
        let full = ui.max_rect();
        self.toolbar(ui, s, actions, full);

        let cols = Cols::new(full.left(), full.width());
        let mut spec = s.store.data.settings.sort_for("firewall", NATURAL);
        if spec.col > 8 && spec.col != u8::MAX {
            spec = NATURAL;
        }
        let (hr, _) = ui.allocate_exact_size(Vec2::new(full.width(), 34.0), Sense::hover());
        ui.painter().rect_filled(hr, CornerRadius::ZERO, c::panel());
        let (top, bot) = (hr.top(), hr.bottom());
        let span = |x0: f32, x1: f32| Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x1, bot));
        let mut changed = false;
        changed |= sort::header(ui, span(cols.app + 30.0, cols.inc - 8.0), "Apps", 0, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
        changed |= sort::header(ui, span(cols.inc + 8.0, cols.out - 4.0), "Incoming", 1, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
        changed |= sort::header(ui, span(cols.out + 8.0, cols.publisher - 4.0), "Outgoing", 2, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
        changed |= sort::header(ui, span(cols.publisher, cols.version - 4.0), "Publisher", 3, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
        changed |= sort::header(ui, span(cols.version, cols.hosts - 4.0), "Version", 4, &mut spec, false, NATURAL, Align2::LEFT_CENTER);
        changed |= sort::header(ui, span(cols.hosts, cols.down - 76.0), "Hosts", 5, &mut spec, true, NATURAL, Align2::LEFT_CENTER);
        theme::glyph(ui.painter(), Rect::from_center_size(Pos2::new(cols.down - 30.0, hr.center().y), Vec2::splat(13.0)), Glyph::Down, c::down());
        theme::glyph(ui.painter(), Rect::from_center_size(Pos2::new(cols.up - 30.0, hr.center().y), Vec2::splat(13.0)), Glyph::Up, c::up());
        changed |= sort::header(ui, span(cols.down - 44.0, cols.down), "", 6, &mut spec, true, NATURAL, Align2::RIGHT_CENTER);
        changed |= sort::header(ui, span(cols.up - 44.0, cols.up), "", 7, &mut spec, true, NATURAL, Align2::RIGHT_CENTER);
        changed |= sort::header(ui, span(cols.spark, cols.end), "Traffic", 8, &mut spec, true, NATURAL, Align2::LEFT_CENTER);
        ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, c::border()));
        if changed {
            s.store.data.settings.set_sort("firewall", spec);
            s.store.mark_dirty();
        }

        let q = self.search.to_lowercase();
        let mut active: Vec<FwRow> = Vec::new();
        let mut inactive: Vec<FwRow> = Vec::new();
        for (k, a) in &s.store.data.apps {
            let name = s.app_name(k);
            let hay = format!("{} {} {}", name, a.publisher.clone().unwrap_or_default(), a.hosts.join(" ")).to_lowercase();
            if !q.is_empty() && !hay.contains(&q) {
                continue;
            }
            let lv = s.apps_live.get(k);
            let live = lv.map(|l| l.in_conns + l.out_conns > 0 || l.rate != [0, 0]).unwrap_or(false);
            let mut hosts: Vec<String> = lv
                .map(|l| l.conns.iter().filter_map(|c| c.host.clone().or_else(|| (!crate::net::is_local(&c.remote) && c.proto == Proto::Tcp).then(|| c.remote.to_string()))).collect())
                .unwrap_or_default();
            hosts.dedup();
            let hosts = hosts.len() + a.hosts.iter().filter(|h| !hosts.contains(h)).count();
            let rate = lv.map(|l| l.rate).unwrap_or([0, 0]);
            let fw = FwRow {
                key: k.clone(),
                name,
                publisher: a.publisher.clone(),
                version: a.version.clone(),
                block_in: a.block_in,
                block_out: a.block_out,
                hosts,
                down: rate[0],
                up: rate[1],
                traffic: rate[0] + rate[1],
                last_seen: a.last_seen,
            };
            if live { active.push(fw) } else { inactive.push(fw) }
        }
        sort_group(&mut active, spec, true);
        sort_group(&mut inactive, spec, false);

        // per-app sparkline series from the last minute of live samples
        let mut spark: HashMap<&str, (Vec<f64>, Vec<f64>)> = HashMap::new();
        let window: Vec<_> = s.live.iter().rev().take(60).collect();
        for FwRow { key: k, .. } in active.iter() {
            let mut d = vec![0.0; window.len()];
            let mut u = vec![0.0; window.len()];
            for (i, l) in window.iter().rev().enumerate() {
                if let Some((_, v)) = l.apps.iter().find(|(kk, _)| kk == k) {
                    d[i] = v[0] as f64;
                    u[i] = v[1] as f64;
                }
            }
            spark.insert(k.as_str(), (d, u));
        }

        let enabled = s.store.data.settings.firewall_enabled;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if section(ui, full.width(), "Active apps", active.len(), !self.hide_active) {
                self.hide_active = !self.hide_active;
            }
            if !self.hide_active {
                for (i, FwRow { key: k, name, .. }) in active.iter().enumerate() {
                    self.row(ui, s, icons, actions, &cols, k, name, i, spark.get(k.as_str()), enabled);
                }
            }
            if section(ui, full.width(), "Inactive apps", inactive.len(), !self.hide_inactive) {
                self.hide_inactive = !self.hide_inactive;
            }
            if !self.hide_inactive {
                for (i, FwRow { key: k, name, .. }) in inactive.iter().enumerate() {
                    self.row(ui, s, icons, actions, &cols, k, name, i, None, enabled);
                }
            }
            if active.is_empty() && inactive.is_empty() {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| theme::label(ui, "No apps match your search.", theme::regular(14.0), c::muted()));
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, s: &mut Shared, actions: &mut Vec<Action>, full: Rect) {
        let (bar, _) = ui.allocate_exact_size(Vec2::new(full.width(), 96.0), Sense::hover());
        let mut b = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(28.0, 16.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let enabled = s.store.data.settings.firewall_enabled;
        b.vertical(|ui| {
            title(ui, Glyph::Shield, "Firewall rules", "Master switch for blocking apps. Blocks apply while this is on.");
            ui.horizontal(|ui| {
                if theme::switch(ui, enabled).clicked() && !s.firewall_busy {
                    actions.push(Action::FirewallEnabled(!enabled));
                }
                theme::label(ui, if enabled { "On" } else { "Off" }, theme::semibold(13.0), if enabled { c::good() } else { c::muted() })
                    .on_hover_text(if enabled { "Firewall rules are on" } else { "Firewall rules are off" });
            });
        });
        b.add_space(28.0);
        sep(&mut b);
        b.add_space(28.0);
        b.vertical(|ui| {
            let engine = match &s.firewall_product {
                Some(p) if !p.to_lowercase().contains("windows") => format!("Enforced at the Windows filtering layer, alongside {p}."),
                _ => "Enforced at the Windows filtering layer.".to_string(),
            };
            title(
                ui,
                Glyph::Lock,
                "Firewall mode",
                &format!(
                    "Click to Block: everything is allowed until you block it.\nAsk to Connect: apps connecting for the first time are blocked until you allow them.\nBlock All: every outgoing connection is blocked.\n{engine}"
                ),
            );
            let mode = s.store.data.settings.firewall_mode;
            let mut sel = MODES.iter().position(|m| *m == mode).unwrap_or(0);
            ui.add_enabled_ui(!s.firewall_busy, |ui| {
                if theme::dropdown(ui, "fw-mode", &MODE_NAMES, &mut sel) {
                    actions.push(Action::FirewallMode(MODES[sel]));
                }
            });
        });
        b.add_space(28.0);
        sep(&mut b);
        b.add_space(28.0);
        b.vertical(|ui| {
            title(ui, Glyph::Shield, "Profile", "Saved sets of block rules. Pick a profile to apply its rules; apps it doesn't list are allowed.");
            ui.horizontal(|ui| self.profiles(ui, s, actions));
        });
        b.add_space(28.0);
        sep(&mut b);
        b.add_space(28.0);
        let blocked = s.store.data.apps.values().filter(|a| a.block_in || a.block_out).count();
        b.vertical(|ui| {
            title(ui, Glyph::Globe, "Blocked apps", "Apps with at least one blocked direction.");
            theme::label(ui, blocked.to_string(), theme::regular(13.0), if blocked > 0 { c::bad() } else { c::muted() });
        });
        b.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::search_box(ui, &mut self.search, 320.0, "Search apps, publishers, hosts").on_hover_text("Filter by app name, publisher or host");
            if s.firewall_busy {
                ui.add_space(12.0);
                ui.add(egui::Spinner::new().size(18.0).color(c::accent()));
                theme::label(ui, "Applying…", theme::regular(13.0), c::muted());
            }
        });
    }

    fn profiles(&mut self, ui: &mut egui::Ui, s: &mut Shared, actions: &mut Vec<Action>) {
        if let Some(name) = self.new_profile.as_mut() {
            let r = ui.add(egui::TextEdit::singleline(name).hint_text("Profile name").desired_width(140.0).font(theme::regular(13.5)));
            if !r.lost_focus() {
                r.request_focus();
            }
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let name = name.trim().to_string();
            if (theme::primary_button(ui, "Save").clicked() || enter) && !name.is_empty() {
                // Snapshot of the current rules
                let profile: Profile = s.store.data.apps.iter().filter(|(_, a)| a.block_in || a.block_out).map(|(k, a)| (k.clone(), (a.block_in, a.block_out))).collect();
                s.store.data.profiles.insert(name.clone(), profile);
                s.store.data.active_profile = Some(name.clone());
                s.store.mark_dirty();
                s.toast = Some(format!("Saved firewall profile \"{name}\""));
                self.new_profile = None;
            } else if theme::pill(ui, "Cancel", None, false).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.new_profile = None;
            }
            return;
        }
        let names: Vec<String> = s.store.data.profiles.keys().cloned().collect();
        if !names.is_empty() {
            let mut options = vec!["No profile".to_string()];
            options.extend(names.iter().cloned());
            let refs: Vec<&str> = options.iter().map(String::as_str).collect();
            let mut sel = s.store.data.active_profile.as_ref().and_then(|a| names.iter().position(|n| n == a)).map(|i| i + 1).unwrap_or(0);
            ui.add_enabled_ui(!s.firewall_busy, |ui| {
                if theme::dropdown(ui, "fw-profile", &refs, &mut sel) {
                    if sel == 0 {
                        s.store.data.active_profile = None;
                        s.store.mark_dirty();
                    } else {
                        actions.push(Action::ApplyProfile(names[sel - 1].clone()));
                    }
                }
            });
        }
        if theme::pill(ui, if names.is_empty() { "Save current rules…" } else { "Save…" }, None, false)
            .on_hover_text("Save the current block rules as a profile")
            .clicked()
        {
            self.new_profile = Some(String::new());
        }
        if let Some(active) = s.store.data.active_profile.clone() {
            if theme::pill(ui, "Delete", None, false).on_hover_text(format!("Delete profile \"{active}\" (rules stay as they are)")).clicked() {
                s.store.data.profiles.remove(&active);
                s.store.data.active_profile = None;
                s.store.mark_dirty();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Shared,
        icons: &mut Icons,
        actions: &mut Vec<Action>,
        cols: &Cols,
        key: &str,
        name: &str,
        i: usize,
        spark: Option<&(Vec<f64>, Vec<f64>)>,
        enabled: bool,
    ) {
        let width = ui.available_width();
        let (row, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
        let live = s.apps_live.get(key).cloned().unwrap_or_default();
        let rec = s.store.data.apps.get(key).cloned().unwrap_or_default();
        let expanded = self.expanded.contains(key);
        let p = ui.painter().clone();
        if i % 2 == 1 {
            p.rect_filled(row, CornerRadius::ZERO, c::row_alt());
        }
        let ht = super::motion::hover(ui, resp.id, resp.hovered());
        if ht > 0.0 {
            p.rect_filled(row, CornerRadius::ZERO, c::hover().gamma_multiply(ht));
        }
        let cy = row.center().y;
        let has_conns = !live.conns.is_empty();
        if has_conns {
            theme::glyph(&p, Rect::from_center_size(Pos2::new(cols.app + 4.0, cy), Vec2::splat(12.0)), if expanded { Glyph::ChevronDown } else { Glyph::Chevron }, c::muted());
        }
        let ir = Rect::from_center_size(Pos2::new(cols.app + 30.0, cy), Vec2::splat(20.0));
        icons.draw(ui, s, key, ir, name);
        theme::text(&p, Pos2::new(cols.app + 48.0, cy), name, theme::regular(14.0), c::text(), cols.inc - cols.app - 60.0);
        app_popup::link(ui, ui.id().with((key, "app-link")), ir, Pos2::new(cols.app + 48.0, cy), name, theme::regular(14.0), cols.inc - cols.app - 60.0, key, actions);

        let protected = rec.path.as_deref().map(crate::system::is_protected).unwrap_or(false);
        let blockable = rec.path.is_some() && key != SYSTEM_KEY && key != OTHER_KEY && !protected;
        for (dir, x, blocked) in [(Dir::In, cols.inc, rec.block_in), (Dir::Out, cols.out, rec.block_out)] {
            let pr = Rect::from_min_size(Pos2::new(x, cy - 12.0), Vec2::new(96.0, 24.0));
            let id = ui.id().with((key, dir == Dir::In));
            let r = theme::status_pill(ui, pr, blocked, blockable && enabled && !s.firewall_busy, id);
            let r = if protected {
                r.on_hover_text("Core Windows process: blocking it would break system services.")
            } else if !blockable {
                r.on_hover_text("This app can't be blocked (system component or unknown path).")
            } else if !enabled {
                r.on_hover_text("Turn on firewall rules to block apps.")
            } else {
                let what = if dir == Dir::In { "incoming" } else { "outgoing" };
                r.on_hover_text(if blocked { format!("Click to allow {what} connections") } else { format!("Click to block {what} connections") })
            };
            if r.clicked() {
                actions.push(Action::Block(key.to_string(), dir, !blocked));
            }
        }
        theme::text(&p, Pos2::new(cols.publisher, cy), rec.publisher.as_deref().unwrap_or("—"), theme::regular(13.5), c::muted(), 180.0);
        theme::text(&p, Pos2::new(cols.version, cy), rec.version.as_deref().unwrap_or("—"), theme::regular(13.5), c::muted(), 110.0);

        // hosts: current first, "+N more" chip
        let mut hosts: Vec<String> = live.conns.iter().filter_map(|c| c.host.clone().or_else(|| (!crate::net::is_local(&c.remote) && c.proto == Proto::Tcp).then(|| c.remote.to_string()))).collect();
        hosts.dedup();
        for h in &rec.hosts {
            if !hosts.contains(h) {
                hosts.push(h.clone());
            }
        }
        let hosts_w = cols.down - 70.0 - cols.hosts;
        if let Some(first) = hosts.first() {
            theme::glyph(&p, Rect::from_center_size(Pos2::new(cols.hosts + 7.0, cy), Vec2::splat(13.0)), Glyph::Globe, c::faint());
            let tr = theme::text(&p, Pos2::new(cols.hosts + 20.0, cy), first, theme::regular(13.5), c::text(), hosts_w - 100.0);
            if hosts.len() > 1 {
                let label = format!("+{} more", hosts.len() - 1);
                let chip = Rect::from_min_size(Pos2::new((tr.right() + 10.0).min(cols.hosts + hosts_w - 70.0), cy - 10.0), Vec2::new(66.0, 20.0));
                p.rect_stroke(chip, CornerRadius::same(4), Stroke::new(1.0_f32, c::border()), egui::StrokeKind::Inside);
                p.text(chip.center(), Align2::CENTER_CENTER, label, theme::regular(12.0), c::muted());
                let all = hosts.iter().take(25).cloned().collect::<Vec<_>>().join("\n");
                ui.interact(chip, ui.id().with((key, "hosts")), Sense::hover()).on_hover_text(all);
            }
        }
        if live.rate[0] > 0 {
            theme::text_right(&p, Pos2::new(cols.down, cy), &theme::rate(live.rate[0] as f64), theme::regular(13.5), c::text());
        }
        if live.rate[1] > 0 {
            theme::text_right(&p, Pos2::new(cols.up, cy), &theme::rate(live.rate[1] as f64), theme::regular(13.5), c::text());
        }
        if let Some((d, u)) = spark {
            theme::spark(&p, Rect::from_min_max(Pos2::new(cols.spark, cy - 11.0), Pos2::new(cols.end - 6.0, cy + 11.0)), d, u);
        }

        if resp.clicked() && has_conns {
            if expanded {
                self.expanded.remove(key);
            } else {
                self.expanded.insert(key.to_string());
            }
        }
        if expanded {
            let mut conns: Vec<&ConnView> = live.conns.iter().filter(|c| c.proto == Proto::Tcp && !crate::net::is_local(&c.remote)).collect();
            conns.sort_by_key(|c| std::cmp::Reverse(c.rate[0] + c.rate[1]));
            let listening: Vec<&ConnView> = live.conns.iter().filter(|c| c.state == crate::net::TcpState::Listen || c.proto == Proto::Udp).collect();
            for cv in conns.iter().take(60) {
                let (r, _) = ui.allocate_exact_size(Vec2::new(width, SUB_H), Sense::hover());
                let p = ui.painter();
                p.rect_filled(r, CornerRadius::ZERO, c::raised().gamma_multiply(0.5));
                let y = r.center().y;
                let host = cv.host.clone().unwrap_or_else(|| cv.remote.to_string());
                p.text(Pos2::new(cols.app + 48.0, y), Align2::LEFT_CENTER, if cv.inbound { "↘ Incoming" } else { "↗ Outgoing" }, theme::regular(12.5), c::muted());
                p.text(Pos2::new(cols.inc + 8.0, y), Align2::LEFT_CENTER, cv.state.label(), theme::regular(12.5), c::muted());
                p.text(Pos2::new(cols.publisher, y), Align2::LEFT_CENTER, format!("PID {}", cv.pid), theme::regular(12.5), c::faint());
                p.text(Pos2::new(cols.version, y), Align2::LEFT_CENTER, format!("Port {}", cv.remote_port), theme::regular(12.5), c::faint());
                theme::text(p, Pos2::new(cols.hosts + 20.0, y), &format!("{host}  ·  {}", cv.remote), theme::regular(12.5), c::text(), hosts_w - 30.0);
                if cv.rate[0] > 0 {
                    theme::text_right(p, Pos2::new(cols.down, y), &theme::rate(cv.rate[0] as f64), theme::regular(12.5), c::muted());
                }
                if cv.rate[1] > 0 {
                    theme::text_right(p, Pos2::new(cols.up, y), &theme::rate(cv.rate[1] as f64), theme::regular(12.5), c::muted());
                }
            }
            if !listening.is_empty() {
                let ports: Vec<String> = listening.iter().take(12).map(|c| format!("{} {}", if c.proto == Proto::Tcp { "TCP" } else { "UDP" }, c.local_port)).collect();
                let (r, _) = ui.allocate_exact_size(Vec2::new(width, SUB_H), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::ZERO, c::raised().gamma_multiply(0.5));
                ui.painter().text(Pos2::new(cols.app + 48.0, r.center().y), Align2::LEFT_CENTER, format!("Listening on {}", ports.join(", ")), theme::regular(12.5), c::faint());
            }
        }
    }
}

fn section(ui: &mut egui::Ui, width: f32, title: &str, count: usize, open: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    let p = ui.painter();
    let ht = super::motion::hover(ui, resp.id, resp.hovered());
    if ht > 0.0 {
        p.rect_filled(r, CornerRadius::ZERO, c::hover().gamma_multiply(ht));
    }
    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, c::border().gamma_multiply(0.5)));
    let resp = resp.on_hover_text(if open { "Click to collapse" } else { "Click to expand" });
    let p = ui.painter();
    theme::glyph(p, Rect::from_center_size(Pos2::new(r.left() + 20.0, r.center().y), Vec2::splat(12.0)), if open { Glyph::ChevronDown } else { Glyph::Chevron }, c::muted());
    let tr = p.text(Pos2::new(r.left() + 36.0, r.center().y), Align2::LEFT_CENTER, title, theme::semibold(14.5), c::text());
    p.text(Pos2::new(tr.right() + 8.0, r.center().y), Align2::LEFT_CENTER, count.to_string(), theme::regular(13.0), c::muted());
    resp.clicked()
}

/// Toolbar heading with a leading glyph and a hover tooltip.
fn title(ui: &mut egui::Ui, g: Glyph, text: &str, tip: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
        theme::glyph(ui.painter(), r, g, c::accent());
        theme::label(ui, text, theme::semibold(15.0), c::text()).on_hover_text(tip);
    });
}

fn sep(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 48.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, c::border());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, publisher: Option<&str>) -> FwRow {
        FwRow { key: name.to_string(), name: name.to_string(), publisher: publisher.map(String::from), version: None, block_in: false, block_out: false, hosts: 0, down: 0, up: 0, traffic: 0, last_seen: 0 }
    }

    #[test]
    fn firewall_sort_keeps_groups() {
        let mut active = vec![row("x", Some("Acme")), row("y", None)];
        let mut inactive = vec![row("z", Some("Zed"))];
        let spec = SortSpec { col: 3, desc: true };
        sort_group(&mut active, spec, true);
        sort_group(&mut inactive, spec, false);
        let names = |v: &[FwRow]| v.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&active), ["x", "y"], "publisher desc, missing publisher last");
        assert_eq!(names(&inactive), ["z"]);
        let spec = SortSpec { col: 3, desc: false };
        sort_group(&mut active, spec, true);
        assert_eq!(names(&active), ["x", "y"], "missing publisher last ascending too");
    }

    #[test]
    fn blocked_sorts_before_allowed() {
        let mut v = vec![row("a", None), row("b", None)];
        v[1].block_out = true;
        sort_group(&mut v, SortSpec { col: 2, desc: false }, true);
        assert_eq!(v[0].name, "b");
    }

    #[test]
    fn firewall_natural_default_matches_base() {
        let mut active = vec![row("a", None), row("b", None)];
        active[0].traffic = 5;
        active[1].traffic = 9;
        let mut inactive = vec![row("c", None), row("d", None)];
        inactive[0].last_seen = 100;
        inactive[1].last_seen = 300;
        sort_group(&mut active, NATURAL, true);
        sort_group(&mut inactive, NATURAL, false);
        assert_eq!(active.iter().map(|r| r.traffic).collect::<Vec<_>>(), [9, 5]);
        assert_eq!(inactive.iter().map(|r| r.last_seen).collect::<Vec<_>>(), [300, 100]);
    }
}
