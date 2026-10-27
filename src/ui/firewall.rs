//! Firewall: every app that has touched the network, with click-to-block for each direction.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::theme::{self, c, Glyph, Icons};
use super::Action;
use crate::monitor::{ConnView, Shared, OTHER_KEY, SYSTEM_KEY};
use crate::net::Proto;
use crate::system::Dir;

const ROW_H: f32 = 38.0;
const SUB_H: f32 = 28.0;

#[derive(Default)]
pub struct FirewallPage {
    search: String,
    expanded: HashSet<String>,
    hide_inactive: bool,
    hide_active: bool,
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
        let (hr, _) = ui.allocate_exact_size(Vec2::new(full.width(), 34.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(hr, CornerRadius::ZERO, c::PANEL);
        let hy = hr.center().y;
        let head = |x: f32, t: &str| {
            p.text(Pos2::new(x, hy), Align2::LEFT_CENTER, t, theme::regular(13.0), c::MUTED);
        };
        head(cols.app + 30.0, "Apps");
        head(cols.inc + 8.0, "Incoming");
        head(cols.out + 8.0, "Outgoing");
        head(cols.publisher, "Publisher");
        head(cols.version, "Version");
        head(cols.hosts, "Hosts");
        theme::glyph(p, Rect::from_center_size(Pos2::new(cols.down - 30.0, hy), Vec2::splat(13.0)), Glyph::Down, c::DOWN);
        theme::glyph(p, Rect::from_center_size(Pos2::new(cols.up - 30.0, hy), Vec2::splat(13.0)), Glyph::Up, c::UP);
        head(cols.spark, "Traffic");
        p.line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, c::BORDER));

