//! Hardware resources: per-app CPU / GPU / memory / disk with stacked 5-minute charts.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::motion;
use super::theme::{self, c, Glyph, Icons};
use crate::hw::ProcSample;
use crate::monitor::{Shared, HW_SECONDS};

const ROW_H: f32 = 32.0;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Name,
    Cpu,
    Gpu,
    #[default]
    Memory,
    Disk,
}

#[derive(Default)]
pub struct HardwarePage {
    selected: Option<String>,
    expanded: HashSet<String>,
    search: String,
    sort: SortBy,
}

fn pct(v: f32) -> String {
    if v < 0.5 { String::new() } else { format!("{v:.0} %") }
}

impl HardwarePage {
    pub fn show(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons) {
        let full = ui.max_rect();
        let area = Rect::from_min_max(Pos2::new(full.left() + 16.0, full.top() + 4.0), Pos2::new(full.right() - 16.0, full.bottom() - 16.0));
        let left_w = 600.0f32.min(area.width() * 0.45);
        let left = Rect::from_min_max(area.min, Pos2::new(area.left() + left_w, area.bottom()));
        let right = Rect::from_min_max(Pos2::new(left.right() + 12.0, area.top()), area.max);
        theme::glass(ui.painter(), left, 14);
        theme::glass(ui.painter(), right, 14);
        let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left.shrink2(Vec2::new(6.0, 6.0))));
        self.list(&mut lui, s, icons);
        let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right.shrink2(Vec2::new(24.0, 6.0))));
        self.charts(&mut rui, s);
    }

    fn list(&mut self, ui: &mut egui::Ui, s: &mut Shared, icons: &mut Icons) {
        let r = ui.max_rect();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            theme::label(ui, "Process resource usage", theme::semibold(15.0), c::text());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                theme::search_box(ui, &mut self.search, 200.0, "Search");
            });
        });
        ui.add_space(8.0);
        let (xc, xg, xm, xd) = (r.right() - 300.0, r.right() - 228.0, r.right() - 96.0, r.right() - 16.0);
        let (hr, _) = ui.allocate_exact_size(Vec2::new(r.width(), 30.0), Sense::hover());
        let heads = [(SortBy::Name, r.left() + 46.0, "Apps", false), (SortBy::Cpu, xc, "CPU", true), (SortBy::Gpu, xg, "GPU", true), (SortBy::Memory, xm, "Memory", true), (SortBy::Disk, xd, "Disk", true)];
        for (sort, x, label, right) in heads {
            let w = if right { 70.0 } else { 120.0 };
            let hit = if right { Rect::from_min_max(Pos2::new(x - w, hr.top()), Pos2::new(x, hr.bottom())) } else { Rect::from_min_size(Pos2::new(x, hr.top()), Vec2::new(w, hr.height())) };
            let resp = ui.interact(hit, ui.id().with(label), Sense::click());
            let col = if self.sort == sort || resp.hovered() { c::text() } else { c::muted() };
            let font = theme::semibold(12.0);
            let p = ui.painter();
            if right { theme::text_right(p, Pos2::new(x, hr.center().y), label, font, col); } else { p.text(Pos2::new(x, hr.center().y), Align2::LEFT_CENTER, label, font, col); }
            if resp.clicked() {
                self.sort = sort;
            }
        }
        ui.painter().line_segment([hr.left_bottom(), hr.right_bottom()], Stroke::new(1.0_f32, c::border()));

        let q = self.search.to_lowercase();
        let mut apps: Vec<(String, String, ProcSample, Vec<(u32, String, ProcSample)>)> = s
            .hw
            .apps
            .iter()
            .map(|(k, a)| (k.clone(), s.app_name(k), a.total, a.procs.clone()))
            .filter(|(_, n, _, _)| q.is_empty() || n.to_lowercase().contains(&q))
            .collect();
        match self.sort {
            SortBy::Name => apps.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase())),
            SortBy::Cpu => apps.sort_by(|a, b| b.2.cpu.total_cmp(&a.2.cpu)),
            SortBy::Gpu => apps.sort_by(|a, b| b.2.gpu.total_cmp(&a.2.gpu)),
            SortBy::Memory => apps.sort_by_key(|a| std::cmp::Reverse(a.2.memory)),
            SortBy::Disk => apps.sort_by(|a, b| b.2.disk.total_cmp(&a.2.disk)),
        }
        let sys = s.hw.history.back().copied().unwrap_or_default();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // All apps
            let (row, resp) = ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::click());
            let p = ui.painter();
            let ht = motion::hover(ui, resp.id, resp.hovered());
            p.rect_filled(row, CornerRadius::same(8), if self.selected.is_none() { c::selected() } else { theme::lerp_color(Color32::TRANSPARENT, c::hover(), ht) });
            theme::glyph(p, Rect::from_center_size(Pos2::new(row.left() + 26.0, row.center().y), Vec2::splat(16.0)), Glyph::Grid, c::accent());
            p.text(Pos2::new(row.left() + 46.0, row.center().y), Align2::LEFT_CENTER, "All apps", theme::semibold(14.0), c::text());
            let y = row.center().y;
            theme::text_right(p, Pos2::new(xc, y), &format!("{:.0} %", sys.cpu), theme::regular(13.5), c::text());
            theme::text_right(p, Pos2::new(xg, y), &format!("{:.0} %", sys.gpu), theme::regular(13.5), c::text());
            theme::text_right(p, Pos2::new(xm, y), &theme::bytes(sys.mem_used), theme::regular(13.5), c::text());
            theme::text_right(p, Pos2::new(xd, y), &theme::rate(sys.disk_read + sys.disk_write), theme::regular(13.5), c::text());
            if resp.clicked() {
                self.selected = None;
            }

            for (i, (key, name, t, procs)) in apps.iter().enumerate() {
                let (row, resp) = ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::click());
                let selected = self.selected.as_deref() == Some(key);
                let expanded = self.expanded.contains(key);
                let p = ui.painter().clone();
                let ht = motion::hover(ui, resp.id, resp.hovered());
                let base = if selected { c::selected() } else if i % 2 == 0 { c::row_alt() } else { Color32::TRANSPARENT };
                p.rect_filled(row, CornerRadius::same(8), if selected { base } else { theme::lerp_color(base, c::hover(), ht) });
                let y = row.center().y;
                let chev = Rect::from_center_size(Pos2::new(row.left() + 8.0 + 4.0, y), Vec2::splat(16.0));
                let chev_resp = ui.interact(chev.expand(4.0), ui.id().with(("chev", key)), Sense::click());
                theme::glyph(&p, chev.shrink(2.0), if expanded { Glyph::ChevronDown } else { Glyph::Chevron }, c::muted());
                icons.draw(ui, s, key, Rect::from_center_size(Pos2::new(row.left() + 34.0, y), Vec2::splat(18.0)), name);
                let label = if procs.len() > 1 { format!("{name} ({})", procs.len()) } else { name.clone() };
                theme::text(&p, Pos2::new(row.left() + 52.0, y), &label, theme::regular(14.0), c::text(), xc - row.left() - 120.0);
                theme::text_right(&p, Pos2::new(xc, y), &pct(t.cpu), theme::regular(13.5), c::text());
                theme::text_right(&p, Pos2::new(xg, y), &pct(t.gpu), theme::regular(13.5), c::text());
                theme::text_right(&p, Pos2::new(xm, y), &theme::bytes(t.memory), theme::regular(13.5), c::text());
                if t.disk >= 1000.0 {
                    theme::text_right(&p, Pos2::new(xd, y), &theme::rate(t.disk), theme::regular(13.5), c::text());
                }
                if chev_resp.clicked() {
                    if expanded { self.expanded.remove(key); } else { self.expanded.insert(key.clone()); }
                } else if resp.clicked() {
                    self.selected = Some(key.clone());
                }
                if expanded {
                    let mut procs = procs.clone();
                    procs.sort_by_key(|p| std::cmp::Reverse(p.2.memory));
                    for (pid, exe, ps) in procs {
                        let (sr, _) = ui.allocate_exact_size(Vec2::new(r.width(), 26.0), Sense::hover());
                        let p = ui.painter();
                        p.rect_filled(sr, CornerRadius::same(6), c::row_alt());
                        let y = sr.center().y;
                        theme::text(p, Pos2::new(sr.left() + 52.0, y), &format!("{exe}  ·  PID {pid}"), theme::regular(12.5), c::muted(), xc - sr.left() - 120.0);
                        theme::text_right(p, Pos2::new(xc, y), &pct(ps.cpu), theme::regular(12.5), c::muted());
                        theme::text_right(p, Pos2::new(xg, y), &pct(ps.gpu), theme::regular(12.5), c::muted());
                        theme::text_right(p, Pos2::new(xm, y), &theme::bytes(ps.memory), theme::regular(12.5), c::muted());
                        if ps.disk >= 1000.0 {
                            theme::text_right(p, Pos2::new(xd, y), &theme::rate(ps.disk), theme::regular(12.5), c::muted());
                        }
                    }
                }
            }
        });
    }

    fn charts(&mut self, ui: &mut egui::Ui, s: &mut Shared) {
        let r = ui.max_rect();
        let p = ui.painter_at(r.expand(4.0));
        let stats_h = 84.0;
        let axis_h = 28.0;
        let top = r.top() + 50.0;
        let title = match &self.selected {
            Some(k) => s.app_name(k),
            None => "All apps".into(),
        };
        p.text(Pos2::new(r.left(), r.top() + 26.0), Align2::LEFT_CENTER, &title, theme::semibold(16.0), c::text());
        p.text(Pos2::new(r.right(), r.top() + 26.0), Align2::RIGHT_CENTER, "5 Minutes", theme::regular(13.5), c::muted());

        let n = HW_SECONDS;
        let pad = |v: Vec<f64>| -> Vec<f64> {
            let mut out = vec![0.0; n.saturating_sub(v.len())];
            out.extend(v);
            out
        };
        let sys = s.hw.history.back().copied().unwrap_or_default();
        let (cpu, mem, disk, gpu, mem_max, mem_label): (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>, f64, &str) = match self.selected.as_ref().and_then(|k| s.hw.app_history.get(k)) {
            Some(h) => {
                let mem: Vec<f64> = h.iter().map(|x| x.memory as f64).collect();
                let mm = theme::nice_max(mem.iter().cloned().fold(0.0, f64::max) * 1.2);
                (
                    pad(h.iter().map(|x| x.cpu as f64).collect()),
                    pad(mem),
                    pad(h.iter().map(|x| x.disk).collect()),
                    pad(h.iter().map(|x| x.gpu as f64).collect()),
                    mm,
                    "Memory",
                )
            }
            None => {
                let h = &s.hw.history;
                (
                    pad(h.iter().map(|x| x.cpu as f64).collect()),
                    pad(h.iter().map(|x| x.mem_used as f64 / x.mem_total.max(1) as f64 * 100.0).collect()),
                    pad(h.iter().map(|x| x.disk_read + x.disk_write).collect()),
                    pad(h.iter().map(|x| x.gpu as f64).collect()),
                    0.0,
                    "Memory",
                )
            }
        };
        let app_mode = self.selected.is_some() && mem_max > 0.0;
        let charts_h = r.bottom() - stats_h - axis_h - top;
        let each = charts_h / 4.0;
        let pct_max = |v: &[f64]| (theme::nice_max(v.iter().cloned().fold(0.0, f64::max) * 1.15)).clamp(5.0, 100.0);
        let disk_max = theme::nice_max(disk.iter().cloned().fold(0.0, f64::max) * 1.15).max(1e6);
        let rows: [(&str, &Vec<f64>, f64, String); 4] = [
            ("CPU", &cpu, pct_max(&cpu), format!("{:.0}%", pct_max(&cpu))),
            (mem_label, &mem, if app_mode { mem_max } else { 100.0 }, if app_mode { theme::bytes(mem_max as u64) } else { "100%".into() }),
            ("Disk transfer rate", &disk, disk_max, theme::rate(disk_max)),
            ("GPU", &gpu, pct_max(&gpu), format!("{:.0}%", pct_max(&gpu))),
        ];
        let line_col = egui::Color32::from_rgb(0x5b, 0x8d, 0xff);
        for (i, (label, vals, max, max_label)) in rows.iter().enumerate() {
            let cr = Rect::from_min_max(Pos2::new(r.left(), top + each * i as f32 + 12.0), Pos2::new(r.right(), top + each * (i + 1) as f32 - 10.0));
            theme::chart_frame(&p, cr, label, max_label);
            let plot = Rect::from_min_max(Pos2::new(cr.left(), cr.top() + 14.0), cr.max);
            theme::area(&p, plot, vals, *max, line_col);
        }
        // time axis
        let ay = r.bottom() - stats_h - axis_h + 8.0;
        let now = crate::store::now();
        for i in 0..=5 {
            let x = r.left() + r.width() * i as f32 / 5.0;
            let t = now - (n as i64) + (n as i64) * i / 5;
            let align = if i == 0 { Align2::LEFT_TOP } else if i == 5 { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
            p.text(Pos2::new(x, ay), align, theme::local(t).format("%-I:%M:%S %p").to_string(), theme::regular(12.0), c::muted());
        }
        // stats row
        let sr = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - stats_h), r.max);
        p.line_segment([sr.left_top(), sr.right_top()], Stroke::new(1.0_f32, c::border()));
        let stats = [
            (format!("{:.0} %", sys.cpu), "CPU usage"),
            (theme::bytes(sys.mem_used), "Memory usage"),
            (theme::rate(sys.disk_read), "Disk read"),
            (theme::rate(sys.disk_write), "Disk write"),
            (format!("{:.0} %", sys.gpu), "GPU usage"),
            (theme::bytes(sys.gpu_mem), "GPU memory"),
        ];
        let w = sr.width() / stats.len() as f32;
        for (i, (v, l)) in stats.iter().enumerate() {
            let cx = sr.left() + w * (i as f32 + 0.5);
            p.text(Pos2::new(cx, sr.center().y - 10.0), Align2::CENTER_CENTER, v, theme::semibold(17.0), c::text());
            p.text(Pos2::new(cx, sr.center().y + 14.0), Align2::CENTER_CENTER, *l, theme::regular(13.0), c::muted());
        }
    }
}
