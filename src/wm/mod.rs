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
    pub app_id: String,
    pub title: String,
    pub active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WmState {
    pub windows: Vec<Window>,
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
}

impl Tracker {
    fn lock(&self) -> MutexGuard<'_, WmState> {
        // A poisoned lock only means a tracker thread panicked; the data is still usable.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn snapshot(&self) -> WmState {
        self.lock().clone()
    }

    pub(crate) fn replace(&self, windows: Vec<Window>) -> bool {
        let mut s = self.lock();
        if s.windows == windows {
            return false;
        }
        s.windows = windows;
        true
    }

    /// Starts tracking on the current session. `on_change` runs on the tracker thread after each change.
    /// Returns a tracker even when no protocol is available (it then simply reports no windows).
    pub fn start<F: Fn() + Send + Sync + 'static>(on_change: F) -> Self {
        let tracker = Tracker::default();
        let notify: Notify = Arc::new(on_change);
        let forced = std::env::var("MACSHELL_BACKEND").ok();
        let mut started = false;
        #[cfg(feature = "wayland")]
        if !started
            && forced.as_deref() != Some("x11")
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            started = wayland::spawn(tracker.clone(), notify.clone());
        }
        #[cfg(feature = "x11")]
        if !started && forced.as_deref() != Some("wayland") && std::env::var_os("DISPLAY").is_some()
        {
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
        self.lock()
            .windows
            .iter()
            .map(|w| w.app_id.clone())
            .collect()
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
        let w = vec![Window {
            app_id: "foot".into(),
            title: "x".into(),
            active: true,
        }];
        assert!(t.replace(w.clone()));
        assert!(!t.replace(w));
        assert_eq!(t.active_app_name().as_deref(), Some("Foot"));
        assert_eq!(t.running_ids(), vec!["foot"]);
    }
}
