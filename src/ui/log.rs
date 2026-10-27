//! Activity log: first network activity, connectivity changes, app updates, new devices…

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::theme::{self, c, Glyph, Icons};
use crate::monitor::Shared;
use crate::store::{Event, EventKind};

const FILTERS: [&str; 2] = ["Important", "Everything"];
const PERIODS: [&str; 4] = ["All time", "Today", "Last 7 days", "Last 30 days"];
const ROW_H: f32 = 78.0;

#[derive(Default)]
pub struct LogPage {
    filter: usize,
    period: usize,
    search: String,
    fresh: Option<HashSet<u64>>,
}

impl LogPage {
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons) {
        let full = ui.max_rect();
        // Remember what was unread when the page opened, then mark everything read.
        let fresh = self.fresh.get_or_insert_with(|| s.store.data.events.iter().filter(|e| !e.read).map(|e| e.id).collect()).clone();
        if s.store.data.events.iter().any(|e| !e.read) {
            for e in s.store.data.events.iter_mut() {
                e.read = true;
            }
            s.store.mark_dirty();
        }

        let mut clear = false;
        let (bar, _) = ui.allocate_exact_size(Vec2::new(full.width(), 72.0), Sense::hover());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, c::panel());
        ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0_f32, c::border()));
        let mut b = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(24.0, 16.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        theme::dropdown(&mut b, "log-filter", &FILTERS, &mut self.filter);
        theme::dropdown(&mut b, "log-period", &PERIODS, &mut self.period);
        b.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::pill(ui, "Clear log", None, false).clicked() {
                clear = true;
            }
            ui.add_space(4.0);
            theme::search_box(ui, &mut self.search, 300.0, "Search");
        });
        if clear {
            s.store.data.events.clear();
            s.store.mark_dirty();
            self.fresh = Some(HashSet::new());
        }

        let now = chrono::Local::now();
        let today = now.date_naive();
        let min_t = match self.period {
            1 => today.and_hms_opt(0, 0, 0).and_then(|d| d.and_local_timezone(chrono::Local).single()).map(|d| d.timestamp()).unwrap_or(0),
            2 => now.timestamp() - 7 * 86400,
            3 => now.timestamp() - 30 * 86400,
            _ => 0,
        };
        let q = self.search.to_lowercase();
        let events: Vec<Event> = s
            .store
            .data
            .events
            .iter()
            .filter(|e| e.t >= min_t && (self.filter == 1 || e.kind.important()))
            .filter(|e| {
                q.is_empty() || {
                    let app = e.app.as_ref().map(|a| s.app_name(a)).unwrap_or_default();
                    format!("{} {} {}", app, e.subject, describe(e.kind)).to_lowercase().contains(&q)
                }
            })
            .cloned()
            .collect();

        if events.is_empty() {
            let p = ui.painter_at(full);
            theme::empty_state(&p, full.center(), "Nothing here yet. Network events will show up as they happen.");
            return;
        }

        let width = full.width();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut last_day = None;
            let mut idx_in_group = 0;
            for e in &events {
                let day = theme::local(e.t).date_naive();
                if last_day != Some(day) {
                    last_day = Some(day);
                    idx_in_group = 0;
                    ui.add_space(12.0);
                    let label = if day == today {
                        "Today".to_string()
                    } else if Some(day) == today.pred_opt() {
                        "Yesterday".to_string()
                    } else {
                        day.format("%A, %-d %B %Y").to_string()
                    };
                    let (r, _) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::hover());
                    ui.painter().text(Pos2::new(r.left() + 48.0, r.center().y), Align2::LEFT_CENTER, label, theme::semibold(15.5), c::text());
                }
                let (r, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                let card = Rect::from_min_max(Pos2::new(r.left() + 40.0, r.top()), Pos2::new(r.right() - 40.0, r.bottom()));
                let p = ui.painter().clone();
                p.rect_filled(card, CornerRadius::ZERO, if resp.hovered() { c::hover() } else { c::panel() });
                if idx_in_group > 0 {
                    p.line_segment([Pos2::new(card.left() + 20.0, card.top()), Pos2::new(card.right() - 20.0, card.top())], Stroke::new(1.0_f32, c::border()));
                }
                idx_in_group += 1;
                let cy = card.center().y;
                badge(&p, Pos2::new(card.left() + 44.0, cy), e.kind);
                if fresh.contains(&e.id) {
                    p.circle_filled(Pos2::new(card.left() + 14.0, cy), 4.0, c::accent());
                }

                let mut x = card.left() + 88.0;
                let max_x = card.right() - 260.0;
                if let Some(app) = &e.app {
                    let name = s.app_name(app);
                    icons.draw(ui, s, app, Rect::from_center_size(Pos2::new(x + 9.0, cy), Vec2::splat(18.0)), &name);
                    x += 26.0;
                    x = seg(&p, x, cy, &name, true, max_x);
                }
                for (t, bold) in segments(e) {
                    x = seg(&p, x, cy, &t, bold, max_x);
                }
                p.text(Pos2::new(card.right() - 28.0, cy - 11.0), Align2::RIGHT_CENTER, e.kind.source(), theme::regular(13.0), c::muted());
                p.text(Pos2::new(card.right() - 28.0, cy + 11.0), Align2::RIGHT_CENTER, theme::local(e.t).format("%-I:%M %P").to_string(), theme::regular(13.0), c::muted());
            }
            ui.add_space(24.0);
        });
    }
}

