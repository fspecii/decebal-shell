//! The Windows-style taskbar of decdock: pure model and layout math (no windowing, no drawing; see
//! [`draw`] for that). Coordinates are in the strip's own space, x from the left edge of the monitor and
//! y from the strip's top; the host shifts them to wherever the strip sits in its surface.
//!
//! Left to right: Start, search, task buttons (pinned and running apps; centred or left aligned) ...
//! system tray, clock, show-desktop sliver. When there are too many buttons they shrink, then scroll.

pub mod draw;

use crate::config::{WinAlign, WinLabels, WinSearch};
use crate::menubar::Tray;
use crate::wm::{app_matches, Window};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

/// Sizes of the taskbar parts at one scale. The two looks differ in a few numbers (Windows 11 style
/// centred and rounded, Windows 10 style left aligned and square), kept together here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub scale: f32,
    /// Windows 10 look: square buttons, a line under every running app, wider search box.
    pub classic: bool,
    pub bar_h: f32,
    pub btn_w: f32,
    pub btn_h: f32,
    pub gap: f32,
    pub icon: f32,
    pub pad: f32,
    pub start_w: f32,
    pub search_box_w: f32,
    pub search_btn_w: f32,
    pub tray_w: f32,
    pub clock_pad: f32,
    pub desktop_w: f32,
    /// Widest and narrowest a labelled button gets before the row scrolls.
    pub label_max: f32,
    pub label_min: f32,
    /// Narrowest an icon-only button shrinks to before the row scrolls.
    pub icon_min: f32,
    /// Corner radius of a button's plate.
    pub corner: f32,
}

impl Metrics {
    /// `scale` is the DPI factor (1.0 = 96 dpi); non-finite or absurd values are clamped.
    pub fn new(scale: f32, align: WinAlign) -> Metrics {
        let s = if scale.is_finite() { scale.clamp(0.75, 4.0) } else { 1.0 };
        let classic = align == WinAlign::Left;
        let r = |v: f32| (v * s).round();
        Metrics {
            scale: s,
            classic,
            bar_h: r(if classic { 44.0 } else { 48.0 }),
            btn_w: r(if classic { 48.0 } else { 44.0 }),
            btn_h: r(if classic { 44.0 } else { 40.0 }),
            gap: r(if classic { 0.0 } else { 4.0 }),
            icon: r(24.0),
            pad: r(12.0),
            start_w: r(if classic { 48.0 } else { 44.0 }),
            search_box_w: r(if classic { 236.0 } else { 184.0 }),
            search_btn_w: r(if classic { 48.0 } else { 44.0 }),
            tray_w: r(30.0),
            clock_pad: r(9.0),
            desktop_w: r(if classic { 8.0 } else { 6.0 }),
            label_max: r(172.0),
            label_min: r(64.0),
            icon_min: r(32.0),
            corner: 4.0 * s,
        }
    }

    /// Takes the plate corner from the theme's radius (8 gives the Windows 11 default of 4).
    pub fn themed(mut self, t: &crate::theme::Theme) -> Metrics {
        self.corner = (t.radius * 0.5).clamp(2.0, 8.0) * self.scale;
        self
    }
}

// ------------------------------------------------------------------ buttons

/// One task button: a pinned launcher, a running app, or (with titles) one window.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskButton {
    /// Desktop id: names the icon and the pinned entry.
    pub app_id: String,
    /// The window it stands for (titled mode), the one to focus (combined mode), or none when not running.
    pub window: Option<u64>,
    pub title: String,
    pub pinned: bool,
    pub running: bool,
    /// Its window (or one of the app's windows) has the focus.
    pub active: bool,
    /// Windows of the app (combined mode) or 1.
    pub windows: usize,
}

