//! Geometry, hit testing and painting of the monitor. Coordinates are window pixels.

use super::model::{Section, SortKey, VRow};
use super::sample::{Perf, HISTORY};
use super::{Monitor, Tab};
use crate::dash::charts::{arrow, load_color, sparkline};
use crate::dash::layout::Rect;
use crate::dash::sysinfo::{format_uptime, human_bytes, human_kb, human_rate};
use crate::gfx::{color, fill_rect, fill_rounded, stroke_rounded};
use crate::text::{Role, Weight};
use crate::theme::Theme;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

pub const ROW_H: f32 = 28.0;
const PAD: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tab(Tab),
    Link,
    Search,
    Mine,
    Kernel,
    Sort(SortKey),
    /// A list row; true when the click landed on its fold arrow.
    Row(usize, bool),
    End(usize),
    Force(usize),
    FootEnd,
    FootForce,
    ConfirmYes,
    ConfirmNo,
    Perf(usize),
}

/// What the hit test needs to know about the current state.
pub struct HitState {
    pub tab: Tab,
    pub rows: usize,
    pub scroll: f32,
    pub has_sel: bool,
    pub confirm: bool,
    pub perf_items: usize,
}

pub struct Lay {
    pub w: f32,
    pub h: f32,
    pub popup: bool,
    pub tabs: [Rect; 2],
    pub link: Rect,
    pub search: Rect,
    pub mine: Rect,
    pub kernel: Rect,
    pub head: Rect,
    pub list: Rect,
    pub foot: Rect,
    pub btn_end: Rect,
    pub btn_force: Rect,
    pub side: Rect,
    pub main: Rect,
    /// name, pid, CPU, memory, disk
    pub cols: [Rect; 5],
    pub dialog: Rect,
    pub dlg_yes: Rect,
    pub dlg_no: Rect,
}

impl Lay {
    pub fn new(w: f32, h: f32, popup: bool) -> Lay {
        let kernel = Rect::new(w - PAD - 118.0, 56.0, 118.0, 30.0);
        let mine = Rect::new(kernel.x - 8.0 - 112.0, 56.0, 112.0, 30.0);
        let head = Rect::new(0.0, 96.0, w, 24.0);
        let foot = Rect::new(0.0, h - 46.0, w, 46.0);
        let btn_force = Rect::new(w - PAD - 112.0, foot.y + 8.0, 112.0, 30.0);
        let io_r = w - PAD;
        let mem_r = io_r - 96.0;
        let cpu_r = mem_r - 96.0;
        let pid_r = cpu_r - 80.0;
        let col = |x: f32, r: f32| Rect::new(x, head.y, r - x, head.h);
        let dialog = Rect::new((w - 460.0) / 2.0, (h - 200.0) / 2.0, 460.0, 200.0);
        Lay {
            w,
            h,
            popup,
            tabs: [Rect::new(PAD, 10.0, 104.0, 30.0), Rect::new(PAD + 108.0, 10.0, 120.0, 30.0)],
            link: Rect::new(w - PAD - 196.0, 10.0, 196.0, 30.0),
            search: Rect::new(PAD, 56.0, (w * 0.4).clamp(180.0, 300.0), 30.0),
            mine,
            kernel,
            head,
            list: Rect::new(0.0, head.bottom(), w, (foot.y - head.bottom()).max(0.0)),
            foot,
            btn_end: Rect::new(btn_force.x - 8.0 - 100.0, btn_force.y, 100.0, 30.0),
            btn_force,
            side: Rect::new(0.0, 52.0, 220.0, (h - 52.0).max(0.0)),
            main: Rect::new(220.0, 52.0, (w - 220.0).max(0.0), (h - 52.0).max(0.0)),
            cols: [col(PAD, pid_r - 76.0), col(pid_r - 76.0, pid_r), col(cpu_r - 80.0, cpu_r), col(mem_r - 96.0, mem_r), col(io_r - 96.0, io_r)],
            dialog,
            dlg_yes: Rect::new(dialog.right() - PAD - 120.0, dialog.bottom() - PAD - 32.0, 120.0, 32.0),
            dlg_no: Rect::new(dialog.right() - PAD - 120.0 - 8.0 - 100.0, dialog.bottom() - PAD - 32.0, 100.0, 32.0),
        }
    }

    /// The inline End / Force buttons of the row whose top is `y`.
    pub fn row_buttons(&self, y: f32) -> (Rect, Rect) {
        let force = Rect::new(self.w - PAD - 84.0, y + 3.0, 84.0, ROW_H - 6.0);
        (Rect::new(force.x - 6.0 - 72.0, y + 3.0, 72.0, ROW_H - 6.0), force)
    }

    pub fn perf_item(&self, i: usize) -> Rect {
        Rect::new(10.0, self.side.y + 8.0 + i as f32 * 66.0, self.side.w - 20.0, 60.0)
    }

    pub fn hit(&self, x: f32, y: f32, s: &HitState) -> Option<Hit> {
        if s.confirm {
            return if self.dlg_yes.contains(x, y) {
                Some(Hit::ConfirmYes)
            } else if self.dlg_no.contains(x, y) {
                Some(Hit::ConfirmNo)
            } else {
                None
            };
        }
        if let Some(i) = self.tabs.iter().position(|r| r.contains(x, y)) {
            return Some(Hit::Tab(Tab::ALL[i]));
        }
        if self.popup && self.link.contains(x, y) {
            return Some(Hit::Link);
        }
        match s.tab {
            Tab::Processes => {
                if self.search.contains(x, y) {
                    return Some(Hit::Search);
                }
                if self.mine.contains(x, y) {
                    return Some(Hit::Mine);
                }
                if self.kernel.contains(x, y) {
                    return Some(Hit::Kernel);
                }
                if self.head.contains(x, y) {
                    let keys = [SortKey::Name, SortKey::Pid, SortKey::Cpu, SortKey::Mem, SortKey::Io];
                    return self.cols.iter().position(|c| c.contains(x, y)).map(|i| Hit::Sort(keys[i]));
                }
                if s.has_sel {
                    if self.btn_end.contains(x, y) {
                        return Some(Hit::FootEnd);
                    }
                    if self.btn_force.contains(x, y) {
                        return Some(Hit::FootForce);
                    }
                }
                if self.list.contains(x, y) {
                    let i = ((y - self.list.y + s.scroll) / ROW_H) as usize;
                    if i < s.rows {
                        let top = self.list.y + i as f32 * ROW_H - s.scroll;
                        let (end, force) = self.row_buttons(top);
                        return Some(if force.contains(x, y) {
                            Hit::Force(i)
                        } else if end.contains(x, y) {
                            Hit::End(i)
                        } else {
                            Hit::Row(i, x < 40.0)
                        });
                    }
                }
                None
            }
            Tab::Performance => (0..s.perf_items).find(|&i| self.perf_item(i).contains(x, y)).map(Hit::Perf),
        }
    }
}

