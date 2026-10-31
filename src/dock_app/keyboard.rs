//! Keyboard mode of the macOS-style dock (`decdock --focus`), the counterpart of the taskbar's in `windows.rs`.
//!
//! Started only by the explicit `--focus` request (SIGUSR2), never by the pointer or by a theme: until then the dock asks the backend for no
//! keyboard at all. While it is on, the backend holds the keyboard (an X11 grab / Wayland on-demand interactivity), so no key reaches another
//! window, and every way out hands it back: Enter or Space activates, Escape leaves, a pointer press leaves.
//!
//! * Arrows along the dock's axis (Left/Right of a top or bottom dock, Up/Down of a side one) move the ring, wrapping at the ends;
//!   Home / End jump to the first / last item. The pointer never moves, and the magnification (which follows the pointer) stays put.
//! * The arrow towards the desktop (Up on a bottom dock, Down on a top one, Right on a left one, Left on a right one) opens the item's
//!   context menu, the very menu a right click opens, with the same typed actions. Inside it Up / Down / Home / End move, Enter or Space run
//!   the row, Escape closes the menu and keeps the ring.
//! * Enter or Space on an item does what a click does: focus the running app's window, else start it (Launchpad and Trash included).
//!   The app takes the keyboard from there, so nothing is restored.
//! * Escape (with no menu open) leaves and puts the keyboard focus back on the window that had it when the mode began, if the window
//!   manager moved it meanwhile and the window still exists.
//! * The ring follows its item by id when the item list changes; an emptied dock leaves the mode.

use super::DockApp;
use crate::app::NavKey;
use crate::dock_menu::Outcome;
use crate::panel::Edge;

/// The keyboard position: the focused item's id (followed across list changes), its last index, and the window that had the focus before.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Kb {
    pub id: String,
    pub index: usize,
    pub prev: Option<u64>,
}

/// What an arrow key means on a dock at `edge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Back,
    Forward,
    Menu,
}

fn step_for(edge: Edge, key: NavKey) -> Option<Step> {
    use NavKey::*;
    match (edge, key) {
        (Edge::Top | Edge::Bottom, Left) | (Edge::Left | Edge::Right, Up) => Some(Step::Back),
        (Edge::Top | Edge::Bottom, Right) | (Edge::Left | Edge::Right, Down) => Some(Step::Forward),
        (Edge::Bottom, Up) | (Edge::Top, Down) | (Edge::Left, Right) | (Edge::Right, Left) => Some(Step::Menu),
        _ => None,
    }
}

impl DockApp {
    /// `decdock --focus` on the macOS dock: ring on the first item, menus closed. False when the dock is empty (nothing to focus).
    pub(super) fn kb_start(&mut self) -> bool {
        self.close_menu();
        let Some(first) = self.dock.items.first() else {
            self.kb = None;
            return false;
        };
        let prev = self.tracker.as_ref().and_then(|t| t.snapshot().active().map(|w| w.id));
        self.dock.hover_x = None;
        self.kb = Some(Kb { id: first.id.clone(), index: 0, prev });
        self.wake();
        true
    }

    /// Called every tick after the item list is synchronised: starts the mode on request and keeps the ring on its item.
    pub(super) fn kb_sync(&mut self, start: bool) -> bool {
        let started = start && self.kb_start();
        let Some(kb) = self.kb.as_ref() else { return started };
        if self.dock.items.is_empty() {
            self.kb = None;
            return true;
        }
        let at = self.dock.items.iter().position(|i| i.id == kb.id).unwrap_or_else(|| kb.index.min(self.dock.items.len() - 1));
        let id = self.dock.items[at].id.clone();
        let moved = at != kb.index || id != kb.id;
        if let Some(kb) = self.kb.as_mut() {
            kb.index = at;
            kb.id = id;
        }
        started || moved
    }

    /// Leaves the mode. `restore` hands the keyboard focus back to the window that had it before (Escape); an activation does not.
    pub(super) fn kb_leave(&mut self, restore: bool) -> bool {
        let Some(kb) = self.kb.take() else { return false };
        if let (true, Some(prev), Some(t)) = (restore, kb.prev, self.tracker.as_ref()) {
            let state = t.snapshot();
            let still_there = state.windows.iter().any(|w| w.id == prev);
            if still_there && state.active().map(|w| w.id) != Some(prev) {
                t.activate(prev);
            }
        }
        self.wake();
        true
    }

