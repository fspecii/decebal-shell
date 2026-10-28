//! Window tracking: which apps are running and which one is focused.
//!
//! One thread per display protocol keeps a shared [`WmState`] up to date and calls a
//! wake-up callback on every change, so UIs only redraw when something actually happened.

#[cfg(feature = "wayland")]
pub mod wayland;
#[cfg(feature = "x11")]
pub mod x11;

use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Window {
    /// Backend handle used to activate the window (X11 window id, Wayland protocol id).
    pub id: u64,
    pub app_id: String,
    pub title: String,
    pub active: bool,
    /// Desktop / tag the window lives on (EWMH `_NET_WM_DESKTOP`); `None` = unknown or on all.
    pub desktop: Option<u32>,
}

/// One workspace (EWMH desktop, rdwm tag, ext-workspace group member).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspace {
    pub name: String,
    pub active: bool,
    /// At least one window lives here (unknown on Wayland, where it stays false).
    pub occupied: bool,
    /// A window here asks for attention (EWMH `_NET_WM_STATE_DEMANDS_ATTENTION`); never set on the active workspace.
    pub urgent: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WmState {
    pub windows: Vec<Window>,
    pub workspaces: Vec<Workspace>,
}

/// Things a UI can ask the window manager to do.
pub trait Control: Send + Sync {
    /// Raise and focus the window with this [`Window::id`].
    fn activate(&self, id: u64);
    /// Switch to workspace `index` (position in [`WmState::workspaces`]).
    fn switch_workspace(&self, index: usize);
    /// Minimise (hide) the window; focusing it again restores it.
    fn minimize(&self, _id: u64) {}
    /// Ask the window to close (like its close button); the app may still ask to save.
    fn close(&self, _id: u64) {}
}

/// True when the running window manager is rdwm (the Wayland session sets `XDG_CURRENT_DESKTOP`,
/// the X11 one names its EWMH check window "rdwm").
pub fn is_rdwm() -> bool {
    let env = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').any(|d| d.eq_ignore_ascii_case("rdwm"));
    #[cfg(feature = "x11")]
    if !env && std::env::var_os("DISPLAY").is_some() && std::env::var("DECEBAL_BACKEND").as_deref() != Ok("wayland") {
        return x11::wm_name().as_deref() == Some("rdwm");
    }
    env
}

/// Marks workspaces that have windows on them.
pub fn mark_occupied(workspaces: &mut [Workspace], windows: &[Window]) {
    for (i, ws) in workspaces.iter_mut().enumerate() {
        ws.occupied = windows.iter().any(|w| w.desktop == Some(i as u32));
    }
}

/// Read access to window information; implemented by [`Tracker`] and by test doubles.
pub trait ActiveWindow {
    /// Human readable name of the focused app, if any window has focus.
    fn active_app_name(&self) -> Option<String>;
    /// Application ids that currently have at least one open window.
    fn running_ids(&self) -> Vec<String>;
}

impl WmState {
    pub fn active(&self) -> Option<&Window> {
        self.windows.iter().find(|w| w.active)
    }
}

/// True when a dock id (`foot`, `org.gnome.Nautilus`, `obsidian`) names the window's app id.
pub fn app_matches(dock_id: &str, app_id: &str) -> bool {
    let (d, a) = (dock_id.to_lowercase(), app_id.to_lowercase());
    if d.is_empty() || a.is_empty() {
        return false;
    }
    d == a || d.ends_with(&format!(".{a}")) || a.ends_with(&format!(".{d}"))
}

/// Turns an app id such as `org.gnome.Nautilus` or `foot` into a display name (`Nautilus`, `Foot`).
pub fn display_name(app_id: &str) -> String {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    let mut chars = last.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Handle to a running tracker thread plus its shared state.
#[derive(Clone, Default)]
pub struct Tracker {
    state: Arc<Mutex<WmState>>,
    control: Arc<Mutex<Option<Arc<dyn Control>>>>,
}

impl Tracker {
    fn lock(&self) -> MutexGuard<'_, WmState> {
        // A poisoned lock only means a tracker thread panicked; the data is still usable.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn snapshot(&self) -> WmState {
        self.lock().clone()
    }

    pub(crate) fn replace(&self, windows: Vec<Window>, workspaces: Vec<Workspace>) -> bool {
        let mut s = self.lock();
        if s.windows == windows && s.workspaces == workspaces {
            return false;
        }
        s.windows = windows;
        s.workspaces = workspaces;
        true
    }

    pub(crate) fn set_control(&self, c: Arc<dyn Control>) {
        *self.control.lock().unwrap_or_else(|p| p.into_inner()) = Some(c);
    }

    fn controller(&self) -> Option<Arc<dyn Control>> {
        self.control.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Asks the window manager to focus a window (no-op when the protocol cannot do it).
    pub fn activate(&self, id: u64) {
        if let Some(c) = self.controller() {
            c.activate(id);
        }
    }

    /// Asks the window manager to minimise a window.
    pub fn minimize(&self, id: u64) {
        if let Some(c) = self.controller() {
            c.minimize(id);
        }
    }

    /// Asks a window to close.
    pub fn close(&self, id: u64) {
        if let Some(c) = self.controller() {
            c.close(id);
        }
    }

    /// Every window of an app (matched like the dock matches ids), in window-list order.
    pub fn windows_for(&self, dock_id: &str) -> Vec<Window> {
        self.lock().windows.iter().filter(|w| app_matches(dock_id, &w.app_id)).cloned().collect()
    }

    /// Asks the window manager to switch to workspace `index`.
    pub fn switch_workspace(&self, index: usize) {
        if let Some(c) = self.controller() {
            c.switch_workspace(index);
        }
    }

    pub fn workspaces(&self) -> Vec<Workspace> {
        self.lock().workspaces.clone()
    }

    /// The first window of an app (matched like the dock matches ids), preferring the active one.
    pub fn window_for(&self, dock_id: &str) -> Option<Window> {
        let s = self.lock();
        let mut hits = s.windows.iter().filter(|w| app_matches(dock_id, &w.app_id));
        let first = hits.next()?;
        if first.active {
            return Some(first.clone());
        }
        Some(hits.find(|w| w.active).unwrap_or(first).clone())
    }

    /// Starts tracking on the current session. `on_change` runs on the tracker thread after each change.
    /// Returns a tracker even when no protocol is available (it then simply reports no windows).
    pub fn start<F: Fn() + Send + Sync + 'static>(on_change: F) -> Self {
        let tracker = Tracker::default();
        let notify: Notify = Arc::new(on_change);
        let forced = std::env::var("DECEBAL_BACKEND").ok();
        let mut started = false;
        #[cfg(feature = "wayland")]
        if !started && forced.as_deref() != Some("x11") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
            started = wayland::spawn(tracker.clone(), notify.clone());
        }
        #[cfg(feature = "x11")]
        if !started && forced.as_deref() != Some("wayland") && std::env::var_os("DISPLAY").is_some() {
            started = x11::spawn(tracker.clone(), notify.clone());
        }
        let _ = (started, forced, &notify);
        tracker
    }
}

