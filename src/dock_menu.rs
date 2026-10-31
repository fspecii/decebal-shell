//! The dock's right-click context menu: a pure model (entries, hover, keyboard, two-step confirm)
//! plus drawing in the same macOS style as the menu bar's dropdowns.
//!
//! Row geometry and hit-testing come from [`crate::menubar::menus::Dropdown`], so the dock menu and
//! the bar menus lay out identically; [`draw_dropdown`] is the shared painter (the bar can adopt it).

use crate::config::{DockStyle, WinAlign, WinLabels};
use crate::gfx::{color, fill_rect, fill_rounded, stroke_rounded};
use crate::menubar::menus::{Dropdown, Item, Menu, PAD};
use crate::text::{Fonts, Weight};
use tiny_skia::Pixmap;

pub const TEXT_PX: f32 = 12.0;
/// Longest label before it is cut with an ellipsis (window titles can be arbitrarily long).
const MAX_LABEL_W: f32 = 260.0;
/// At most this many windows are listed.
const MAX_WINDOWS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Focus (and restore) one window.
    Focus(u64),
    /// Launch the app (also "Open" for an app that is not running).
    Open,
    KeepInDock,
    RemoveFromDock,
    ShowInFinder,
    /// Minimise every window of the app.
    Hide,
    /// Ask every window of the app to close.
    Quit,
    EmptyTrash,
    /// Close one window (a taskbar button of a single window).
    CloseWindow(u64),
    /// Switch the dock between its macOS and Windows looks.
    SetStyle(DockStyle),
    SetAlign(WinAlign),
    SetLabels(WinLabels),
    /// Open the Desktop & Dock pane of the settings app.
    OpenSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub label: String,
    /// `None` for a separator.
    pub action: Option<Action>,
    pub enabled: bool,
    /// Needs a second activation ("Empty Trash?").
    pub confirm: bool,
}

impl Entry {
    fn new(label: &str, action: Action) -> Entry {
        Entry { label: label.to_string(), action: Some(action), enabled: true, confirm: false }
    }

    fn sep() -> Entry {
        Entry { label: String::new(), action: None, enabled: false, confirm: false }
    }

    fn is_sep(&self) -> bool {
        self.action.is_none()
    }
}

/// What the menu needs to know about the item it was opened for.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    pub id: String,
    pub display_name: String,
    pub running: bool,
    pub pinned: bool,
    /// (window id, title) of the app's windows.
    pub windows: Vec<(u64, String)>,
    /// A `.desktop` file exists, so "Show in Finder" and "Keep in Dock" make sense.
    pub has_desktop: bool,
}

/// Entries for a dock item, in macOS order: windows, new window, options, hide / quit.
pub fn build_entries(ctx: &Ctx) -> Vec<Entry> {
    let mut out = Vec::new();
    match ctx.id.as_str() {
        "launchpad" => out.push(Entry::new("Open", Action::Open)),
        "trash" => {
            out.push(Entry::new("Open", Action::Open));
            out.push(Entry::sep());
            out.push(Entry { confirm: true, ..Entry::new("Empty Trash…", Action::EmptyTrash) });
        }
        _ => {
            if ctx.running {
                for (id, title) in ctx.windows.iter().take(MAX_WINDOWS) {
                    let label = if title.trim().is_empty() { ctx.display_name.clone() } else { title.trim().to_string() };
                    out.push(Entry::new(&label, Action::Focus(*id)));
                }
                if !ctx.windows.is_empty() {
                    out.push(Entry::sep());
                }
                out.push(Entry::new("New Window", Action::Open));
            } else {
                out.push(Entry::new("Open", Action::Open));
            }
            out.push(Entry::sep());
            out.push(if ctx.pinned { Entry::new("Remove from Dock", Action::RemoveFromDock) } else { Entry::new("Keep in Dock", Action::KeepInDock) });
            if ctx.has_desktop {
                out.push(Entry::new("Show in Finder", Action::ShowInFinder));
            }
            if ctx.running {
                out.push(Entry::sep());
                out.push(Entry::new("Hide", Action::Hide));
                out.push(Entry::new("Quit", Action::Quit));
            }
        }
    }
    out
}