// ---------------------------------------------------------------- performance items

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfKind {
    Cpu,
    Mem,
    Disk,
    Net(usize),
    Gpu(usize),
    Battery,
}

pub fn perf_items(p: &Perf) -> Vec<PerfKind> {
    let mut v = vec![PerfKind::Cpu, PerfKind::Mem, PerfKind::Disk];
    v.extend((0..p.nets.len()).map(PerfKind::Net));
    v.extend((0..p.gpus.len()).map(PerfKind::Gpu));
    if p.battery.is_some() {
        v.push(PerfKind::Battery);
    }
    v
}

fn pct(f: f32) -> String {
    format!("{:.0}%", f * 100.0)
}

/// `12.3%` for a CPU share, `0%` when idle.
pub fn cpu_text(c: f32) -> String {
    if c < 0.05 {
        "0%".into()
    } else if c < 10.0 {
        format!("{c:.1}%")
    } else {
        format!("{c:.0}%")
    }
}

fn rate_text(r: f64) -> String {
    if r < 1.0 {
        "0 B/s".into()
    } else {
        human_rate(r)
    }
}

// ---------------------------------------------------------------- painting

impl Monitor {
    fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(px * 0.55 * s.chars().count() as f32, |f| f.measure(s, px, w))
    }

    #[allow(clippy::too_many_arguments)] // a drawing helper: target, text, position, size, weight, colour
    fn txt(&mut self, pm: &mut Pixmap, s: &str, x: f32, top: f32, px: f32, w: Weight, c: Color) -> f32 {
        self.fonts.as_mut().map_or(0.0, |f| f.draw_as(Role::Ui, pm, s, x.round(), top.round(), px, w, c))
    }

    #[allow(clippy::too_many_arguments)]
    fn mono(&mut self, pm: &mut Pixmap, s: &str, x: f32, top: f32, px: f32, w: Weight, c: Color) -> f32 {
        self.fonts.as_mut().map_or(0.0, |f| f.draw_as(Role::Mono, pm, s, x.round(), top.round(), px, w, c))
    }

    #[allow(clippy::too_many_arguments)]
    fn txt_r(&mut self, pm: &mut Pixmap, s: &str, right: f32, top: f32, px: f32, w: Weight, c: Color) {
        let width = self.measure(s, px, w);
        self.txt(pm, s, right - width, top, px, w, c);
    }

    #[allow(clippy::too_many_arguments)]
    fn txt_c(&mut self, pm: &mut Pixmap, s: &str, cx: f32, top: f32, px: f32, w: Weight, c: Color) {
        let width = self.measure(s, px, w);
        self.txt(pm, s, cx - width / 2.0, top, px, w, c);
    }

    fn fit(&self, s: &str, px: f32, w: Weight, max: f32) -> String {
        if self.measure(s, px, w) <= max {
            return s.to_string();
        }
        let mut chars: Vec<char> = s.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let cand: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
            if self.measure(&cand, px, w) <= max {
                return cand;
            }
        }
        String::new()
    }

    fn mid(y: f32, h: f32, px: f32) -> f32 {
        y + (h - px * 1.25) / 2.0
    }

    pub(super) fn paint(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        let lay = self.lay();
        pm.fill(Color::TRANSPARENT);
        let radius = if self.popup() { th.r(1.8) } else { 0.0 };
        let base = if !self.popup() {
            th.background.color(1.0)
        } else if self.alpha {
            th.overlay_c()
        } else {
            th.overlay.color(1.0)
        };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, base);
        self.draw_top(pm, &th, &lay);
        match self.tab {
            Tab::Processes => self.draw_processes(pm, &th, &lay),
            Tab::Performance => self.draw_performance(pm, &th, &lay),
        }
        if self.confirm.is_some() {
            self.draw_confirm(pm, &th, &lay);
        }
        if self.popup() {
            stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), th.outline_c(), 1.0);
        }
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }

    fn base_color(&self, th: &Theme) -> Color {
        if !self.popup() {
            th.background.color(1.0)
        } else if self.alpha {
            th.overlay_c()
        } else {
            th.overlay.color(1.0)
        }
    }

    fn draw_top(&mut self, pm: &mut Pixmap, th: &Theme, lay: &Lay) {
        for (i, t) in Tab::ALL.iter().enumerate() {
            let r = lay.tabs[i];
            let selected = *t == self.tab;
            if selected {
                fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, th.ink(0.14));
            } else if self.hover == Some(Hit::Tab(*t)) {
                fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, th.ink(0.07));
            }
            let c = if selected { th.text_c() } else { th.sub() };
            self.txt_c(pm, t.label(), r.cx(), Self::mid(r.y, r.h, 13.0), 13.0, if selected { Weight::SemiBold } else { Weight::Medium }, c);
        }
        if lay.popup {
            let r = lay.link;
            let hov = self.hover == Some(Hit::Link);
            if hov {
                fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, th.ink(0.08));
            }
            self.txt_c(pm, "Open Activity Monitor  ↗", r.cx(), Self::mid(r.y, r.h, 12.0), 12.0, Weight::Medium, if hov { th.text_c() } else { th.accent_c() });
        }
        fill_rect(pm, PAD, 47.0, lay.w - 2.0 * PAD, 1.0, th.sep());
    }

    fn chip(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect, label: &str, on: bool, hov: bool) {
        let bg = if on { th.accent.color(0.28) } else { th.ink(if hov { 0.12 } else { 0.07 }) };
        fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, bg);
        let c = if on { th.text_c() } else { th.sub() };
        self.txt_c(pm, label, r.cx(), Self::mid(r.y, r.h, 12.0), 12.0, Weight::Medium, c);
    }

    fn draw_processes(&mut self, pm: &mut Pixmap, th: &Theme, lay: &Lay) {
        // Search box.
        let s = lay.search;
        fill_rounded(pm, s.x, s.y, s.w, s.h, s.h / 2.0, th.ink(0.08));
        stroke_rounded(pm, s.x + 0.5, s.y + 0.5, s.w - 1.0, s.h - 1.0, s.h / 2.0 - 0.5, th.ink(0.14), 1.0);
        let q = self.view.query.clone();
        if q.is_empty() {
            self.txt(pm, "Search processes (just type)", s.x + 14.0, Self::mid(s.y, s.h, 12.5), 12.5, Weight::Regular, th.faint());
        } else {
            let shown = self.fit(&q, 12.5, Weight::Regular, s.w - 28.0);
            let wd = self.txt(pm, &shown, s.x + 14.0, Self::mid(s.y, s.h, 12.5), 12.5, Weight::Regular, th.text_c());
            fill_rect(pm, (s.x + 15.0 + wd).round(), s.y + 7.0, 1.0, s.h - 14.0, th.text_c());
        }
        let (mine, kernel) = (self.view.mine_only, self.view.kernel);
        self.chip(pm, th, lay.mine, "My processes", mine, self.hover == Some(Hit::Mine));
        self.chip(pm, th, lay.kernel, "Kernel threads", kernel, self.hover == Some(Hit::Kernel));

        // Rows (only the visible ones are painted).
        let n = self.rows.len();
        if n == 0 {
            let msg = if self.snap.procs.is_empty() { "Reading processes…" } else { "No process matches" };
            self.txt_c(pm, msg, lay.w / 2.0, lay.list.y + 50.0, 13.0, Weight::Regular, th.faint());
        }
        let first = (self.scroll / ROW_H) as usize;
        let last = (((self.scroll + lay.list.h) / ROW_H).ceil() as usize + 1).min(n);
        let hover_row = match self.hover {
            Some(Hit::Row(i, _)) | Some(Hit::End(i)) | Some(Hit::Force(i)) => Some(i),
            _ => None,
        };
        let sel_row = self.selected_row();
        for i in first..last {
            let y = lay.list.y + i as f32 * ROW_H - self.scroll;
            if y + ROW_H <= lay.list.y || y >= lay.list.bottom() {
                continue;
            }
            self.draw_row(pm, th, lay, i, y, hover_row == Some(i), sel_row == Some(i));
        }
        // The rows scroll under the header and footer: paint those over a clean strip.
        fill_rect(pm, 0.0, lay.head.y, lay.w, lay.head.h, self.base_color(th));
        // Column headers.
        let titles = [("Name", SortKey::Name, false), ("PID", SortKey::Pid, true), ("CPU", SortKey::Cpu, true), ("Memory", SortKey::Mem, true), ("Disk", SortKey::Io, true)];
        let total_cpu: f32 = self.snap.perf.cpu * 100.0;
        for (i, (title, key, right)) in titles.iter().enumerate() {
            let c = lay.cols[i];
            let active = self.view.sort == *key;
            if self.hover == Some(Hit::Sort(*key)) {
                fill_rect(pm, c.x, c.y, c.w, c.h, th.ink(0.06));
            }
            let label = match (*key, active) {
                (SortKey::Cpu, _) => format!("CPU {:.0}%", total_cpu),
                (SortKey::Mem, _) => format!("Memory {}", pct(self.snap.perf.mem.used_frac())),
                _ => (*title).to_string(),
            };
            let col = if active { th.text_c() } else { th.sub() };
            let top = Self::mid(c.y, c.h, 11.5);
            if *right {
                let tw = self.measure(&label, 11.5, Weight::SemiBold);
                self.txt(pm, &label, c.right() - 10.0 - tw, top, 11.5, Weight::SemiBold, col);
                if active {
                    arrow(pm, c.right() - 10.0 - tw - 8.0, c.cy(), 7.0, !self.view.desc, th.sub());
                }
            } else {
                let tw = self.txt(pm, &label, c.x + 38.0 + 20.0, top, 11.5, Weight::SemiBold, col);
                if active {
                    arrow(pm, c.x + 38.0 + 20.0 + tw + 10.0, c.cy(), 7.0, !self.view.desc, th.sub());
                }
            }
        }
        fill_rect(pm, 0.0, lay.head.bottom() - 1.0, lay.w, 1.0, th.sep());

        fill_rect(pm, 0.0, lay.foot.y, lay.w, lay.foot.h, self.base_color(th));
        self.draw_foot(pm, th, lay);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_row(&mut self, pm: &mut Pixmap, th: &Theme, lay: &Lay, i: usize, y: f32, hover: bool, selected: bool) {
        let (snap, row) = (self.snap.clone(), self.rows[i]);
        let c = &lay.cols;
        // Clip the top: rows that start above the list are cut by repainting the header afterwards (the list is below it).
        if let VRow::Section(s, count) = row {
            let label = match s {
                Section::Apps => format!("Apps ({count})"),
                Section::Background => format!("Background processes ({count})"),
            };
            self.txt(pm, &label, PAD, Self::mid(y, ROW_H, 12.0), 12.0, Weight::Bold, th.accent_c());
            return;
        }
        if selected {
            fill_rect(pm, 0.0, y, lay.w, ROW_H, th.accent.color(0.20));
        } else if hover {
            fill_rect(pm, 0.0, y, lay.w, ROW_H, th.ink(0.06));
        }
        let (p, g, child) = match row {
            VRow::Group(gi) => {
                let g = &snap.groups[gi];
                (&snap.procs[g.members[0]], Some(g), false)
            }
            VRow::Child(gi, mi) => (&snap.procs[snap.groups[gi].members[mi]], None, true),
            VRow::Section(..) => return,
        };
        let multi = g.is_some_and(|g| g.members.len() > 1);
        let expanded = multi && matches!(self.rows.get(i + 1), Some(VRow::Child(..)));
        let (cpu, rss, io, name) = match g {
            Some(g) => (g.cpu, g.rss, g.io, g.name.clone()),
            None => (p.cpu, p.rss, p.io_total(), p.name.clone()),
        };
        let top = Self::mid(y, ROW_H, 12.5);
        let mut x = PAD + 2.0;
        if multi {
            let tri = if expanded { [(x, y + 11.0), (x + 9.0, y + 11.0), (x + 4.5, y + 17.0)] } else { [(x + 2.0, y + 8.0), (x + 2.0, y + 18.0), (x + 8.0, y + 13.0)] };
            let mut pb = PathBuilder::new();
            pb.move_to(tri[0].0, tri[0].1);
            pb.line_to(tri[1].0, tri[1].1);
            pb.line_to(tri[2].0, tri[2].1);
            pb.close();
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(th.sub());
                paint.anti_alias = true;
                pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            }
        }
        x += 22.0;
        if child {
            x += 20.0;
        }
        // Icon: the app's, a quiet dot for the rest.
        match g.and_then(|g| g.icon.clone()) {
            Some(icon) => {
                let pix = self.icons.get(&icon, 36).clone();
                crate::gfx::blit_scaled(pm, &pix, x, y + (ROW_H - 18.0) / 2.0, 18.0, 18.0, 1.0);
            }
            None => fill_rounded(pm, x + 5.0, y + 10.0, 8.0, 8.0, 2.0, th.ink(if child { 0.12 } else { 0.22 })),
        }
        x += 26.0;
        let limit = c[0].right() - x - 8.0;
        let mut label = name;
        if multi && !expanded {
            label = format!("{label}  ({})", g.map_or(0, |g| g.members.len()));
        }
        let label = self.fit(&label, 12.5, Weight::Medium, limit);
        let name_c = if child { th.sub() } else { th.text_c() };
        self.txt(pm, &label, x, top, 12.5, if child { Weight::Regular } else { Weight::Medium }, name_c);
        // Numbers, with a faint heat tint behind CPU and memory like Task Manager.
        let cores = self.snap.perf.cores.len().max(1) as f32;
        let heat = |th: &Theme, f: f32| load_color(th, f.clamp(0.0, 1.0));
        let cpu_f = cpu / 100.0 / cores;
        if cpu_f > 0.004 {
            let hc = heat(th, cpu_f);
            fill_rect(pm, c[2].x, y, c[2].w, ROW_H, Color::from_rgba(hc.red(), hc.green(), hc.blue(), (cpu_f * 2.5).clamp(0.05, 0.40)).unwrap_or(hc));
        }
        let total = (self.snap.perf.mem.total_kb * 1024).max(1) as f32;
        let mem_f = rss as f32 / total;
        if mem_f > 0.002 {
            let hc = heat(th, 0.3);
            fill_rect(pm, c[3].x, y, c[3].w, ROW_H, Color::from_rgba(hc.red(), hc.green(), hc.blue(), (mem_f * 6.0).clamp(0.04, 0.35)).unwrap_or(hc));
        }
        self.mono(pm, &p.pid.to_string(), c[1].right() - 10.0 - self.measure_mono(&p.pid.to_string(), 12.0), top + 0.5, 12.0, Weight::Regular, th.sub());
        let t = cpu_text(cpu);
        self.mono(pm, &t, c[2].right() - 10.0 - self.measure_mono(&t, 12.0), top + 0.5, 12.0, Weight::Regular, th.text_c());
        let t = human_bytes(rss);
        self.mono(pm, &t, c[3].right() - 10.0 - self.measure_mono(&t, 12.0), top + 0.5, 12.0, Weight::Regular, th.text_c());
        let t = if g.is_some_and(|g| g.members.iter().any(|&m| snap.procs[m].io.is_some())) || p.io.is_some() { rate_text(io) } else { "–".to_string() };
        if !(hover || selected) {
            self.mono(pm, &t, c[4].right() - 10.0 - self.measure_mono(&t, 12.0), top + 0.5, 12.0, Weight::Regular, th.sub());
        } else {
            let (end, force) = lay.row_buttons(y);
            for (r, label, hit, danger) in [(end, "End task", Hit::End(i), false), (force, "Force quit", Hit::Force(i), true)] {
                let hov = self.hover == Some(hit);
                let base = if danger { th.danger.color(if hov { 0.9 } else { 0.55 }) } else { th.ink(if hov { 0.22 } else { 0.13 }) };
                fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, base);
                self.txt_c(pm, label, r.cx(), Self::mid(r.y, r.h, 11.0), 11.0, Weight::Medium, if danger && hov { color(255, 255, 255, 1.0) } else { th.text_c() });
            }
        }
    }

    fn measure_mono(&self, s: &str, px: f32) -> f32 {
        self.fonts.as_ref().map_or(px * 0.6 * s.chars().count() as f32, |f| f.measure_as(Role::Mono, s, px, Weight::Regular))
    }

    fn draw_foot(&mut self, pm: &mut Pixmap, th: &Theme, lay: &Lay) {
        fill_rect(pm, 0.0, lay.foot.y, lay.w, 1.0, th.sep());
        let p = &self.snap.perf;
        let default = format!("{} processes  ·  {} threads  ·  CPU {}  ·  Memory {} of {}", p.processes, p.threads, pct(p.cpu), human_kb(p.mem.used_kb()), human_kb(p.mem.total_kb));
        let (msg, err) = match &self.status {
            Some((m, _, e)) => (m.clone(), *e),
            None => (default, false),
        };
        let avail = lay.btn_end.x - PAD - 12.0;
        let shown = self.fit(&msg, 12.0, Weight::Regular, avail);
        self.txt(pm, &shown, PAD, Self::mid(lay.foot.y, lay.foot.h, 12.0), 12.0, Weight::Regular, if err { th.danger.color(1.0) } else { th.sub() });
        let enabled = self.sel.is_some();
        for (r, label, hit, danger) in [(lay.btn_end, "End task", Hit::FootEnd, false), (lay.btn_force, "Force quit", Hit::FootForce, true)] {
            let hov = enabled && self.hover == Some(hit);
            let a = if enabled { 1.0 } else { 0.4 };
            let base = if danger { th.danger.color(if hov { 0.95 } else { 0.6 * a }) } else { th.ink(if hov { 0.22 } else { 0.12 * a }) };
            fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, base);
            let tc = if enabled { th.text_c() } else { th.faint() };
            self.txt_c(pm, label, r.cx(), Self::mid(r.y, r.h, 12.0), 12.0, Weight::Medium, if danger && hov { color(255, 255, 255, 1.0) } else { tc });
        }
    }

    fn draw_confirm(&mut self, pm: &mut Pixmap, th: &Theme, lay: &Lay) {
        fill_rect(pm, 0.0, 0.0, lay.w, lay.h, color(0, 0, 0, 0.45));
        let Some(c) = self.confirm.clone() else { return };
        let d = lay.dialog;
        crate::gfx::soft_shadow(pm, d.x, d.y, d.w, d.h, 14.0, 18.0, 0.35);
        fill_rounded(pm, d.x, d.y, d.w, d.h, 14.0, th.overlay.color(1.0));
        stroke_rounded(pm, d.x + 0.5, d.y + 0.5, d.w - 1.0, d.h - 1.0, 13.5, th.outline_c(), 1.0);
        let title = self.fit(&c.title, 15.0, Weight::SemiBold, d.w - 2.0 * PAD);
        self.txt(pm, &title, d.x + PAD, d.y + 18.0, 15.0, Weight::SemiBold, th.text_c());
        let mut y = d.y + 48.0;
        for l in c.lines.iter().take(5) {
            let t = self.fit(l, 12.0, Weight::Regular, d.w - 2.0 * PAD);
            self.txt(pm, &t, d.x + PAD, y, 12.0, Weight::Regular, th.sub());
            y += 18.0;
        }
        for (r, label, hit, danger) in [(lay.dlg_no, "Cancel", Hit::ConfirmNo, false), (lay.dlg_yes, c.button, Hit::ConfirmYes, true)] {
            let hov = self.hover == Some(hit);
            let base = if danger { th.danger.color(if hov { 1.0 } else { 0.8 }) } else { th.ink(if hov { 0.22 } else { 0.12 }) };
            fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, base);
            self.txt_c(pm, label, r.cx(), Self::mid(r.y, r.h, 12.5), 12.5, Weight::SemiBold, if danger { color(255, 255, 255, 1.0) } else { th.text_c() });
        }
    }

    // ------------------------------------------------------------ performance

    fn draw_performance(&mut self, pm: &mut Pixmap, th: &Theme, lay: &Lay) {
        let snap = self.snap.clone();
        let p = &snap.perf;
        let items = perf_items(p);
        let sel = self.perf_sel.min(items.len() - 1);
        fill_rect(pm, lay.side.right() - 1.0, lay.side.y, 1.0, lay.side.h, th.sep());
        for (i, kind) in items.iter().enumerate() {
            let r = lay.perf_item(i);
            if i == sel {
                fill_rounded(pm, r.x, r.y, r.w, r.h, 10.0, th.ink(0.12));
            } else if self.hover == Some(Hit::Perf(i)) {
                fill_rounded(pm, r.x, r.y, r.w, r.h, 10.0, th.ink(0.06));
            }
            let (title, sub, hist, max, c) = self.item_summary(th, p, *kind);
            self.txt(pm, &title, r.x + 12.0, r.y + 8.0, 12.5, Weight::SemiBold, th.text_c());
            let sub = self.fit(&sub, 11.0, Weight::Regular, r.w - 90.0);
            self.txt(pm, &sub, r.x + 12.0, r.y + 28.0, 11.0, Weight::Regular, th.sub());
            sparkline(pm, Rect::new(r.right() - 72.0, r.y + 12.0, 60.0, r.h - 24.0), &hist, max, HISTORY, c, false);
        }
        let m = Rect::new(lay.main.x + 20.0, lay.main.y + 12.0, lay.main.w - 40.0, lay.main.h - 24.0);
        match items[sel] {
            PerfKind::Cpu => self.perf_cpu(pm, th, m, p),
            PerfKind::Mem => self.perf_mem(pm, th, m, p),
            PerfKind::Disk => self.perf_disk(pm, th, m, p),
            PerfKind::Net(i) => self.perf_net(pm, th, m, p, i),
            PerfKind::Gpu(i) => self.perf_gpu(pm, th, m, p, i),
            PerfKind::Battery => self.perf_battery(pm, th, m, p),
        }
    }

    fn item_summary(&self, th: &Theme, p: &Perf, kind: PerfKind) -> (String, String, Vec<f32>, f32, Color) {
        let acc = th.accent_c();
        match kind {
            PerfKind::Cpu => ("CPU".into(), format!("{}  {}", pct(p.cpu), p.cpu_mhz.map_or(String::new(), |m| format!("{:.2} GHz", f64::from(m) / 1000.0))), p.cpu_hist.clone(), 1.0, acc),
            PerfKind::Mem => ("Memory".into(), format!("{} / {}", human_kb(p.mem.used_kb()), human_kb(p.mem.total_kb)), p.mem_hist.clone(), 1.0, th.ok.color(1.0)),
            PerfKind::Disk => (
                "Disk".into(),
                format!("R {}  W {}", rate_text(p.disk_read), rate_text(p.disk_write)),
                p.disk_read_hist.iter().zip(&p.disk_write_hist).map(|(r, w)| r + w).collect(),
                auto_max(&p.disk_read_hist, &p.disk_write_hist),
                th.warn.color(1.0),
            ),
            PerfKind::Net(i) => {
                let n = &p.nets[i];
                (n.name.clone(), format!("↓ {}  ↑ {}", rate_text(n.rx), rate_text(n.tx)), n.rx_hist.iter().zip(&n.tx_hist).map(|(r, w)| r + w).collect(), auto_max(&n.rx_hist, &n.tx_hist), acc)
            }
            PerfKind::Gpu(i) => {
                let g = &p.gpus[i];
                (
                    "GPU".into(),
                    g.gpu.util.map_or_else(|| g.gpu.name.clone(), |u| format!("{u:.0}%  {}", g.gpu.temp.map_or(String::new(), |t| format!("{t:.0}°C")))),
                    g.util_hist.clone(),
                    1.0,
                    th.ok.color(1.0),
                )
            }
            PerfKind::Battery => {
                let b = p.battery.as_ref();
                ("Battery".into(), b.map_or(String::new(), |b| format!("{}%  {:.1} W", b.percent, b.watts)), p.power_hist.clone(), p.power_hist.iter().copied().fold(5.0, f32::max), th.warn.color(1.0))
            }
        }
    }

    /// A titled graph: frame, grid, the series (first one filled) and a scale label.
    #[allow(clippy::too_many_arguments)]
    fn graph(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect, title: &str, now: &str, series: &[(&[f32], Color)], max: f32, scale: &str) {
        self.txt(pm, title, r.x, r.y, 13.0, Weight::SemiBold, th.text_c());
        self.txt_r(pm, now, r.right(), r.y, 13.0, Weight::SemiBold, th.sub());
        let plot = Rect::new(r.x, r.y + 24.0, r.w, (r.h - 24.0).max(10.0));
        fill_rounded(pm, plot.x, plot.y, plot.w, plot.h, 8.0, th.ink(0.05));
        for k in 1..4 {
            fill_rect(pm, plot.x + 1.0, (plot.y + plot.h * k as f32 / 4.0).round(), plot.w - 2.0, 1.0, th.ink(0.07));
        }
        for k in 1..6 {
            fill_rect(pm, (plot.x + plot.w * k as f32 / 6.0).round(), plot.y + 1.0, 1.0, plot.h - 2.0, th.ink(0.05));
        }
        let inner = Rect::new(plot.x + 2.0, plot.y + 4.0, plot.w - 4.0, plot.h - 8.0);
        for (i, (vals, c)) in series.iter().enumerate() {
            sparkline(pm, inner, vals, max, HISTORY, *c, i == 0);
        }
        stroke_rounded(pm, plot.x + 0.5, plot.y + 0.5, plot.w - 1.0, plot.h - 1.0, 7.5, th.ink(0.12), 1.0);
        self.txt(pm, scale, plot.x + 8.0, plot.y + 5.0, 10.0, Weight::Regular, th.faint());
        self.txt_r(pm, "60 s", plot.right() - 8.0, plot.bottom() - 15.0, 10.0, Weight::Regular, th.faint());
    }

    /// Rows of `label / value` pairs, four across.
    fn stats(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect, items: &[(&str, String)]) -> f32 {
        let cols = if r.w > 560.0 { 4 } else { 3 };
        let cw = r.w / cols as f32;
        let mut y = r.y;
        for chunk in items.chunks(cols) {
            for (i, (label, value)) in chunk.iter().enumerate() {
                let x = r.x + i as f32 * cw;
                self.txt(pm, label, x, y, 11.0, Weight::Regular, th.sub());
                let v = self.fit(value, 15.0, Weight::SemiBold, cw - 12.0);
                self.mono(pm, &v, x, y + 16.0, 15.0, Weight::SemiBold, th.text_c());
            }
            y += 46.0;
        }
        y
    }

    fn heading(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect, title: &str, sub: &str) {
        self.txt(pm, title, r.x, r.y, 20.0, Weight::Bold, th.text_c());
        let s = self.fit(sub, 12.5, Weight::Regular, r.w * 0.6);
        self.txt_r(pm, &s, r.right(), r.y + 6.0, 12.5, Weight::Regular, th.sub());
    }

    fn perf_cpu(&mut self, pm: &mut Pixmap, th: &Theme, m: Rect, p: &Perf) {
        self.heading(pm, th, m, "CPU", &p.cpu_model);
        let g = Rect::new(m.x, m.y + 36.0, m.w, 150.0);
        self.graph(pm, th, g, "Utilization", &pct(p.cpu), &[(&p.cpu_hist, th.accent_c())], 1.0, "100%");
        let n = p.cores.len();
        let cols = if n > 16 {
            8
        } else if n > 8 {
            6
        } else {
            4
        }
        .min(n.max(1));
        let rows = n.div_ceil(cols).max(1);
        let stats_h = 46.0 * 2.0;
        let grid_top = g.bottom() + 14.0;
        let avail = (m.bottom() - stats_h - grid_top).max(40.0);
        let cell_h = (avail / rows as f32).clamp(34.0, 58.0);
        let cw = m.w / cols as f32;
        for (i, c) in p.cores.iter().enumerate() {
            let (cx, cy) = (m.x + (i % cols) as f32 * cw, grid_top + (i / cols) as f32 * cell_h);
            let cell = Rect::new(cx + 3.0, cy + 2.0, cw - 6.0, cell_h - 4.0);
            fill_rounded(pm, cell.x, cell.y, cell.w, cell.h, 6.0, th.ink(0.05));
            let hist = p.core_hist.get(i).cloned().unwrap_or_default();
            sparkline(pm, Rect::new(cell.x + 4.0, cell.y + 14.0, cell.w - 8.0, cell.h - 18.0), &hist, 1.0, HISTORY, load_color(th, *c), true);
            self.txt(pm, &format!("CPU {i}"), cell.x + 6.0, cell.y + 1.0, 9.5, Weight::Regular, th.faint());
            self.txt_r(pm, &pct(*c), cell.right() - 6.0, cell.y + 1.0, 9.5, Weight::SemiBold, th.sub());
        }
        let y = grid_top + rows as f32 * cell_h + 8.0;
        let items = [
            ("Speed", p.cpu_mhz.map_or("–".into(), |m| format!("{:.2} GHz", f64::from(m) / 1000.0))),
            ("Temperature", p.cpu_temp.map_or("–".into(), |t| format!("{t:.0} °C"))),
            ("Load (1 min)", p.load.map_or("–".into(), |l| format!("{l:.2}"))),
            ("Up time", p.uptime.map_or("–".into(), format_uptime)),
            ("Processes", p.processes.to_string()),
            ("Threads", p.threads.to_string()),
            ("Cores", n.to_string()),
        ];
        self.stats(pm, th, Rect::new(m.x, y, m.w, m.bottom() - y), &items);
    }

    fn perf_mem(&mut self, pm: &mut Pixmap, th: &Theme, m: Rect, p: &Perf) {
        self.heading(pm, th, m, "Memory", &format!("{} total", human_kb(p.mem.total_kb)));
        let g = Rect::new(m.x, m.y + 36.0, m.w, 170.0);
        self.graph(pm, th, g, "Memory in use", &pct(p.mem.used_frac()), &[(&p.mem_hist, th.ok.color(1.0))], 1.0, &human_kb(p.mem.total_kb));
        let mut y = g.bottom() + 14.0;
        if p.mem.swap_total_kb > 0 {
            let g2 = Rect::new(m.x, y, m.w, 110.0);
            self.graph(pm, th, g2, "Swap in use", &pct(p.mem.swap_frac()), &[(&p.swap_hist, th.warn.color(1.0))], 1.0, &human_kb(p.mem.swap_total_kb));
            y = g2.bottom() + 14.0;
        }
        let items = [
            ("In use", human_kb(p.mem.used_kb())),
            ("Available", human_kb(p.mem.available_kb)),
            ("Cached", human_kb(p.cached_kb)),
            ("Total", human_kb(p.mem.total_kb)),
            ("Swap used", human_kb(p.mem.swap_used_kb())),
            ("Swap total", human_kb(p.mem.swap_total_kb)),
        ];
        self.stats(pm, th, Rect::new(m.x, y, m.w, m.bottom() - y), &items);
    }

    fn perf_disk(&mut self, pm: &mut Pixmap, th: &Theme, m: Rect, p: &Perf) {
        self.heading(pm, th, m, "Disk", "all physical disks");
        let max = auto_max(&p.disk_read_hist, &p.disk_write_hist);
        let g1 = Rect::new(m.x, m.y + 36.0, m.w, 130.0);
        self.graph(pm, th, g1, "Read", &rate_text(p.disk_read), &[(&p.disk_read_hist, th.accent_c())], max, &human_rate(f64::from(max)));
        let g2 = Rect::new(m.x, g1.bottom() + 14.0, m.w, 130.0);
        self.graph(pm, th, g2, "Write", &rate_text(p.disk_write), &[(&p.disk_write_hist, th.warn.color(1.0))], max, &human_rate(f64::from(max)));
        let items = [("Read", rate_text(p.disk_read)), ("Write", rate_text(p.disk_write))];
        self.stats(pm, th, Rect::new(m.x, g2.bottom() + 14.0, m.w, 60.0), &items);
    }

    fn perf_net(&mut self, pm: &mut Pixmap, th: &Theme, m: Rect, p: &Perf, i: usize) {
        let Some(n) = p.nets.get(i) else { return };
        self.heading(pm, th, m, &n.name, "network interface");
        let max = auto_max(&n.rx_hist, &n.tx_hist);
        let g1 = Rect::new(m.x, m.y + 36.0, m.w, 130.0);
        self.graph(pm, th, g1, "Receive", &rate_text(n.rx), &[(&n.rx_hist, th.accent_c())], max, &human_rate(f64::from(max)));
        let g2 = Rect::new(m.x, g1.bottom() + 14.0, m.w, 130.0);
        self.graph(pm, th, g2, "Send", &rate_text(n.tx), &[(&n.tx_hist, th.ok.color(1.0))], max, &human_rate(f64::from(max)));
        let items = [("Receiving", rate_text(n.rx)), ("Sending", rate_text(n.tx)), ("Received in total", human_bytes(n.rx_total)), ("Sent in total", human_bytes(n.tx_total))];
        self.stats(pm, th, Rect::new(m.x, g2.bottom() + 14.0, m.w, 60.0), &items);
    }

    fn perf_gpu(&mut self, pm: &mut Pixmap, th: &Theme, m: Rect, p: &Perf, i: usize) {
        let Some(g) = p.gpus.get(i) else { return };
        self.heading(pm, th, m, "GPU", &g.gpu.name);
        let g1 = Rect::new(m.x, m.y + 36.0, m.w, 160.0);
        let now = g.gpu.util.map_or("–".into(), |u| format!("{u:.0}%"));
        self.graph(pm, th, g1, "Utilization", &now, &[(&g.util_hist, th.ok.color(1.0))], 1.0, "100%");
        let mut y = g1.bottom() + 14.0;
        if g.gpu.mem_total.is_some() {
            let g2 = Rect::new(m.x, y, m.w, 120.0);
            let now = g.gpu.mem_used.map_or("–".into(), human_bytes);
            self.graph(pm, th, g2, "Video memory", &now, &[(&g.mem_hist, th.accent_c())], 1.0, &g.gpu.mem_total.map_or(String::new(), human_bytes));
            y = g2.bottom() + 14.0;
        }
        let items = [
            ("Utilization", g.gpu.util.map_or("n/a".into(), |u| format!("{u:.0}%"))),
            ("Temperature", g.gpu.temp.map_or("n/a".into(), |t| format!("{t:.0} °C"))),
            ("Power", g.gpu.power_w.map_or("n/a".into(), |w| format!("{w:.0} W"))),
            ("Clock", g.gpu.freq_mhz.map_or("n/a".into(), |f| format!("{f} MHz"))),
            ("Memory used", g.gpu.mem_used.map_or("n/a".into(), human_bytes)),
            ("Memory total", g.gpu.mem_total.map_or("n/a".into(), human_bytes)),
        ];
        self.stats(pm, th, Rect::new(m.x, y, m.w, m.bottom() - y), &items);
    }

    fn perf_battery(&mut self, pm: &mut Pixmap, th: &Theme, m: Rect, p: &Perf) {
        let Some(b) = &p.battery else { return };
        self.heading(pm, th, m, "Battery", &b.status);
        let max = p.power_hist.iter().copied().fold(5.0, f32::max) * 1.1;
        let g1 = Rect::new(m.x, m.y + 36.0, m.w, 190.0);
        self.graph(pm, th, g1, "Power draw", &format!("{:.1} W", b.watts), &[(&p.power_hist, th.warn.color(1.0))], max, &format!("{max:.0} W"));
        let items = [("Charge", format!("{}%", b.percent)), ("Status", if b.status.is_empty() { "–".into() } else { b.status.clone() }), ("Power", format!("{:.1} W", b.watts))];
        self.stats(pm, th, Rect::new(m.x, g1.bottom() + 14.0, m.w, 60.0), &items);
        let _ = (pm, th);
    }
}

