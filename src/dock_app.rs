//! Glue between [`Dock`] and a display backend: pointer handling and click-to-launch.

use crate::app::{App, Button, InputRect, Tick, Waker};
use crate::dock::Dock;
use crate::launch;
use crate::theme::PANEL_HEIGHT;
use crate::wm::{app_matches, ActiveWindow, Tracker};
use std::time::Duration;
use tiny_skia::Pixmap;

pub struct DockApp {
    pub dock: Dock,
    size: (u32, u32),
    tracker: Option<Tracker>,
    track_windows: bool,
    waker: Option<Waker>,
}

impl DockApp {
    pub fn new(dock: Dock) -> Self {
        let size = dock.canvas_size();
        Self { dock, size, tracker: None, track_windows: false, waker: None }
    }

    /// Show a running dot under every pinned app that has an open window (tracker starts with the event loop).
    pub fn track_windows(mut self) -> Self {
        self.track_windows = true;
        self
    }

    fn wake(&self) {
        if let Some(w) = &self.waker {
            w.wake();
        }
    }

    /// Focuses the app's window when it already runs (like macOS); false when a launch is needed.
    fn activate_running(&self, id: &str) -> bool {
        let Some(win) = self.tracker.as_ref().and_then(|t| t.window_for(id)) else { return false };
        if let Some(t) = &self.tracker {
            t.activate(win.id);
        }
        true
    }

    fn refresh_running(&mut self) {
        let Some(t) = &self.tracker else { return };
        let running = t.running_ids();
        for item in &mut self.dock.items {
            item.running = running.iter().any(|app| app_matches(&item.id, app));
        }
    }
}

impl App for DockApp {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        self.refresh_running();
        self.dock.draw(pm);
    }

    fn pointer_motion(&mut self, x: f32, _y: f32) -> bool {
        let changed = self.dock.hover_x != Some(x);
        self.dock.hover_x = Some(x);
        if changed {
            self.wake(); // start the easing animation
        }
        changed
    }

    fn pointer_leave(&mut self) -> bool {
        let was = self.dock.hover_x.take().is_some();
        if was {
            self.wake();
        }
        was
    }

    fn set_waker(&mut self, waker: Waker) {
        if self.track_windows && self.tracker.is_none() {
            let w = waker.clone();
            self.tracker = Some(Tracker::start(move || w.wake()));
        }
        self.waker = Some(waker);
    }

    fn tick(&mut self) -> Tick {
        let moving = self.dock.step(self.size.0 as f32);
        Tick { redraw: true, next: moving.then(|| Duration::from_millis(16)) }
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        if button != Button::Left {
            return false;
        }
        let (w, h) = self.size;
        if let Some(i) = self.dock.hit_test(w as f32, h as f32, x, y) {
            let id = self.dock.items[i].id.clone();
            if self.activate_running(&id) {
                return false;
            }
            if !launch::launch(&id) {
                eprintln!("macdock: could not launch {id}");
            }
        }
        false
    }

    fn input_rect(&self) -> Option<InputRect> {
        let (w, h) = self.size;
        let strip = (PANEL_HEIGHT + 8.0) as i32;
        Some(InputRect { x: 0, y: h as i32 - strip, w: w as i32, h: strip })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wm::{Control, Window};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    struct Rec(Arc<AtomicU64>);
    impl Control for Rec {
        fn activate(&self, id: u64) {
            self.0.store(id, Ordering::SeqCst);
        }
        fn switch_workspace(&self, _: usize) {}
    }

    fn app_with(windows: Vec<Window>) -> (DockApp, Arc<AtomicU64>) {
        let seen = Arc::new(AtomicU64::new(0));
        let tracker = Tracker::default();
        tracker.set_control(Arc::new(Rec(seen.clone())));
        tracker.replace(windows, vec![]);
        let mut app = DockApp::new(Dock::new(vec![], crate::config::DockSettings::default()));
        app.tracker = Some(tracker);
        (app, seen)
    }

    fn win(id: u64, app: &str) -> Window {
        Window { id, app_id: app.into(), title: String::new(), active: false, desktop: None }
    }

    #[test]
    fn clicking_a_running_app_focuses_its_window() {
        let (app, seen) = app_with(vec![win(7, "foot"), win(9, "firefox")]);
        assert!(app.activate_running("firefox"));
        assert_eq!(seen.load(Ordering::SeqCst), 9);
    }

    #[test]
    fn clicking_a_closed_app_falls_through_to_launch() {
        let (app, seen) = app_with(vec![win(7, "foot")]);
        assert!(!app.activate_running("firefox"));
        assert_eq!(seen.load(Ordering::SeqCst), 0);
        assert!(!DockApp::new(Dock::new(vec![], crate::config::DockSettings::default())).activate_running("foot"), "no tracker");
    }
}