/// Entries for a taskbar button (Windows wording): windows, the app, pin/unpin, minimise, close.
pub fn build_entries_windows(ctx: &Ctx) -> Vec<Entry> {
    let mut out = Vec::new();
    let name = if ctx.display_name.is_empty() { ctx.id.clone() } else { ctx.display_name.clone() };
    if ctx.running && ctx.windows.len() > 1 {
        for (id, title) in ctx.windows.iter().take(MAX_WINDOWS) {
            let label = if title.trim().is_empty() { name.clone() } else { title.trim().to_string() };
            out.push(Entry::new(&label, Action::Focus(*id)));
        }
        out.push(Entry::sep());
    }
    out.push(Entry::new(&name, Action::Open));
    out.push(Entry::sep());
    out.push(if ctx.pinned { Entry::new("Unpin from taskbar", Action::RemoveFromDock) } else { Entry::new("Pin to taskbar", Action::KeepInDock) });
    if ctx.has_desktop {
        out.push(Entry::new("Open file location", Action::ShowInFinder));
    }
    if ctx.running {
        out.push(Entry::sep());
        out.push(Entry::new("Minimize", Action::Hide));
        match ctx.windows.as_slice() {
            [(id, _)] => out.push(Entry::new("Close window", Action::CloseWindow(*id))),
            _ => out.push(Entry::new("Close all windows", Action::Quit)),
        }
    }
    out
}

/// The dock's own menu (right click on the empty part of the dock or taskbar): style and, in the
/// Windows style, alignment and window titles. The current choice is shown greyed out.
pub fn build_entries_dock(style: DockStyle, align: WinAlign, labels: WinLabels) -> Vec<Entry> {
    let mut out = vec![
        Entry { enabled: style != DockStyle::Mac, ..Entry::new("Dock Style: macOS", Action::SetStyle(DockStyle::Mac)) },
        Entry { enabled: style != DockStyle::Windows, ..Entry::new("Dock Style: Windows", Action::SetStyle(DockStyle::Windows)) },
    ];
    if style == DockStyle::Windows {
        out.push(Entry::sep());
        out.push(match align {
            WinAlign::Center => Entry::new("Taskbar Alignment: Left", Action::SetAlign(WinAlign::Left)),
            WinAlign::Left => Entry::new("Taskbar Alignment: Center", Action::SetAlign(WinAlign::Center)),
        });
        out.push(match labels {
            WinLabels::Never => Entry::new("Show Window Titles", Action::SetLabels(WinLabels::Always)),
            WinLabels::Always => Entry::new("Hide Window Titles", Action::SetLabels(WinLabels::Never)),
        });
    }
    out.push(Entry::sep());
    out.push(Entry::new(if style == DockStyle::Windows { "Taskbar Settings…" } else { "Dock Settings…" }, Action::OpenSettings));
    out
}

/// The style switch appended to an item's menu in the macOS style.
pub fn style_entries() -> Vec<Entry> {
    vec![Entry::sep(), Entry::new("Dock Style: Windows", Action::SetStyle(DockStyle::Windows))]
}

/// What the owner should do after feeding the menu an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    None,
    /// Redraw only (hover or confirm state changed).
    Redraw,
    /// Run the action and close the menu.
    Run(Action),
    /// Close the menu without doing anything.
    Close,
}

pub struct DockMenu {
    /// The dock item the menu belongs to.
    pub item_id: String,
    pub entries: Vec<Entry>,
    pub hover: Option<usize>,
    /// Entry armed by its first activation, waiting for the confirming one.
    pub armed: Option<usize>,
    /// Entry the left button went down on (activates on release over the same entry).
    pressed: Option<usize>,
    /// Layout at y = 0; [`DockMenu::placed`] shifts it.
    base: Dropdown,
    menu: Menu,
    /// Wanted horizontal centre (the clicked cell), before clamping into the canvas.
    pub anchor_x: f32,
}