fn describe(k: EventKind) -> &'static str {
    match k {
        EventKind::NewApp => "initiated its first network connection to",
        EventKind::InternetUp => "Internet connection now available.",
        EventKind::InternetDown => "Internet connection was lost.",
        EventKind::VersionChanged => "version changed",
        EventKind::NewDevice => "New device joined your network:",
        EventKind::Firewall => "firewall",
        EventKind::DataCap => "Daily data usage limit reached",
    }
}

fn segments(e: &Event) -> Vec<(String, bool)> {
    match e.kind {
        EventKind::NewApp => vec![(" initiated its first network connection to ".into(), false), (e.subject.clone(), true)],
        EventKind::InternetUp | EventKind::InternetDown => vec![(describe(e.kind).into(), false)],
        EventKind::VersionChanged => {
            let (a, b) = e.subject.split_once(" → ").unwrap_or(("?", &e.subject));
            vec![(" version changed from ".into(), false), (a.into(), true), (" to ".into(), false), (format!("{b}."), true)]
        }
        EventKind::NewDevice => vec![("New device joined your network: ".into(), false), (e.subject.clone(), true)],
        EventKind::Firewall => {
            let lead = if e.app.is_some() { format!(" — {}.", e.subject.to_lowercase()) } else { format!("{}.", e.subject) };
            vec![(lead, false)]
        }
        EventKind::DataCap => vec![("You reached your daily data usage limit of ".into(), false), (format!("{}.", e.subject), true)],
    }
}

fn seg(p: &egui::Painter, x: f32, y: f32, t: &str, bold: bool, max_x: f32) -> f32 {
    if x >= max_x {
        return x;
    }
    let font = if bold { theme::semibold(14.5) } else { theme::regular(14.5) };
    let r = theme::text(p, Pos2::new(x, y), t, font, c::text(), max_x - x);
    // keep a trailing space visible: layout trims it, so add the advance manually
    r.right() + if t.ends_with(' ') { 4.0 } else { 0.0 }
}

fn badge(p: &egui::Painter, center: Pos2, kind: EventKind) {
    let (col, g, txt) = match kind {
        EventKind::NewApp => (c::good(), None, Some("NEW")),
        EventKind::InternetUp => (c::good(), Some(Glyph::Plug), None),
        EventKind::InternetDown => (c::orange(), Some(Glyph::Plug), None),
        EventKind::VersionChanged => (c::warn(), Some(Glyph::Info), None),
        EventKind::NewDevice => (c::accent(), Some(Glyph::Scanner), None),
        EventKind::Firewall => (Color32::from_rgb(0x64, 0x6c, 0x7c), Some(Glyph::Shield), None),
        EventKind::DataCap => (c::bad(), Some(Glyph::Info), None),
    };
    p.circle_filled(center, 20.0, col);
    if let Some(g) = g {
        theme::glyph(p, Rect::from_center_size(center, Vec2::splat(22.0)), g, Color32::WHITE);
    }
    if let Some(t) = txt {
        p.text(center, Align2::CENTER_CENTER, t, theme::semibold(11.5), Color32::WHITE);
    }
}