/// Builds the buttons: pinned apps in their order (Launchpad and Trash are the dock's, not the taskbar's),
/// then the running apps that are not pinned in window-list order. `resolve` maps a window's app id to
/// a desktop id (None = unknown, which falls back to the lower-cased app id so every window shows).
pub fn build_buttons(pinned: &[String], windows: &[Window], labels: WinLabels, resolve: &mut dyn FnMut(&str) -> Option<String>) -> Vec<TaskButton> {
    // (item id, pinned) in display order, and the windows of each.
    let mut groups: Vec<(String, bool, Vec<&Window>)> = pinned.iter().filter(|id| !matches!(id.as_str(), "launchpad" | "trash")).map(|id| (id.clone(), true, Vec::new())).collect();
    for w in windows {
        if w.app_id.is_empty() {
            continue;
        }
        let at = match groups.iter().position(|(id, _, _)| app_matches(id, &w.app_id)) {
            Some(i) => i,
            None => {
                let id = resolve(&w.app_id).unwrap_or_else(|| w.app_id.to_lowercase());
                match groups.iter().position(|(g, _, _)| *g == id || app_matches(g, &id)) {
                    Some(i) => i,
                    None => {
                        groups.push((id, false, Vec::new()));
                        groups.len() - 1
                    }
                }
            }
        };
        groups[at].2.push(w);
    }
    let mut out = Vec::new();
    for (id, pin, ws) in groups {
        if ws.is_empty() {
            out.push(TaskButton { app_id: id, window: None, title: String::new(), pinned: pin, running: false, active: false, windows: 0 });
        } else if labels == WinLabels::Always {
            for (i, w) in ws.iter().enumerate() {
                out.push(TaskButton { app_id: id.clone(), window: Some(w.id), title: w.title.clone(), pinned: pin && i == 0, running: true, active: w.active, windows: 1 });
            }
        } else {
            let lead = ws.iter().find(|w| w.active).unwrap_or(&ws[0]);
            out.push(TaskButton { app_id: id, window: Some(lead.id), title: lead.title.clone(), pinned: pin, running: true, active: ws.iter().any(|w| w.active), windows: ws.len() });
        }
    }
    out
}

/// What a left click on a task button does, given the (id, active) of the app's windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClick {
    /// Not running: start it.
    Launch,
    /// Bring the window up (restoring it when minimised).
    Activate(u64),
    /// The focused window's button was clicked again: minimise it.
    Minimize(u64),
}

pub fn click_action(windows: &[(u64, bool)]) -> TaskClick {
    let Some(first) = windows.first() else { return TaskClick::Launch };
    match windows.iter().position(|w| w.1) {
        Some(_) if windows.len() == 1 => TaskClick::Minimize(first.0),
        // Several windows of one app: clicking the button walks through them.
        Some(i) => TaskClick::Activate(windows[(i + 1) % windows.len()].0),
        None => TaskClick::Activate(first.0),
    }
}

// ------------------------------------------------------------------- layout

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Start,
    Search,
    Task(usize),
    Tray(Tray),
    Clock,
    Desktop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub start: Rect,
    pub search: Option<Rect>,
    /// Parallel to the buttons; already shifted by the scroll offset, so some may lie outside `clip`.
    pub tasks: Vec<Rect>,
    /// Visible span of the task row: (left, right).
    pub clip: (f32, f32),
    /// How far the row can scroll (0 when everything fits).
    pub scroll_max: f32,
    /// Width of one task button after shrinking.
    pub task_w: f32,
    pub tray: Vec<(Tray, Rect)>,
    pub clock: Rect,
    pub desktop: Rect,
}

pub struct Inputs<'a> {
    pub width: f32,
    pub align: WinAlign,
    pub labels: WinLabels,
    pub search: WinSearch,
    pub buttons: usize,
    pub tray: &'a [Tray],
    /// Width of the widest clock line.
    pub clock_text_w: f32,
    pub scroll: f32,
}