/// Wake-up callback shared with the tracker threads.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

impl ActiveWindow for Tracker {
    fn active_app_name(&self) -> Option<String> {
        self.lock().active().map(|w| display_name(&w.app_id))
    }

    fn running_ids(&self) -> Vec<String> {
        self.lock().windows.iter().map(|w| w.app_id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_and_suffix() {
        assert!(app_matches("foot", "foot"));
        assert!(app_matches("org.gnome.Nautilus", "org.gnome.nautilus"));
        assert!(app_matches("org.gnome.Nautilus", "Nautilus"));
        assert!(!app_matches("foot", "footprint"));
        assert!(!app_matches("", "foot"));
    }

    #[test]
    fn display_names() {
        assert_eq!(display_name("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(display_name("foot"), "Foot");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn tracker_reports_changes_once() {
        let t = Tracker::default();
        let w = vec![Window { id: 1, app_id: "foot".into(), title: "x".into(), active: true, desktop: None }];
        assert!(t.replace(w.clone(), vec![]));
        assert!(!t.replace(w, vec![]));
        assert_eq!(t.active_app_name().as_deref(), Some("Foot"));
        assert_eq!(t.running_ids(), vec!["foot"]);
    }

    #[test]
    fn workspace_changes_count_as_changes() {
        let t = Tracker::default();
        let ws = |a| vec![Workspace { name: "1".into(), active: a, ..Default::default() }];
        assert!(t.replace(vec![], ws(true)));
        assert!(!t.replace(vec![], ws(true)));
        assert!(t.replace(vec![], ws(false)));
    }

    #[test]
    fn occupied_follows_window_desktops() {
        let mut ws: Vec<Workspace> = (1..=3).map(|n| Workspace { name: n.to_string(), ..Default::default() }).collect();
        let win = |id, d| Window { id, app_id: "a".into(), title: String::new(), active: false, desktop: d };
        mark_occupied(&mut ws, &[win(1, Some(2)), win(2, None), win(3, Some(9))]);
        assert_eq!(ws.iter().map(|w| w.occupied).collect::<Vec<_>>(), vec![false, false, true]);
    }

    #[test]
    fn window_for_prefers_the_active_one() {
        let t = Tracker::default();
        let w = |id, active| Window { id, app_id: "org.gnome.Nautilus".into(), title: String::new(), active, desktop: None };
        t.replace(vec![w(1, false), w(2, true), w(3, false)], vec![]);
        assert_eq!(t.window_for("nautilus").map(|w| w.id), Some(2));
        assert_eq!(t.window_for("foot"), None);
        t.replace(vec![w(5, false), w(6, false)], vec![]);
        assert_eq!(t.window_for("Nautilus").map(|w| w.id), Some(5));
    }

    #[test]
    fn requests_without_a_controller_are_ignored() {
        let t = Tracker::default();
        t.activate(1);
        t.switch_workspace(2);
    }

    #[test]
    fn requests_reach_the_controller() {
        use std::sync::atomic::{AtomicU64, Ordering};
        struct C(Arc<AtomicU64>);
        impl Control for C {
            fn activate(&self, id: u64) {
                self.0.store(id, Ordering::SeqCst);
            }
            fn switch_workspace(&self, index: usize) {
                self.0.store(1000 + index as u64, Ordering::SeqCst);
            }
        }
        let seen = Arc::new(AtomicU64::new(0));
        let t = Tracker::default();
        t.set_control(Arc::new(C(seen.clone())));
        t.activate(7);
        assert_eq!(seen.load(Ordering::SeqCst), 7);
        t.switch_workspace(3);
        assert_eq!(seen.load(Ordering::SeqCst), 1003);
    }
}
