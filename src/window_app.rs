//! The contract between a normal top-level window backend (xdg-shell / X11) and an application UI.
//!
//! Unlike [`crate::app::App`] (a layer-shell strip such as the dock), a `WindowApp` is a
//! resizable window that also takes keyboard input and wheel scrolling.

use crate::app::Button;
use std::sync::Arc;
use tiny_skia::Pixmap;

/// Wakes the backend's event loop from any thread; the backend then calls [`WindowApp::tick`].
pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    /// Any printable key; the text is in [`KeyEvent::text`].
    Char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    /// Text the key produces (empty for navigation keys or when Ctrl/Alt is held).
    pub text: String,
    /// For [`Key::Char`], the unshifted lowercase character (used for shortcuts such as Ctrl+A).
    pub ch: Option<char>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Things the app can ask the window system to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Close,
    /// Start an interactive window move (title-bar drag).
    Move,
    Minimize,
    ToggleMaximize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Response {
    pub redraw: bool,
    pub action: Option<Action>,
}

impl Response {
    pub const NONE: Response = Response { redraw: false, action: None };
    pub const REDRAW: Response = Response { redraw: true, action: None };

    pub fn act(action: Action) -> Response {
        Response { redraw: true, action: Some(action) }
    }
}

pub trait WindowApp {
    fn title(&self) -> String;
    /// Application id / window class, for window rules.
    fn app_id(&self) -> &'static str {
        "machaven"
    }
    /// Ask for a full-screen overlay above all windows (a layer surface) instead of a normal window.
    /// Backends without layer-shell fall back to a normal window and never call [`WindowApp::set_overlay`].
    fn overlay(&self) -> bool {
        false
    }
    /// The backend really is showing the app as a full-screen overlay.
    fn set_overlay(&mut self, _overlay: bool) {}
    /// Preferred window size on first map.
    fn initial_size(&self) -> (u32, u32);
    /// Called once before the event loop starts; clone and keep it for worker threads.
    fn set_waker(&mut self, waker: Waker);
    /// Size of the output the window is on, when the backend can tell (0 x 0 = unknown).
    fn set_screen(&mut self, w: u32, h: u32);
    /// The drawable area changed.
    fn resize(&mut self, w: u32, h: u32);
    /// Whether the window should draw rounded outer corners (false when tiled/maximised).
    fn set_rounded(&mut self, rounded: bool);
    /// Paint the whole window; `pm` is the current size.
    fn draw(&mut self, pm: &mut Pixmap);
    fn pointer_motion(&mut self, x: f32, y: f32) -> Response;
    fn pointer_leave(&mut self) -> Response;
    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response;
    /// Vertical scroll in pixels (positive = content moves up, i.e. scroll down).
    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> Response;
    fn key(&mut self, ev: &KeyEvent) -> Response;
    /// Called after the waker fired: drain results from worker threads here.
    fn tick(&mut self) -> Response;
}

/// Runs `app` in a normal top-level window: Wayland (xdg-shell) when the compositor offers it,
/// otherwise X11. Set `MACSHELL_BACKEND=x11` or `wayland` to force one.
pub fn run_window<A: WindowApp + 'static>(app: A) -> Result<(), String> {
    let forced = std::env::var("MACSHELL_BACKEND").ok();
    #[cfg(feature = "wayland")]
    if forced.as_deref() != Some("x11") && std::env::var_os("WAYLAND_DISPLAY").is_some() && crate::backend::wayland_window::supported() {
        return crate::backend::wayland_window::run(app).map_err(|e| e.to_string());
    }
    #[cfg(feature = "x11")]
    if forced.as_deref() != Some("wayland") && std::env::var_os("DISPLAY").is_some() {
        return crate::backend::x11_window::run(app).map_err(|e| e.to_string());
    }
    let _ = (app, forced);
    Err("no usable display: need a Wayland compositor with xdg-shell, or an X11 DISPLAY".into())
}