        let q = self.search.to_lowercase();
        let mut active: Vec<(String, String)> = Vec::new();
        let mut inactive: Vec<(String, String)> = Vec::new();
        for (k, a) in &s.store.data.apps {
            let name = s.app_name(k);
            let hay = format!("{} {} {}", name, a.publisher.clone().unwrap_or_default(), a.hosts.join(" ")).to_lowercase();
            if !q.is_empty() && !hay.contains(&q) {
                continue;
            }
            let live = s.apps_live.get(k).map(|l| l.in_conns + l.out_conns > 0 || l.rate != [0, 0]).unwrap_or(false);
            if live { active.push((k.clone(), name)) } else { inactive.push((k.clone(), name)) }
        }
        let rate_of = |k: &str| s.apps_live.get(k).map(|l| l.rate[0] + l.rate[1]).unwrap_or(0);
        active.sort_by(|a, b| rate_of(&b.0).cmp(&rate_of(&a.0)).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
        inactive.sort_by(|a, b| {
            let la = s.store.data.apps[&a.0].last_seen;
            let lb = s.store.data.apps[&b.0].last_seen;
            lb.cmp(&la)
        });

        // per-app sparkline series from the last minute of live samples
        let mut spark: HashMap<&str, (Vec<f64>, Vec<f64>)> = HashMap::new();
        let window: Vec<_> = s.live.iter().rev().take(60).collect();
        for (k, _) in active.iter() {
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
                for (i, (k, name)) in active.iter().enumerate() {
                    self.row(ui, s, icons, actions, &cols, k, name, i, spark.get(k.as_str()), enabled);
                }
            }
            if section(ui, full.width(), "Inactive apps", inactive.len(), !self.hide_inactive) {
                self.hide_inactive = !self.hide_inactive;
            }
            if !self.hide_inactive {
                for (i, (k, name)) in inactive.iter().enumerate() {
                    self.row(ui, s, icons, actions, &cols, k, name, i, None, enabled);
                }
            }
            if active.is_empty() && inactive.is_empty() {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| theme::label(ui, "No apps match your search.", theme::regular(14.0), c::MUTED));
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, s: &Shared, actions: &mut Vec<Action>, full: Rect) {
        let (bar, _) = ui.allocate_exact_size(Vec2::new(full.width(), 96.0), Sense::hover());
        let mut b = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(28.0, 16.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let enabled = s.store.data.settings.firewall_enabled;
        b.vertical(|ui| {
            theme::label(ui, "Firewall rules", theme::semibold(15.0), c::TEXT);
            ui.horizontal(|ui| {
                if theme::switch(ui, enabled).clicked() && !s.firewall_busy {
                    actions.push(Action::FirewallEnabled(!enabled));
                }
                theme::label(ui, if enabled { "On" } else { "Off" }, theme::semibold(13.0), if enabled { c::GOOD } else { c::MUTED });
            });
        });
        b.add_space(28.0);
        sep(&mut b);
        b.add_space(28.0);
        b.vertical(|ui| {
            theme::label(ui, "Click to block", theme::semibold(15.0), c::TEXT);
            let engine = match &s.firewall_product {
                Some(p) if !p.to_lowercase().contains("windows") => format!("Enforced at the filtering layer · works alongside {p}"),
                _ => "Enforced at the Windows filtering layer".to_string(),
            };
            theme::label(ui, engine, theme::regular(13.0), c::MUTED)
                .on_hover_text("Blocks are Windows Filtering Platform filters, which apply no matter which firewall product is active. They persist across reboots.");
        });
        b.add_space(28.0);
        sep(&mut b);
        b.add_space(28.0);
        let blocked = s.store.data.apps.values().filter(|a| a.block_in || a.block_out).count();
        b.vertical(|ui| {
            theme::label(ui, "Blocked apps", theme::semibold(15.0), c::TEXT);
            theme::label(ui, blocked.to_string(), theme::regular(13.0), if blocked > 0 { c::BAD } else { c::MUTED });
        });
        b.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::search_box(ui, &mut self.search, 320.0, "Search apps, publishers, hosts");
            if s.firewall_busy {
                ui.add_space(12.0);
                ui.add(egui::Spinner::new().size(18.0).color(c::ACCENT));
                theme::label(ui, "Applying…", theme::regular(13.0), c::MUTED);
            }
        });
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
        if resp.hovered() {
            p.rect_filled(row, CornerRadius::ZERO, c::HOVER);
        } else if i % 2 == 1 {
            p.rect_filled(row, CornerRadius::ZERO, c::ROW_ALT);
        }
        let cy = row.center().y;
        let has_conns = !live.conns.is_empty();
        if has_conns {
            theme::glyph(&p, Rect::from_center_size(Pos2::new(cols.app + 4.0, cy), Vec2::splat(12.0)), if expanded { Glyph::ChevronDown } else { Glyph::Chevron }, c::MUTED);
        }
        let ir = Rect::from_center_size(Pos2::new(cols.app + 30.0, cy), Vec2::splat(20.0));
        icons.draw(ui, s, key, ir, name);
        theme::text(&p, Pos2::new(cols.app + 48.0, cy), name, theme::regular(14.0), c::TEXT, cols.inc - cols.app - 60.0);

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
        theme::text(&p, Pos2::new(cols.publisher, cy), rec.publisher.as_deref().unwrap_or("—"), theme::regular(13.5), c::MUTED, 180.0);
        theme::text(&p, Pos2::new(cols.version, cy), rec.version.as_deref().unwrap_or("—"), theme::regular(13.5), c::MUTED, 110.0);

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
            theme::glyph(&p, Rect::from_center_size(Pos2::new(cols.hosts + 7.0, cy), Vec2::splat(13.0)), Glyph::Globe, c::FAINT);
            let tr = theme::text(&p, Pos2::new(cols.hosts + 20.0, cy), first, theme::regular(13.5), c::TEXT, hosts_w - 100.0);
            if hosts.len() > 1 {
                let label = format!("+{} more", hosts.len() - 1);
                let chip = Rect::from_min_size(Pos2::new((tr.right() + 10.0).min(cols.hosts + hosts_w - 70.0), cy - 10.0), Vec2::new(66.0, 20.0));
                p.rect_stroke(chip, CornerRadius::same(4), Stroke::new(1.0_f32, c::BORDER), egui::StrokeKind::Inside);
                p.text(chip.center(), Align2::CENTER_CENTER, label, theme::regular(12.0), c::MUTED);
                let all = hosts.iter().take(25).cloned().collect::<Vec<_>>().join("\n");
                ui.interact(chip, ui.id().with((key, "hosts")), Sense::hover()).on_hover_text(all);
            }
        }
        if live.rate[0] > 0 {
            theme::text_right(&p, Pos2::new(cols.down, cy), &theme::rate(live.rate[0] as f64), theme::regular(13.5), c::TEXT);
        }
        if live.rate[1] > 0 {
            theme::text_right(&p, Pos2::new(cols.up, cy), &theme::rate(live.rate[1] as f64), theme::regular(13.5), c::TEXT);
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
                p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(0x17, 0x19, 0x1e));
                let y = r.center().y;
                let host = cv.host.clone().unwrap_or_else(|| cv.remote.to_string());
                p.text(Pos2::new(cols.app + 48.0, y), Align2::LEFT_CENTER, if cv.inbound { "↘ Incoming" } else { "↗ Outgoing" }, theme::regular(12.5), c::MUTED);
                p.text(Pos2::new(cols.inc + 8.0, y), Align2::LEFT_CENTER, cv.state.label(), theme::regular(12.5), c::MUTED);
                p.text(Pos2::new(cols.publisher, y), Align2::LEFT_CENTER, format!("PID {}", cv.pid), theme::regular(12.5), c::FAINT);
                p.text(Pos2::new(cols.version, y), Align2::LEFT_CENTER, format!("Port {}", cv.remote_port), theme::regular(12.5), c::FAINT);
                theme::text(p, Pos2::new(cols.hosts + 20.0, y), &format!("{host}  ·  {}", cv.remote), theme::regular(12.5), c::TEXT, hosts_w - 30.0);
                if cv.rate[0] > 0 {
                    theme::text_right(p, Pos2::new(cols.down, y), &theme::rate(cv.rate[0] as f64), theme::regular(12.5), c::MUTED);
                }
                if cv.rate[1] > 0 {
                    theme::text_right(p, Pos2::new(cols.up, y), &theme::rate(cv.rate[1] as f64), theme::regular(12.5), c::MUTED);
                }
            }
            if !listening.is_empty() {
                let ports: Vec<String> = listening.iter().take(12).map(|c| format!("{} {}", if c.proto == Proto::Tcp { "TCP" } else { "UDP" }, c.local_port)).collect();
                let (r, _) = ui.allocate_exact_size(Vec2::new(width, SUB_H), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(0x17, 0x19, 0x1e));
                ui.painter().text(Pos2::new(cols.app + 48.0, r.center().y), Align2::LEFT_CENTER, format!("Listening on {}", ports.join(", ")), theme::regular(12.5), c::FAINT);
            }
        }
    }
}

fn section(ui: &mut egui::Ui, width: f32, title: &str, count: usize, open: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(r, CornerRadius::ZERO, c::HOVER);
    }
    theme::glyph(p, Rect::from_center_size(Pos2::new(r.left() + 20.0, r.center().y), Vec2::splat(12.0)), if open { Glyph::ChevronDown } else { Glyph::Chevron }, c::MUTED);
    let tr = p.text(Pos2::new(r.left() + 36.0, r.center().y), Align2::LEFT_CENTER, title, theme::semibold(14.5), c::TEXT);
    p.text(Pos2::new(tr.right() + 8.0, r.center().y), Align2::LEFT_CENTER, count.to_string(), theme::regular(13.0), c::MUTED);
    resp.clicked()
}

fn sep(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 48.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, c::BORDER);
}