/// Cuts `s` to `max` pixels with an ellipsis, measuring with `w`.
pub fn truncate(s: &str, max: f32, w: &dyn Fn(&str) -> f32) -> String {
    if w(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        let mut next = out.clone();
        next.push(c);
        next.push('…');
        if w(&next) > max {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

impl DockMenu {
    /// `text_w` measures a string at the menu font; `anchor_x` is the clicked cell's centre.
    pub fn new(item_id: &str, mut entries: Vec<Entry>, anchor_x: f32, text_w: &dyn Fn(&str) -> f32) -> DockMenu {
        for e in &mut entries {
            if !e.is_sep() {
                e.label = truncate(&e.label, MAX_LABEL_W, text_w);
            }
        }
        let items = entries.iter().map(|e| Item { label: e.label.clone(), action: "", shortcut: "", separator: e.is_sep() }).collect();
        let menu = Menu { key: "dock", title: String::new(), icon: false, bold: false, items };
        let base = Dropdown::layout(&menu, 0.0, 0.0, text_w);
        DockMenu { item_id: item_id.to_string(), entries, hover: None, armed: None, pressed: None, base, menu, anchor_x }
    }

    pub fn size(&self) -> (f32, f32) {
        (self.base.w, self.base.h)
    }

    /// The menu laid out so its bottom edge is at `bottom`, centred on the anchor and kept inside a canvas `canvas_w` wide.
    pub fn placed(&self, canvas_w: f32, bottom: f32) -> Dropdown {
        let mut d = self.base.clone();
        let w = d.w.min((canvas_w - 8.0).max(1.0));
        d.w = w;
        d.x = (self.anchor_x - w / 2.0).clamp(4.0, (canvas_w - w - 4.0).max(4.0));
        let dy = bottom - d.h;
        d.y = dy;
        for r in &mut d.rows {
            r.y += dy;
        }
        d
    }

    fn enabled(&self, i: usize) -> bool {
        self.entries.get(i).is_some_and(|e| e.enabled && !e.is_sep())
    }

    fn set_hover(&mut self, h: Option<usize>) -> bool {
        if self.hover == h {
            return false;
        }
        self.hover = h;
        self.armed = None;
        true
    }

    /// Pointer moved over the menu laid out as `d`. True when a redraw is needed.
    pub fn pointer_moved(&mut self, d: &Dropdown, x: f32, y: f32) -> bool {
        let h = d.hit(&self.menu, x, y).filter(|&i| self.enabled(i));
        self.set_hover(h)
    }

    /// Left button went down at (`x`, `y`). Returns true when the click was inside the menu (consumed).
    pub fn press(&mut self, d: &Dropdown, x: f32, y: f32) -> bool {
        self.pressed = d.hit(&self.menu, x, y).filter(|&i| self.enabled(i));
        d.contains(x, y)
    }

    /// Left button released: activates the entry when the press started on the same one.
    pub fn release(&mut self, d: &Dropdown, x: f32, y: f32) -> Outcome {
        let down = self.pressed.take();
        let up = d.hit(&self.menu, x, y).filter(|&i| self.enabled(i));
        match (down, up) {
            (Some(a), Some(b)) if a == b => self.activate(a),
            _ => Outcome::None,
        }
    }

    /// Activates entry `i` (first activation only arms a confirming entry).
    pub fn activate(&mut self, i: usize) -> Outcome {
        if !self.enabled(i) {
            return Outcome::None;
        }
        let e = &self.entries[i];
        if e.confirm && self.armed != Some(i) {
            self.armed = Some(i);
            self.hover = Some(i);
            return Outcome::Redraw;
        }
        match e.action.clone() {
            Some(a) => Outcome::Run(a),
            None => Outcome::None,
        }
    }

    /// Return / Enter: activates the highlighted entry.
    pub fn activate_hovered(&mut self) -> Outcome {
        match self.hover {
            Some(i) => self.activate(i),
            None => Outcome::None,
        }
    }

    /// Moves the highlight to the next (`dir` = 1) or previous (-1) enabled entry, wrapping around.
    pub fn step(&mut self, dir: i32) -> bool {
        let n = self.entries.len() as i32;
        if n == 0 || !(0..n).any(|i| self.enabled(i as usize)) {
            return false;
        }
        let mut i = match self.hover {
            Some(h) => h as i32,
            None if dir > 0 => -1,
            None => n,
        };
        loop {
            i = (i + dir).rem_euclid(n);
            if self.enabled(i as usize) {
                return self.set_hover(Some(i as usize));
            }
        }
    }

    /// Paints the menu laid out as `d`.
    pub fn draw(&self, pm: &mut Pixmap, d: &Dropdown, fonts: &mut Fonts) {
        let rows: Vec<RowView> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| RowView {
                label: if self.armed == Some(i) { format!("{}?", e.label.trim_end_matches('…')) } else { e.label.clone() },
                shortcut: String::new(),
                separator: e.is_sep(),
                enabled: e.enabled,
                hovered: self.hover == Some(i) && e.enabled,
                danger: self.armed == Some(i),
            })
            .collect();
        draw_dropdown(pm, d, &rows, fonts);
    }
}

/// One row as the shared painter sees it.
pub struct RowView {
    pub label: String,
    pub shortcut: String,
    pub separator: bool,
    pub enabled: bool,
    pub hovered: bool,
    /// Drawn in red (a pending destructive confirmation).
    pub danger: bool,
}

/// Paints a macOS-style dropdown: rounded dark panel, hairline border, blue hover row. The look matches
/// the menu bar's dropdowns.
pub fn draw_dropdown(pm: &mut Pixmap, d: &Dropdown, rows: &[RowView], fonts: &mut Fonts) {
    fill_rounded(pm, d.x, d.y, d.w, d.h, crate::theme::th().radius, crate::theme::th().popup_c());
    stroke_rounded(pm, d.x + 0.5, d.y + 0.5, d.w - 1.0, d.h - 1.0, crate::theme::th().radius - 0.5, crate::theme::th().outline_c(), 1.0);
    for row in &d.rows {
        let Some(r) = rows.get(row.index) else { continue };
        if r.separator {
            fill_rect(pm, d.x + 8.0, row.y + row.h / 2.0, d.w - 16.0, 1.0, crate::theme::th().ink(0.12));
            continue;
        }
        if r.hovered {
            fill_rounded(pm, d.x + PAD, row.y, d.w - 2.0 * PAD, row.h, crate::theme::th().r(0.5), crate::theme::th().accent_c());
        }
        let alpha = if r.enabled { 1.0 } else { 0.35 };
        let main = if r.hovered {
            crate::theme::th().accent_text_c()
        } else if r.danger {
            color(255, 105, 97, 1.0)
        } else {
            crate::theme::th().text.color(alpha)
        };
        let sub = if r.hovered { crate::theme::th().accent_text_c() } else { crate::theme::th().ink(0.45 * alpha) };
        let top = row.y + (row.h - TEXT_PX) / 2.0 - 1.0;
        fonts.draw(pm, &r.label, d.x + 12.0, top, TEXT_PX, Weight::Regular, main);
        if !r.shortcut.is_empty() {
            let sw = fonts.measure(&r.shortcut, TEXT_PX, Weight::Regular);
            fonts.draw(pm, &r.shortcut, d.x + d.w - 12.0 - sw, top, TEXT_PX, Weight::Regular, sub);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> f32 {
        s.chars().count() as f32 * 7.0
    }

    #[test]
    fn windows_menu_uses_taskbar_wording() {
        let mut c = ctx("foot", true, true);
        c.windows = vec![(7, "shell".into())];
        let labels = |e: &[Entry]| e.iter().filter(|e| !e.is_sep()).map(|e| e.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&build_entries_windows(&c)), vec!["Foot", "Unpin from taskbar", "Open file location", "Minimize", "Close window"]);
        c.pinned = false;
        c.windows.push((8, "other".into()));
        let l = labels(&build_entries_windows(&c));
        assert_eq!(l[..2], ["shell", "other"], "several windows are listed first");
        assert!(l.contains(&"Pin to taskbar".to_string()) && l.contains(&"Close all windows".to_string()));
        c.running = false;
        assert!(!labels(&build_entries_windows(&c)).iter().any(|l| l == "Minimize"));
        let e = build_entries_windows(&ctx("foot", true, true));
        assert!(e.iter().any(|e| e.action == Some(Action::Quit)), "no window list means close all");
    }

    #[test]
    fn dock_menu_offers_the_style_switch() {
        let e = build_entries_dock(DockStyle::Mac, WinAlign::Center, WinLabels::Never);
        let mac = e.iter().find(|e| e.label == "Dock Style: macOS").unwrap();
        let win = e.iter().find(|e| e.label == "Dock Style: Windows").unwrap();
        assert!(!mac.enabled && win.enabled, "the current style is greyed out");
        assert_eq!(win.action, Some(Action::SetStyle(DockStyle::Windows)));
        assert!(!e.iter().any(|e| e.label.starts_with("Taskbar Alignment")));
        let e = build_entries_dock(DockStyle::Windows, WinAlign::Left, WinLabels::Always);
        assert!(e.iter().any(|e| e.action == Some(Action::SetAlign(WinAlign::Center))));
        assert!(e.iter().any(|e| e.action == Some(Action::SetLabels(WinLabels::Never))));
        assert!(e.iter().any(|e| e.action == Some(Action::OpenSettings)));
        assert_eq!(style_entries().len(), 2);
    }

    fn ctx(id: &str, running: bool, pinned: bool) -> Ctx {
        Ctx { id: id.into(), display_name: "Foot".into(), running, pinned, windows: vec![], has_desktop: true }
    }

    fn labels(es: &[Entry]) -> Vec<&str> {
        es.iter().map(|e| if e.is_sep() { "-" } else { e.label.as_str() }).collect()
    }

    #[test]
    fn running_app_lists_windows_then_options_then_hide_quit() {
        let mut c = ctx("foot", true, true);
        c.windows = vec![(5, "one".into()), (6, "".into())];
        let es = build_entries(&c);
        assert_eq!(labels(&es), ["one", "Foot", "-", "New Window", "-", "Remove from Dock", "Show in Finder", "-", "Hide", "Quit"]);
        assert_eq!(es[0].action, Some(Action::Focus(5)));
        assert_eq!(es[3].action, Some(Action::Open));
        assert_eq!(es[8].action, Some(Action::Hide));
        assert_eq!(es[9].action, Some(Action::Quit));
    }

    #[test]
    fn running_unpinned_offers_keep_in_dock() {
        let es = build_entries(&ctx("foot", true, false));
        assert!(labels(&es).contains(&"Keep in Dock") && !labels(&es).contains(&"Remove from Dock"));
        assert_eq!(labels(&es)[0], "New Window", "no windows listed -> no separator before it");
    }

    #[test]
    fn closed_pinned_app_opens_and_toggles() {
        let es = build_entries(&ctx("foot", false, true));
        assert_eq!(labels(&es), ["Open", "-", "Remove from Dock", "Show in Finder"]);
    }

    #[test]
    fn show_in_finder_needs_a_desktop_file() {
        let mut c = ctx("foot", false, true);
        c.has_desktop = false;
        assert!(!labels(&build_entries(&c)).contains(&"Show in Finder"));
    }

    #[test]
    fn trash_and_launchpad_menus() {
        let t = build_entries(&ctx("trash", false, true));
        assert_eq!(labels(&t), ["Open", "-", "Empty Trash…"]);
        assert!(t[2].confirm);
        assert_eq!(labels(&build_entries(&ctx("launchpad", false, true))), ["Open"]);
    }

    #[test]
    fn window_list_is_capped() {
        let mut c = ctx("foot", true, true);
        c.windows = (0..40).map(|i| (i, format!("w{i}"))).collect();
        let n = build_entries(&c).iter().filter(|e| matches!(e.action, Some(Action::Focus(_)))).count();
        assert_eq!(n, MAX_WINDOWS);
    }

    #[test]
    fn long_titles_are_truncated_with_an_ellipsis() {
        let t = truncate(&"x".repeat(100), 100.0, &w);
        assert!(t.ends_with('…') && w(&t) <= 100.0, "{t}");
        assert_eq!(truncate("short", 100.0, &w), "short");
    }

    fn open(id: &str, c: Ctx) -> (DockMenu, Dropdown) {
        let m = DockMenu::new(id, build_entries(&c), 300.0, &w);
        let d = m.placed(600.0, 100.0);
        (m, d)
    }

    #[test]
    fn placement_sits_above_the_bottom_and_stays_in_the_canvas() {
        let (m, d) = open("foot", ctx("foot", false, true));
        assert_eq!(d.y + d.h, 100.0);
        assert!((d.x + d.w / 2.0 - 300.0).abs() < 0.5);
        let mut far = DockMenu::new("foot", build_entries(&ctx("foot", false, true)), 5.0, &w);
        assert_eq!(far.placed(600.0, 100.0).x, 4.0);
        far.anchor_x = 599.0;
        let p = far.placed(600.0, 100.0);
        assert!(p.x + p.w <= 596.0 + 0.01);
        // a canvas narrower than the menu shrinks it instead of overflowing
        let narrow = m.placed(150.0, 100.0);
        assert!(narrow.x >= 4.0 && narrow.x + narrow.w <= 150.0);
    }

    #[test]
    fn hover_follows_the_pointer_and_skips_separators() {
        let (mut m, d) = open("foot", ctx("foot", false, true));
        let row = |i: usize| d.rows[i].y + 5.0;
        assert!(m.pointer_moved(&d, d.x + 30.0, row(0)));
        assert_eq!(m.hover, Some(0));
        assert!(!m.pointer_moved(&d, d.x + 31.0, row(0)), "same row: no redraw");
        assert!(m.pointer_moved(&d, d.x + 30.0, row(1)), "separator clears hover");
        assert_eq!(m.hover, None);
        assert!(m.pointer_moved(&d, d.x + 30.0, row(2)));
        assert!(m.pointer_moved(&d, d.x - 20.0, row(2)), "outside the menu");
        assert_eq!(m.hover, None);
    }

    #[test]
    fn keyboard_walks_enabled_rows_and_wraps() {
        let (mut m, _) = open("foot", ctx("foot", false, true)); // Open - Remove Show
        assert!(m.step(1));
        assert_eq!(m.hover, Some(0));
        m.step(1);
        assert_eq!(m.hover, Some(2), "skips the separator");
        m.step(1);
        m.step(1);
        assert_eq!(m.hover, Some(0), "wraps");
        m.step(-1);
        assert_eq!(m.hover, Some(3), "wraps backwards");
        let (mut m2, _) = open("foot", ctx("foot", false, true));
        m2.step(-1);
        assert_eq!(m2.hover, Some(3), "Up from nothing starts at the end");
    }

    #[test]
    fn enter_runs_the_hovered_action() {
        let (mut m, _) = open("foot", ctx("foot", false, true));
        assert_eq!(m.activate_hovered(), Outcome::None);
        m.step(1);
        assert_eq!(m.activate_hovered(), Outcome::Run(Action::Open));
    }

    #[test]
    fn click_activates_on_release_over_the_same_row() {
        let (mut m, d) = open("foot", ctx("foot", false, true));
        let (x, y0, y2) = (d.x + 30.0, d.rows[0].y + 5.0, d.rows[2].y + 5.0);
        assert!(m.press(&d, x, y0));
        assert_eq!(m.release(&d, x, y0), Outcome::Run(Action::Open));
        assert!(m.press(&d, x, y0));
        assert_eq!(m.release(&d, x, y2), Outcome::None, "released elsewhere");
        assert_eq!(m.release(&d, x, y0), Outcome::None, "no press pending");
        assert!(!m.press(&d, d.x - 30.0, y0), "outside is not consumed");
    }

    #[test]
    fn empty_trash_needs_a_second_activation() {
        let (mut m, _) = open("trash", ctx("trash", false, true));
        m.step(1);
        m.step(1);
        assert_eq!(m.hover, Some(2));
        assert_eq!(m.activate_hovered(), Outcome::Redraw);
        assert_eq!(m.armed, Some(2));
        assert_eq!(m.activate_hovered(), Outcome::Run(Action::EmptyTrash));
    }

    #[test]
    fn moving_away_disarms_the_confirmation() {
        let (mut m, _) = open("trash", ctx("trash", false, true));
        m.step(1);
        m.step(1);
        m.activate_hovered();
        m.step(1);
        assert_eq!(m.armed, None);
        m.step(1);
        assert_eq!(m.activate_hovered(), Outcome::Redraw, "must be armed again");
    }

    #[test]
    fn draws_without_panicking_when_fonts_exist() {
        let Some(mut f) = Fonts::load() else { return };
        let (mut m, d) = open("trash", ctx("trash", false, true));
        m.step(1);
        let mut pm = Pixmap::new(600, 200).unwrap();
        m.draw(&mut pm, &d, &mut f);
        assert!(pm.pixel(d.x as u32 + 40, (d.y + 3.0) as u32).unwrap().alpha() > 0);
        assert_eq!(pm.pixel(2, 2).unwrap().alpha(), 0);
    }
}