    /// Escape: an open menu closes first (the ring stays), then the mode ends.
    pub(super) fn kb_escape(&mut self) -> bool {
        self.close_menu() || self.kb_leave(true)
    }

    fn kb_set(&mut self, index: usize) -> bool {
        let Some(item) = self.dock.items.get(index) else { return false };
        let id = item.id.clone();
        if let Some(kb) = self.kb.as_mut() {
            kb.index = index;
            kb.id = id;
        }
        self.wake();
        true
    }

    /// Arrow keys and Enter while the ring is on and no menu is open.
    pub(super) fn kb_nav_key(&mut self, key: NavKey) -> bool {
        let Some(at) = self.kb.as_ref().map(|k| k.index) else { return false };
        let n = self.dock.items.len();
        if n == 0 {
            return self.kb_leave(false);
        }
        let at = at.min(n - 1);
        if key == NavKey::Enter {
            // a click: the app (or the launcher) takes the keyboard, so nothing is handed back
            self.kb_leave(false);
            self.click_item(at);
            return true;
        }
        match step_for(self.dock.edge(), key) {
            Some(Step::Back) => self.kb_set((at + n - 1) % n),
            Some(Step::Forward) => self.kb_set((at + 1) % n),
            Some(Step::Menu) => {
                self.open_menu(at);
                true
            }
            None => false,
        }
    }

    /// Home / End: the ring to the first / last item, or the highlight to the first / last enabled menu row.
    pub(super) fn kb_home_end(&mut self, end: bool) -> bool {
        if let Some(m) = self.menu.as_mut() {
            m.menu.hover = None;
            return m.menu.step(if end { -1 } else { 1 });
        }
        if self.kb.is_none() {
            return false;
        }
        match self.dock.items.len() {
            0 => self.kb_leave(false),
            n => self.kb_set(if end { n - 1 } else { 0 }),
        }
    }