pub fn layout(m: &Metrics, i: &Inputs) -> Layout {
    let h = m.bar_h;
    // Right edge: show-desktop sliver, clock, tray icons.
    let desktop = Rect { x: i.width - m.desktop_w, y: 0.0, w: m.desktop_w, h };
    let clock_w = (i.clock_text_w + 2.0 * m.clock_pad).ceil();
    let clock = Rect { x: desktop.x - clock_w, y: 0.0, w: clock_w, h };
    let mut tx = clock.x - m.gap.max(4.0 * m.scale);
    let mut tray: Vec<(Tray, Rect)> = i
        .tray
        .iter()
        .rev()
        .map(|&k| {
            tx -= m.tray_w;
            (k, Rect { x: tx, y: 0.0, w: m.tray_w, h })
        })
        .collect();
    tray.reverse();
    let tray_left = if tray.is_empty() { clock.x } else { tx };

    let region_l = m.pad;
    let region_r = (tray_left - m.pad * 0.75).max(region_l);
    let search_w = match i.search {
        WinSearch::None => 0.0,
        WinSearch::Button => m.search_btn_w,
        WinSearch::Box => m.search_box_w,
    };
    let spacing = |w: f32| if w > 0.0 { m.gap } else { 0.0 };
    let fixed = m.start_w + spacing(search_w) + search_w + if i.buttons > 0 { m.gap } else { 0.0 };
    let n = i.buttons as f32;
    let (max_w, min_w) = if i.labels == WinLabels::Always { (m.label_max, m.label_min) } else { (m.btn_w, m.icon_min) };
    let avail = (region_r - region_l - fixed).max(0.0);
    let task_w = if i.buttons == 0 { max_w } else { ((avail - m.gap * (n - 1.0)) / n).clamp(min_w, max_w).floor() };
    let tasks_total = if i.buttons == 0 { 0.0 } else { n * task_w + (n - 1.0) * m.gap };
    let overflow = tasks_total > avail + 0.5;
    let group_w = fixed + if overflow { avail } else { tasks_total };
    let x0 = match i.align {
        WinAlign::Left => region_l,
        WinAlign::Center => ((i.width - group_w) / 2.0).round().clamp(region_l, (region_r - group_w).max(region_l)),
    };
    let start = Rect { x: x0, y: 0.0, w: m.start_w, h };
    let mut x = start.x + start.w;
    let search = (search_w > 0.0).then(|| {
        x += m.gap;
        let r = Rect { x, y: 0.0, w: search_w, h };
        x += search_w;
        r
    });
    if i.buttons > 0 {
        x += m.gap;
    }
    let area_l = x;
    let area_w = if overflow { avail } else { tasks_total };
    let scroll_max = if overflow { (tasks_total - area_w).max(0.0) } else { 0.0 };
    let scroll = i.scroll.clamp(0.0, scroll_max);
    let tasks = (0..i.buttons).map(|k| Rect { x: area_l + k as f32 * (task_w + m.gap) - scroll, y: 0.0, w: task_w, h }).collect();
    Layout { width: i.width, start, search, tasks, clip: (area_l, area_l + area_w), scroll_max, task_w, tray, clock, desktop }
}

impl Layout {
    pub fn hit(&self, x: f32, y: f32, bar_h: f32) -> Option<Hit> {
        if y < 0.0 || y >= bar_h || x < 0.0 || x >= self.width {
            return None;
        }
        if self.desktop.contains(x, y) {
            return Some(Hit::Desktop);
        }
        if self.clock.contains(x, y) {
            return Some(Hit::Clock);
        }
        if let Some((k, _)) = self.tray.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(Hit::Tray(*k));
        }
        if self.start.contains(x, y) {
            return Some(Hit::Start);
        }
        if self.search.is_some_and(|r| r.contains(x, y)) {
            return Some(Hit::Search);
        }
        if x >= self.clip.0 && x < self.clip.1 {
            return self.tasks.iter().position(|r| r.contains(x, y)).map(Hit::Task);
        }
        None
    }

    pub fn rect_of(&self, hit: Hit) -> Option<Rect> {
        match hit {
            Hit::Start => Some(self.start),
            Hit::Search => self.search,
            Hit::Task(i) => self.tasks.get(i).copied(),
            Hit::Tray(k) => self.tray.iter().find(|(t, _)| *t == k).map(|(_, r)| *r),
            Hit::Clock => Some(self.clock),
            Hit::Desktop => Some(self.desktop),
        }
    }
}

