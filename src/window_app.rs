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

/// Where a popup hangs under a bar, see [`WindowApp::popup_anchor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupAnchor {
    pub top: i32,
    pub center_x: Option<i32>,
}

impl PopupAnchor {
    /// Left edge of a window `w` wide on a monitor `mon_w` wide: centred on `center_x` (the monitor's middle when unset), 8 px from the edges at most.
    pub fn left(&self, mon_w: i32, w: i32) -> i32 {
        let cx = self.center_x.unwrap_or(mon_w / 2);
        let max = (mon_w - w - 8).max(0);
        (cx - w / 2).clamp(8.min(max), max)
    }
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
    /// X11 only: the app is shown as a borderless popup (override-redirect, keyboard and pointer grabbed)
    /// whose window is exactly the card; there is no full-screen dimmer.
    fn set_popup(&mut self, _popup: bool) {}
    /// Popup mode: the size the window should have right now (the backend resizes it when this changes).
    fn content_size(&self) -> Option<(u32, u32)> {
        None
    }
    /// Popup mode: centre the window vertically on the monitor instead of a fifth of the way down.
    fn popup_centered(&self) -> bool {
        false
    }
    /// Popup mode: hang the window under a bar instead: `top` pixels below the monitor's top edge and, when `center_x`
    /// is set, centred on that x offset from the monitor's left edge (kept on screen). Takes precedence over the other placements.
    fn popup_anchor(&self) -> Option<PopupAnchor> {
        None
    }
    /// Popup mode: a passive indicator (a HUD). The window takes no input at all: an empty input shape lets every
    /// click through, no event but exposure is selected, and neither the keyboard nor the pointer is grabbed or focused.
    fn popup_passive(&self) -> bool {
        false
    }
    /// Popup mode: place the window's bottom edge this fraction of the monitor height above the monitor's bottom
    /// instead of centring it (`popup_centered`) or a fifth of the way down.
    fn popup_bottom_margin(&self) -> Option<f32> {
        None
    }
    /// Popup mode: the card has translucent pixels. The backend uses a 32-bit visual when a compositor runs
    /// and tells the app through [`WindowApp::set_popup_alpha`]; without one the app must paint opaque.
    fn popup_translucent(&self) -> bool {
        false
    }
    /// Popup mode: whether the window really has an alpha channel (see [`WindowApp::popup_translucent`]).
    fn set_popup_alpha(&mut self, _alpha: bool) {}
    /// Popup mode, once the keyboard grab was tried: the X modifier state at that moment (`None` = the grab
    /// failed, e.g. another client holds it). An app that cannot work without the keyboard closes here.
    fn input_grabbed(&mut self, _mods: Option<u16>) -> Response {
        Response::NONE
    }
    /// Popup mode: close when another window takes the input focus (a click elsewhere). Apps that hold the
    /// keyboard grab on purpose return false: the window manager may move the focus (focus follows mouse) under them.
    fn popup_closes_on_focus_loss(&self) -> bool {
        true
    }
    /// Preferred window size on first map.
    fn initial_size(&self) -> (u32, u32);
    /// Called once before the event loop starts; clone and keep it for worker threads.
    fn set_waker(&mut self, waker: Waker);
    /// Size of the output the window is on, when the backend can tell (0 x 0 = unknown).
    fn set_screen(&mut self, w: u32, h: u32);
    /// UI scale factor (1.0 = 96 dpi) when the backend can tell; default no-op.
    fn set_scale(&mut self, _scale: f32) {}
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
    /// A key was released: its keysym (unshifted). Rarely needed; used for "commit when Alt is let go".
    fn key_release(&mut self, _sym: u32) -> Response {
        Response::NONE
    }
    /// A key press the toolkit has no [`Key`] for (function keys, ...): raw keysym and modifiers.
    fn unmapped_key(&mut self, _sym: u32, _shift: bool, _ctrl: bool, _alt: bool) -> Response {
        Response::NONE
    }
    /// The window was mapped (true) or unmapped (false), e.g. moved to a hidden tag. Apps that poll stop while hidden.
    fn set_visible(&mut self, _visible: bool) {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_centres_and_stays_on_screen() {
        let a = PopupAnchor { top: 30, center_x: Some(800) };
        assert_eq!(a.left(1600, 400), 600);
        assert_eq!(PopupAnchor { top: 30, center_x: Some(10) }.left(1600, 400), 8, "kept 8 px from the left edge");
        assert_eq!(PopupAnchor { top: 30, center_x: Some(1590) }.left(1600, 400), 1192, "kept 8 px from the right edge");
        assert_eq!(PopupAnchor { top: 30, center_x: None }.left(1000, 400), 300, "centred on the monitor when unset");
        // A window wider than the monitor must not panic.
        assert_eq!(PopupAnchor { top: 0, center_x: Some(50) }.left(300, 400), 0);
    }
}
