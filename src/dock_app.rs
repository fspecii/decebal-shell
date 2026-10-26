//! Glue between [`Dock`] and a display backend: pointer handling and click-to-launch.

use crate::app::{App, Button, InputRect};
use crate::dock::Dock;
use crate::launch;
use crate::theme::PANEL_HEIGHT;
use crate::wm::{app_matches, ActiveWindow, Tracker};
use tiny_skia::Pixmap;

pub struct DockApp {
    pub dock: Dock,
    size: (u32, u32),
    tracker: Option<Tracker>,
}

impl DockApp {
    pub fn new(dock: Dock) -> Self {
        let size = dock.canvas_size();
        Self {
            dock,
            size,
            tracker: None,
        }
    }

    /// Shows a running dot under every pinned app that has an open window.
    pub fn with_tracker(mut self, tracker: Tracker) -> Self {
        self.tracker = Some(tracker);
        self
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
        changed
    }

    fn pointer_leave(&mut self) -> bool {
        self.dock.hover_x.take().is_some()
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        if button != Button::Left {
            return false;
        }
        let (w, h) = self.size;
        if let Some(i) = self.dock.hit_test(w as f32, h as f32, x, y) {
            let id = self.dock.items[i].id.clone();
            if !launch::launch(&id) {
                eprintln!("macdock: could not launch {id}");
            }
        }
        false
    }

    fn input_rect(&self) -> Option<InputRect> {
        let (w, h) = self.size;
        let strip = (PANEL_HEIGHT + 8.0) as i32;
        Some(InputRect {
            x: 0,
            y: h as i32 - strip,
            w: w as i32,
            h: strip,
        })
    }
}