/// Where the pinned entry `from` should move to when dragged over the task button at `over` (indices
/// into `pinned`); `None` when they are the same app or either is not a movable pinned entry.
pub fn reorder_target(pinned: &[String], dragged: &str, over: &str) -> Option<(usize, usize)> {
    if dragged == over || matches!(over, "launchpad" | "trash") {
        return None;
    }
    let from = pinned.iter().position(|p| p == dragged)?;
    let to = pinned.iter().position(|p| p == over)?;
    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Metrics {
        Metrics::new(1.0, WinAlign::Center)
    }

    fn inp(width: f32, align: WinAlign, buttons: usize) -> Inputs<'static> {
        Inputs { width, align, labels: WinLabels::Never, search: WinSearch::Button, buttons, tray: &[Tray::Network, Tray::Audio, Tray::Battery], clock_text_w: 60.0, scroll: 0.0 }
    }

    fn win(id: u64, app: &str, title: &str, active: bool) -> Window {
        Window { id, app_id: app.into(), title: title.into(), active, desktop: None }
    }

    #[test]
    fn metrics_scale_with_dpi_and_are_clamped() {
        assert_eq!(Metrics::new(1.0, WinAlign::Center).bar_h, 48.0);
        assert_eq!(Metrics::new(2.0, WinAlign::Center).bar_h, 96.0);
        assert_eq!(Metrics::new(1.5, WinAlign::Left).bar_h, 66.0);
        assert!(Metrics::new(1.0, WinAlign::Left).classic && !m().classic);
        assert_eq!(Metrics::new(f32::NAN, WinAlign::Center).scale, 1.0);
        assert_eq!(Metrics::new(50.0, WinAlign::Center).scale, 4.0);
    }

    #[test]
    fn centred_group_sits_in_the_middle_of_the_bar() {
        let l = layout(&m(), &inp(1600.0, WinAlign::Center, 5));
        let last = l.tasks.last().unwrap();
        let (left, right) = (l.start.x, last.x + last.w);
        assert!(((left + right) / 2.0 - 800.0).abs() <= 1.0, "{left} {right}");
        assert_eq!(l.tasks.len(), 5);
        assert!(l.tasks[0].x > l.search.unwrap().x + l.search.unwrap().w);
        assert_eq!(l.scroll_max, 0.0);
    }

    #[test]
    fn left_alignment_starts_at_the_left_pad() {
        let l = layout(&Metrics::new(1.0, WinAlign::Left), &inp(1600.0, WinAlign::Left, 5));
        assert_eq!(l.start.x, 12.0);
        assert_eq!(l.tasks[1].x, l.tasks[0].x + 48.0, "classic buttons touch");
    }

    #[test]
    fn tray_clock_and_sliver_are_right_aligned_in_order() {
        let l = layout(&m(), &inp(1600.0, WinAlign::Center, 3));
        assert_eq!(l.desktop.x + l.desktop.w, 1600.0);
        assert_eq!(l.clock.x + l.clock.w, l.desktop.x);
        assert!(l.tray.iter().all(|(_, r)| r.x + r.w <= l.clock.x));
        let kinds: Vec<Tray> = l.tray.iter().map(|t| t.0).collect();
        assert_eq!(kinds, vec![Tray::Network, Tray::Audio, Tray::Battery]);
        assert!(l.tray[0].1.x < l.tray[1].1.x && l.tray[1].1.x < l.tray[2].1.x);
        assert_eq!(l.clock.w, (60.0f32 + 18.0).ceil());
    }

    #[test]
    fn centred_group_is_pushed_off_the_tray_on_a_narrow_bar() {
        let l = layout(&m(), &inp(700.0, WinAlign::Center, 12));
        let tray_left = l.tray[0].1.x;
        assert!(l.clip.1 <= tray_left, "{} vs {tray_left}", l.clip.1);
        assert!(l.start.x >= 12.0);
        assert!(l.scroll_max > 0.0, "twelve buttons do not fit in 700px");
    }

    #[test]
    fn many_buttons_shrink_then_scroll() {
        let mut i = inp(700.0, WinAlign::Center, 10);
        let normal = layout(&m(), &Inputs { buttons: 4, ..inp(700.0, WinAlign::Center, 4) });
        let shrunk = layout(&m(), &i);
        assert!(shrunk.task_w < normal.task_w && shrunk.task_w >= m().icon_min, "{}", shrunk.task_w);
        assert_eq!(shrunk.scroll_max, 0.0, "shrinking alone is enough");
        i.buttons = 60;
        let l = layout(&m(), &i);
        assert_eq!(l.task_w, m().icon_min);
        assert!(l.scroll_max > 0.0);
        // scrolled to the end: the last button ends exactly at the clip's right edge
        i.scroll = 1.0e6;
        let end = layout(&m(), &i);
        let last = end.tasks.last().unwrap();
        assert!((last.x + last.w - end.clip.1).abs() < 0.5, "{} {}", last.x + last.w, end.clip.1);
        i.scroll = -5.0;
        assert_eq!(layout(&m(), &i).tasks[0].x, end.clip.0);
    }

    #[test]
    fn labelled_buttons_are_wide_and_shrink_to_a_minimum() {
        let mk = |n| layout(&m(), &Inputs { labels: WinLabels::Always, ..inp(1600.0, WinAlign::Left, n) });
        assert_eq!(mk(2).task_w, m().label_max);
        let tight = mk(14);
        assert!(tight.task_w < m().label_max && tight.task_w >= m().label_min);
        let l = mk(40);
        assert_eq!(l.task_w, m().label_min);
        assert!(l.scroll_max > 0.0);
        // the buttons never run into the tray
        assert!(l.clip.1 <= l.tray[0].1.x);
    }

    #[test]
    fn hit_testing_covers_every_zone_and_respects_the_clip() {
        let mut i = inp(1000.0, WinAlign::Left, 40);
        i.search = WinSearch::Box;
        let mm = Metrics::new(1.0, WinAlign::Left);
        let l = layout(&mm, &i);
        let y = 20.0;
        assert_eq!(l.hit(l.start.cx(), y, mm.bar_h), Some(Hit::Start));
        assert_eq!(l.hit(l.search.unwrap().cx(), y, mm.bar_h), Some(Hit::Search));
        assert_eq!(l.hit(l.tasks[0].cx(), y, mm.bar_h), Some(Hit::Task(0)));
        assert_eq!(l.hit(l.tray[1].1.cx(), y, mm.bar_h), Some(Hit::Tray(Tray::Audio)));
        assert_eq!(l.hit(l.clock.cx(), y, mm.bar_h), Some(Hit::Clock));
        assert_eq!(l.hit(999.0, y, mm.bar_h), Some(Hit::Desktop));
        // a scrolled-out button is not clickable, and neither is the area below or above the strip
        assert_eq!(l.hit(l.tasks[39].cx(), y, mm.bar_h), None);
        assert_eq!(l.hit(l.start.cx(), mm.bar_h, mm.bar_h), None);
        assert_eq!(l.hit(l.start.cx(), -1.0, mm.bar_h), None);
        assert_eq!(l.hit(-3.0, y, mm.bar_h), None);
        assert_eq!(l.rect_of(Hit::Search), l.search);
        assert_eq!(l.rect_of(Hit::Tray(Tray::Layout)), None);
    }

    #[test]
    fn empty_taskbar_and_no_tray_are_safe() {
        let l = layout(&m(), &Inputs { tray: &[], search: WinSearch::None, ..inp(800.0, WinAlign::Center, 0) });
        assert!(l.tasks.is_empty() && l.search.is_none() && l.tray.is_empty());
        assert!(l.start.x > 0.0 && l.start.x + l.start.w < l.clock.x);
        let tiny = layout(&m(), &inp(120.0, WinAlign::Center, 3));
        assert!(tiny.tasks.iter().all(|r| r.w > 0.0));
    }

    #[test]
    fn buttons_follow_pinned_order_then_running_extras() {
        let pinned: Vec<String> = ["launchpad", "foot", "firefox", "trash"].iter().map(|s| s.to_string()).collect();
        let windows = vec![win(1, "kitty", "k", false), win(2, "foot", "shell", true), win(3, "Kitty", "k2", false)];
        let mut res = |a: &str| Some(a.to_lowercase());
        let b = build_buttons(&pinned, &windows, WinLabels::Never, &mut res);
        let ids: Vec<&str> = b.iter().map(|b| b.app_id.as_str()).collect();
        assert_eq!(ids, vec!["foot", "firefox", "kitty"], "launchpad and trash are the dock's");
        assert!(b[0].running && b[0].active && b[0].pinned);
        assert!(!b[1].running && b[1].window.is_none());
        assert!(b[2].running && !b[2].pinned && b[2].windows == 2);
    }

    #[test]
    fn titled_mode_gives_each_window_a_button() {
        let pinned = vec!["foot".to_string(), "firefox".to_string()];
        let windows = vec![win(1, "foot", "one", false), win(2, "foot", "two", true), win(3, "mpv", "movie", false)];
        let mut res = |a: &str| Some(a.to_lowercase());
        let b = build_buttons(&pinned, &windows, WinLabels::Always, &mut res);
        assert_eq!(
            b.iter().map(|b| (b.app_id.as_str(), b.title.as_str(), b.active)).collect::<Vec<_>>(),
            vec![("foot", "one", false), ("foot", "two", true), ("firefox", "", false), ("mpv", "movie", false)]
        );
        assert_eq!(b.iter().filter(|b| b.pinned).count(), 2, "an app's pin shows once");
    }

    #[test]
    fn combined_button_leads_with_the_focused_window() {
        let pinned = vec!["foot".to_string()];
        let windows = vec![win(1, "foot", "one", false), win(2, "foot", "two", true)];
        let b = build_buttons(&pinned, &windows, WinLabels::Never, &mut |_| None);
        assert_eq!((b[0].window, b[0].windows, b[0].active), (Some(2), 2, true));
    }

    #[test]
    fn windows_of_unknown_apps_still_get_a_button() {
        let b = build_buttons(&[], &[win(7, "Weird.App", "x", false), win(0, "", "no class", false)], WinLabels::Never, &mut |_| None);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].app_id, "weird.app");
    }

    #[test]
    fn clicking_follows_windows_rules() {
        assert_eq!(click_action(&[]), TaskClick::Launch);
        assert_eq!(click_action(&[(5, false)]), TaskClick::Activate(5));
        assert_eq!(click_action(&[(5, true)]), TaskClick::Minimize(5), "the focused app's button minimises");
        assert_eq!(click_action(&[(5, false), (6, true)]), TaskClick::Activate(5), "several windows are cycled");
        assert_eq!(click_action(&[(5, true), (6, false)]), TaskClick::Activate(6));
        assert_eq!(click_action(&[(5, false), (6, false)]), TaskClick::Activate(5));
    }

    #[test]
    fn reorder_targets() {
        let p: Vec<String> = ["launchpad", "a", "b", "c", "trash"].iter().map(|s| s.to_string()).collect();
        assert_eq!(reorder_target(&p, "a", "c"), Some((1, 3)));
        assert_eq!(reorder_target(&p, "c", "a"), Some((3, 1)));
        assert_eq!(reorder_target(&p, "a", "a"), None);
        assert_eq!(reorder_target(&p, "a", "trash"), None);
        assert_eq!(reorder_target(&p, "x", "a"), None, "unpinned entries cannot be moved");
    }
}