/// A scale for two rate series: the biggest value with some headroom, at least 100 kB/s so an idle graph stays flat.
pub fn auto_max(a: &[f32], b: &[f32]) -> f32 {
    a.iter().chain(b).copied().fold(0.0f32, f32::max).max(100_000.0) * 1.15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fits_the_popup_and_a_small_window() {
        for (w, h) in [(780.0, 560.0), (960.0, 660.0), (520.0, 360.0)] {
            let l = Lay::new(w, h, true);
            assert!(l.list.h > 100.0, "{w}x{h}: list {}", l.list.h);
            assert!(l.cols[0].w > 100.0, "{w}: name column {}", l.cols[0].w);
            assert!(l.search.right() <= l.mine.x, "{w}: the search box overlaps the chips");
            assert!(l.cols.windows(2).all(|c| c[0].right() <= c[1].x + 0.01), "columns do not overlap");
            assert!(l.cols[4].right() <= w);
            assert!(l.btn_end.right() < l.btn_force.x);
        }
    }

    fn state(rows: usize) -> HitState {
        HitState { tab: Tab::Processes, rows, scroll: 0.0, has_sel: false, confirm: false, perf_items: 3 }
    }

    #[test]
    fn hits() {
        let l = Lay::new(780.0, 560.0, true);
        assert_eq!(l.hit(l.tabs[1].cx(), l.tabs[1].cy(), &state(5)), Some(Hit::Tab(Tab::Performance)));
        assert_eq!(l.hit(l.link.cx(), l.link.cy(), &state(5)), Some(Hit::Link));
        assert_eq!(Lay::new(780.0, 560.0, false).hit(l.link.cx(), l.link.cy(), &state(5)), None, "no link in the window");
        assert_eq!(l.hit(l.cols[2].cx(), l.head.cy(), &state(5)), Some(Hit::Sort(SortKey::Cpu)));
        assert_eq!(l.hit(l.search.cx(), l.search.cy(), &state(5)), Some(Hit::Search));
        // rows: third row, name area; its arrow; its inline buttons
        let y = l.list.y + 2.0 * ROW_H + 5.0;
        assert_eq!(l.hit(200.0, y, &state(5)), Some(Hit::Row(2, false)));
        assert_eq!(l.hit(20.0, y, &state(5)), Some(Hit::Row(2, true)));
        let (end, force) = l.row_buttons(l.list.y + 2.0 * ROW_H);
        assert_eq!(l.hit(end.cx(), end.cy(), &state(5)), Some(Hit::End(2)));
        assert_eq!(l.hit(force.cx(), force.cy(), &state(5)), Some(Hit::Force(2)));
        assert_eq!(l.hit(200.0, l.list.y + 9.0 * ROW_H, &state(5)), None, "below the last row");
        // scrolled by one and a half rows
        let s = HitState { scroll: 42.0, ..state(5) };
        assert_eq!(l.hit(200.0, l.list.y + 1.0, &s), Some(Hit::Row(1, false)));
        // footer buttons need a selection
        assert_eq!(l.hit(l.btn_end.cx(), l.btn_end.cy(), &state(5)), None);
        assert_eq!(l.hit(l.btn_end.cx(), l.btn_end.cy(), &HitState { has_sel: true, ..state(5) }), Some(Hit::FootEnd));
        // a dialog blocks everything else
        let d = HitState { confirm: true, ..state(5) };
        assert_eq!(l.hit(l.tabs[1].cx(), l.tabs[1].cy(), &d), None);
        assert_eq!(l.hit(l.dlg_yes.cx(), l.dlg_yes.cy(), &d), Some(Hit::ConfirmYes));
        assert_eq!(l.hit(l.dlg_no.cx(), l.dlg_no.cy(), &d), Some(Hit::ConfirmNo));
        // performance tab
        let p = HitState { tab: Tab::Performance, ..state(5) };
        assert_eq!(l.hit(l.perf_item(1).cx(), l.perf_item(1).cy(), &p), Some(Hit::Perf(1)));
        assert_eq!(l.hit(l.perf_item(3).cx(), l.perf_item(3).cy(), &p), None, "only three items");
    }

    #[test]
    fn texts() {
        assert_eq!(cpu_text(0.0), "0%");
        assert_eq!(cpu_text(3.2), "3.2%");
        assert_eq!(cpu_text(250.0), "250%");
        assert_eq!(rate_text(0.2), "0 B/s");
        assert!(auto_max(&[1.0], &[2.0]) >= 100_000.0);
        let perf = Perf::default();
        assert_eq!(perf_items(&perf), vec![PerfKind::Cpu, PerfKind::Mem, PerfKind::Disk]);
    }
}