    /// Enter on a highlighted menu row while the ring is on: the action runs and the mode ends (the action owns the focus now).
    pub(super) fn kb_menu_enter(&mut self, out: Outcome) -> bool {
        let ran = matches!(out, Outcome::Run(_));
        let changed = self.finish_menu(out);
        if ran {
            self.kb = None;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::Preferences;
    use crate::app::{App, Button, Key};
    use crate::config::DockSettings;
    use crate::dock::{Dock, DockItem};
    use crate::dock_app::Effects;
    use crate::wm::{Control, Tracker, Window};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct Log {
        activated: Vec<u64>,
        minimized: Vec<u64>,
        closed: Vec<u64>,
        launched: Vec<String>,
        argv: Vec<Vec<String>>,
    }

    struct Rec(Arc<Mutex<Log>>);
    impl Control for Rec {
        fn activate(&self, id: u64) {
            self.0.lock().unwrap().activated.push(id);
        }
        fn switch_workspace(&self, _: usize) {}
        fn minimize(&self, id: u64) {
            self.0.lock().unwrap().minimized.push(id);
        }
        fn close(&self, id: u64) {
            self.0.lock().unwrap().closed.push(id);
        }
    }

    struct Fx(Arc<Mutex<Log>>);
    impl Effects for Fx {
        fn launch(&self, id: &str) -> bool {
            self.0.lock().unwrap().launched.push(id.to_string());
            true
        }
        fn spawn(&self, argv: &[String]) -> bool {
            self.0.lock().unwrap().argv.push(argv.to_vec());
            true
        }
    }

    struct Rig {
        app: DockApp,
        log: Arc<Mutex<Log>>,
        dir: PathBuf,
    }

    fn window(id: u64, app: &str, active: bool) -> Window {
        Window { id, app_id: app.into(), title: format!("{app} window"), active, desktop: None }
    }

    /// A dock at `edge` with these pinned ids (plus the fixed Launchpad / Trash ends if asked), these windows and private files.
    fn rig(name: &str, edge: Edge, pinned: &[&str], windows: Vec<Window>, reduce_motion: bool) -> Rig {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("decebal-dockkb-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let items: Vec<DockItem> = pinned.iter().map(|id| DockItem::pinned(id, false)).collect();
        let log = Arc::new(Mutex::new(Log::default()));
        let tracker = Tracker::default();
        tracker.set_control(Arc::new(Rec(log.clone())));
        tracker.replace(windows, vec![]);
        let settings = DockSettings { edge: Some(edge), ..DockSettings::default() };
        let prefs = Preferences { reduce_motion, ..Preferences::default() };
        let dock = Dock::with_preferences(items, settings, prefs);
        let mut app = DockApp::new(dock).with_effects(Box::new(Fx(log.clone()))).watch_files(dir.join("pinned.json"), dir.join("settings.json"));
        app.tracker = Some(tracker);
        app.watch.as_mut().unwrap().every = Duration::ZERO;
        app.tick();
        let (w, h) = app.size();
        app.resized(w, h);
        Rig { app, log, dir }
    }

    impl Rig {
        fn focus(&mut self) -> bool {
            self.app.kb_start()
        }
        fn at(&self) -> Option<usize> {
            self.app.kb.as_ref().map(|k| k.index)
        }
        fn id_at(&self) -> Option<String> {
            self.app.kb.as_ref().map(|k| k.id.clone())
        }
        fn settle(&mut self) {
            let (w, h) = self.app.size();
            self.app.resized(w, h);
        }
        fn labels(&self) -> Vec<String> {
            self.app.menu.as_ref().map(|m| m.menu.entries.iter().map(|e| e.label.clone()).collect()).unwrap_or_default()
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const EDGES: [Edge; 4] = [Edge::Bottom, Edge::Top, Edge::Left, Edge::Right];

    fn back_forward_menu(edge: Edge) -> (NavKey, NavKey, NavKey) {
        match edge {
            Edge::Bottom => (NavKey::Left, NavKey::Right, NavKey::Up),
            Edge::Top => (NavKey::Left, NavKey::Right, NavKey::Down),
            Edge::Left => (NavKey::Up, NavKey::Down, NavKey::Right),
            Edge::Right => (NavKey::Up, NavKey::Down, NavKey::Left),
        }
    }

    #[test]
    fn arrows_follow_the_axis_of_every_edge_wrap_and_home_end_jump() {
        for edge in EDGES {
            let (back, fwd, _) = back_forward_menu(edge);
            let mut r = rig("axis", edge, &["foot", "firefox", "nautilus", "trash"], vec![], false);
            assert!(!r.app.wants_keyboard() && !r.app.wants_pointer_grab(), "{edge:?}: the dock never takes the keyboard by itself");
            assert!(!r.app.nav_key(fwd) && !r.app.home_end(true), "{edge:?}: keys do nothing before --focus");
            assert!(r.focus() && r.at() == Some(0) && r.app.wants_keyboard() && r.app.wants_pointer_grab(), "{edge:?}");
            assert!(r.app.nav_key(fwd) && r.at() == Some(1) && r.app.nav_key(fwd) && r.at() == Some(2), "{edge:?}");
            assert!(r.app.nav_key(back) && r.at() == Some(1), "{edge:?}");
            assert!(r.app.home_end(true) && r.at() == Some(3), "{edge:?}: End");
            assert!(r.app.nav_key(fwd) && r.at() == Some(0), "{edge:?}: wraps forward");
            assert!(r.app.nav_key(back) && r.at() == Some(3), "{edge:?}: wraps back");
            assert!(r.app.home_end(false) && r.at() == Some(0), "{edge:?}: Home");
            // the cross-axis arrow away from the desktop is not bound
            let away = match edge {
                Edge::Bottom => NavKey::Down,
                Edge::Top => NavKey::Up,
                Edge::Left => NavKey::Left,
                Edge::Right => NavKey::Right,
            };
            assert!(!r.app.nav_key(away) && r.at() == Some(0) && r.app.menu.is_none(), "{edge:?}");
        }
    }

    #[test]
    fn the_ring_is_drawn_on_the_focused_item_only_on_every_edge_without_touching_the_pointer_state() {
        for edge in EDGES {
            let mut r = rig("ring", edge, &["foot", "firefox", "nautilus"], vec![], false);
            let (w, h) = r.app.size();
            let render = |app: &mut DockApp| {
                let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
                app.draw(&mut pm);
                pm.take()
            };
            let plain = render(&mut r.app);
            r.focus();
            let first = render(&mut r.app);
            assert_ne!(plain, first, "{edge:?}: the ring appears");
            assert_eq!(r.app.dock.focus, Some(0));
            r.app.nav_key(back_forward_menu(edge).1);
            let second = render(&mut r.app);
            assert_ne!(first, second, "{edge:?}: the ring moves with the focus");
            assert!(r.app.dock.hover_x.is_none(), "{edge:?}: keyboard focus does not fake a pointer hover (no magnification, no warp)");
            r.app.key(Key::Escape);
            assert_eq!(render(&mut r.app), plain, "{edge:?}: Escape removes the ring completely");
            assert!(r.app.dock.focus.is_none());
        }
    }

    #[test]
    fn enter_and_space_activate_like_a_click_launcher_and_trash_included() {
        for edge in EDGES {
            let mut r = rig("activate", edge, &["launchpad", "foot", "firefox", "trash"], vec![window(7, "foot", false), window(9, "firefox", true)], false);
            r.focus();
            assert!(r.app.nav_key(NavKey::Enter));
            assert_eq!(r.log.lock().unwrap().launched, ["launchpad"], "{edge:?}: the launcher starts");
            assert!(!r.app.wants_keyboard() && r.app.kb.is_none(), "{edge:?}: the keyboard is handed over");
            assert!(r.log.lock().unwrap().activated.is_empty(), "{edge:?}");
            // a running app: its window gets the focus
            r.focus();
            r.app.nav_key(back_forward_menu(edge).1);
            assert!(r.app.nav_key(NavKey::Enter));
            assert_eq!(r.log.lock().unwrap().activated, [7], "{edge:?}: a running app is focused, not started again");
            // a closed one starts (and bounces); Space is the same key as Enter to the apps
            r.focus();
            assert!(r.app.home_end(true) && r.app.nav_key(NavKey::Enter));
            assert_eq!(r.log.lock().unwrap().launched, ["launchpad", "trash"], "{edge:?}: Trash too");
        }
    }

    #[test]
    fn the_context_menu_opens_towards_the_desktop_and_runs_the_same_typed_actions() {
        for edge in EDGES {
            let (_, fwd, menu_key) = back_forward_menu(edge);
            let mut r = rig("menu", edge, &["foot", "firefox"], vec![window(7, "foot", false), window(8, "foot", true)], false);
            r.focus();
            r.app.nav_key(fwd);
            assert!(r.app.nav_key(menu_key), "{edge:?}");
            r.settle();
            assert!(r.app.menu.is_some() && r.app.wants_keyboard());
            assert_eq!(r.app.menu.as_ref().unwrap().menu.item_id, "firefox", "{edge:?}: the focused item's menu");
            let pointer_menu = {
                let mut p = rig("menu-ptr", edge, &["foot", "firefox"], vec![window(7, "foot", false), window(8, "foot", true)], false);
                p.app.open_menu(1); // what a right click on the second icon calls
                p.labels()
            };
            assert_eq!(r.labels(), pointer_menu, "{edge:?}: the keyboard opens exactly the menu a right click opens");
            // Escape closes the menu but the ring stays
            assert!(r.app.key(Key::Escape) && r.app.menu.is_none() && r.at() == Some(1) && r.app.wants_keyboard(), "{edge:?}");
        }
    }

    #[test]
    fn menu_rows_move_by_arrows_home_end_and_run_with_enter_then_the_mode_ends() {
        let mut r = rig("rows", Edge::Bottom, &["foot", "firefox"], vec![window(7, "foot", false), window(8, "foot", true)], false);
        r.focus();
        r.app.nav_key(NavKey::Up);
        r.settle();
        let rows = r.labels();
        assert!(rows.len() > 3, "{rows:?}");
        assert!(!r.app.nav_key(NavKey::Enter) && r.app.menu.is_some(), "Enter with nothing highlighted does nothing and keeps the menu");
        assert!(r.app.nav_key(NavKey::Down) && r.app.home_end(true), "End goes to the last enabled row");
        let last = r.app.menu.as_ref().unwrap().menu.hover.unwrap();
        assert!(r.app.home_end(false));
        let first = r.app.menu.as_ref().unwrap().menu.hover.unwrap();
        assert!(first < last);
        assert!(!r.app.nav_key(NavKey::Left) && !r.app.nav_key(NavKey::Right), "sideways keys do nothing inside a menu");
        assert!(r.app.menu.is_some() && r.at() == Some(0));
        // "Quit" closes both windows of foot: a real typed action from the pointer's menu, nothing invented
        r.app.close_menu();
        r.app.nav_key(NavKey::Up);
        r.settle();
        let quit = r.labels().iter().position(|l| l == "Quit").expect("Quit");
        for _ in 0..r.labels().len() {
            if r.app.menu.as_ref().unwrap().menu.hover == Some(quit) {
                break;
            }
            r.app.nav_key(NavKey::Down); // separators and disabled rows are skipped
        }
        assert_eq!(r.app.menu.as_ref().unwrap().menu.hover, Some(quit));
        assert!(r.app.nav_key(NavKey::Enter));
        let mut closed = r.log.lock().unwrap().closed.clone();
        closed.sort();
        assert_eq!(closed, [7, 8], "the same Quit action closes every window of the app");
        assert!(r.app.menu.is_none() && r.app.kb.is_none() && !r.app.wants_keyboard(), "the keyboard is released after the action");
    }

    #[test]
    fn escape_closes_the_menu_first_then_leaves_and_restores_the_previous_window_focus_only_when_it_moved() {
        let mut r = rig("escape", Edge::Bottom, &["foot", "firefox"], vec![window(7, "foot", false), window(8, "firefox", true)], false);
        r.focus();
        assert_eq!(r.app.kb.as_ref().unwrap().prev, Some(8));
        r.app.nav_key(NavKey::Up);
        r.settle();
        assert!(r.app.key(Key::Escape) && r.app.menu.is_none() && r.at() == Some(0) && r.app.wants_keyboard(), "menu first");
        assert!(r.app.key(Key::Escape) && r.app.kb.is_none() && !r.app.wants_keyboard() && !r.app.wants_pointer_grab());
        assert!(r.log.lock().unwrap().activated.is_empty(), "the previous window still has the focus: nothing to restore, nothing activated");
        assert!(!r.app.key(Key::Escape), "a third Escape is not ours");
        // the window manager moved the focus while the mode was on
        r.focus();
        r.app.tracker.as_ref().unwrap().replace(vec![window(7, "foot", true), window(8, "firefox", false)], vec![]);
        assert!(r.app.key(Key::Escape));
        assert_eq!(r.log.lock().unwrap().activated, [8], "the window that had the focus gets it back");
        // the previous window is gone: nothing to activate
        r.focus();
        r.app.tracker.as_ref().unwrap().replace(vec![window(7, "foot", true)], vec![]);
        r.app.key(Key::Escape);
        assert_eq!(r.log.lock().unwrap().activated, [8]);
        // an activation never restores
        r.app.tracker.as_ref().unwrap().replace(vec![window(7, "foot", false), window(8, "firefox", true)], vec![]);
        r.focus();
        r.app.tracker.as_ref().unwrap().replace(vec![window(7, "foot", false), window(8, "firefox", false)], vec![]);
        r.app.nav_key(NavKey::Enter);
        assert_eq!(r.log.lock().unwrap().activated, [8, 7], "only the activated app's window");
    }

    #[test]
    fn a_pointer_press_leaves_keyboard_mode_and_still_does_its_own_job() {
        let mut r = rig("press", Edge::Bottom, &["foot", "firefox"], vec![window(7, "foot", false)], false);
        r.focus();
        let (w, h) = r.app.surface;
        let l = r.app.dock.layout(w as f32);
        let c = l.cells[0];
        let (x, y) = (c.x + c.w / 2.0, h as f32 - 20.0);
        assert!(r.app.pointer_button(x, y, Button::Left), "leaving keyboard mode requests a repaint to erase its ring");
        assert!(r.app.kb.is_none() && !r.app.wants_keyboard() && !r.app.wants_pointer_grab(), "the press ends the mode and its grabs");
        r.app.pointer_release(x, y, Button::Left);
        assert_eq!(r.log.lock().unwrap().activated, [7], "the click itself works");
        // a press elsewhere (a click outside while the pointer is grabbed) also ends it
        r.focus();
        assert!(r.app.pointer_button(1.0, 1.0, Button::Left), "an outside press also repaints the settled dock");
        assert!(r.app.kb.is_none());
        // a menu opened by the keyboard is closed by a press outside, like a pointer menu
        r.focus();
        r.app.nav_key(NavKey::Up);
        r.settle();
        r.app.pointer_button(1.0, 1.0, Button::Left);
        assert!(r.app.menu.is_none() && r.app.kb.is_none() && !r.app.wants_keyboard());
    }

    #[test]
    fn empty_dock_single_item_and_a_shrinking_list_are_safe() {
        let mut r = rig("empty", Edge::Bottom, &[], vec![], false);
        assert!(!r.focus() && r.app.kb.is_none() && !r.app.wants_keyboard(), "an empty dock cannot take the keyboard");
        assert!(!r.app.nav_key(NavKey::Enter) && !r.app.home_end(true) && !r.app.key(Key::Escape));
        let mut r = rig("single", Edge::Left, &["foot"], vec![], false);
        r.focus();
        assert!(r.app.nav_key(NavKey::Down) && r.at() == Some(0) && r.app.nav_key(NavKey::Up) && r.at() == Some(0));
        assert!(r.app.home_end(true) && r.at() == Some(0));
        // the list changes under the ring: it follows its item by id, falls back inside the bounds, and an emptied dock leaves the mode
        let mut r = rig("shrink", Edge::Bottom, &["foot", "firefox", "nautilus"], vec![], false);
        r.focus();
        r.app.home_end(true);
        assert_eq!(r.id_at().as_deref(), Some("nautilus"));
        r.app.dock.items.insert(0, DockItem::pinned("kitty", false));
        assert!(r.app.kb_sync(false));
        assert_eq!((r.at(), r.id_at().as_deref()), (Some(3), Some("nautilus")), "the ring stays on its app when another appears before it");
        r.app.dock.items.retain(|i| i.id == "kitty");
        r.app.kb_sync(false);
        assert_eq!(r.at(), Some(0), "the ring's app left: clamped inside the list");
        r.app.dock.items.clear();
        assert!(r.app.kb_sync(false) && r.app.kb.is_none() && !r.app.wants_keyboard());
        assert!(!r.app.nav_key(NavKey::Down));
        // many items: the ring walks all of them and wraps
        let ids: Vec<String> = (0..60).map(|i| format!("app{i}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut r = rig("many", Edge::Bottom, &refs, vec![], false);
        r.focus();
        for i in 1..60 {
            assert!(r.app.nav_key(NavKey::Right) && r.at() == Some(i));
        }
        assert!(r.app.nav_key(NavKey::Right) && r.at() == Some(0));
        let (w, h) = r.app.size();
        let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
        r.app.draw(&mut pm);
    }

    #[test]
    fn reduced_motion_and_style_changes_leave_nothing_behind() {
        let mut r = rig("still", Edge::Bottom, &["foot", "firefox"], vec![], true);
        r.focus();
        r.app.nav_key(NavKey::Right);
        assert!(!r.app.dock.step(r.app.dock_len()), "no easing, no bounce while the ring moves");
        r.app.nav_key(NavKey::Enter);
        assert!(!r.app.dock.bouncing(), "a launch does not bounce with reduced motion");
        // switching to the Windows taskbar drops the mac ring; the taskbar's keyboard mode is its own
        r.focus();
        std::fs::write(r.dir.join("settings.json"), r#"{"style":"windows"}"#).unwrap();
        r.app.tick();
        assert!(r.app.is_windows() && r.app.kb.is_none());
        std::fs::write(r.dir.join("settings.json"), r#"{"style":"mac"}"#).unwrap();
        r.app.tick();
        assert!(!r.app.is_windows() && r.app.kb.is_none() && !r.app.wants_keyboard());
        assert!(!r.app.home_end(true), "without the mode, Home / End do nothing");
    }
}
