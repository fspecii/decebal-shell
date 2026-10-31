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

/// The screen edge the bar a popup hangs from sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edge {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn parse(s: &str) -> Option<Edge> {
        match s {
            "top" => Some(Edge::Top),
            "bottom" => Some(Edge::Bottom),
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            _ => None,
        }
    }
}

/// Where a popup hangs from a bar, see [`WindowApp::popup_anchor`]. `top` is the gap between the popup and the
/// screen edge the bar is on (named for the usual bar along the top). `center_x` is where along the bar the popup
/// centres: an x offset for a bar along the top or bottom, a y offset for one at a side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupAnchor {
    pub top: i32,
    pub center_x: Option<i32>,
    pub edge: Edge,
}

impl PopupAnchor {
    /// Left edge of a window `w` wide on a monitor `mon_w` wide: centred on `center_x` (the monitor's middle when unset), 8 px from the edges at most.
    pub fn left(&self, mon_w: i32, w: i32) -> i32 {
        match self.edge {
            Edge::Left => return self.top.min((mon_w - w).max(0)),
            Edge::Right => return (mon_w - w - self.top).max(0),
            Edge::Top | Edge::Bottom => {}
        }
        let cx = self.center_x.unwrap_or(mon_w / 2);
        let max = (mon_w - w - 8).max(0);
        (cx - w / 2).clamp(8.min(max), max)
    }

    /// Top edge of a window `h` high on a monitor `mon_h` high: the gap from the top, or from the bottom edge, or
    /// centred on the bar item (`center_x`) beside a bar at a side; 8 px from the edges at most.
    pub fn y(&self, mon_h: i32, h: i32) -> i32 {
        match self.edge {
            Edge::Top => self.top,
            Edge::Bottom => (mon_h - h - self.top).max(0),
            Edge::Left | Edge::Right => {
                let cy = self.center_x.unwrap_or(mon_h / 2);
                let max = (mon_h - h - 8).max(0);
                (cy - h / 2).clamp(8.min(max), max)
            }
        }
    }
}

/// Which monitor an X11 popup opens on, see [`WindowApp::popup_monitor`]: a RandR monitor (output) name, else the
/// monitor holding a root-window point, else the panels' monitor (primary, then the first).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MonitorHint {
    pub output: Option<String>,
    pub point: Option<(i32, i32)>,
}

/// How a compact Wayland card (see [`WindowApp::wants_compact_popup`]) follows its output's scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PopupScale {
    /// The app lays out and draws in logical pixels and does not apply [`WindowApp::set_scale`] itself: the card is
    /// drawn at 1x and the compositor stretches it (the right size, soft on a HiDPI output). No scale is passed in.
    #[default]
    Logical,
    /// The app multiplies its own layout by the scale given to [`WindowApp::set_scale`]: `content_size`, `set_screen`,
    /// `resize`, the drawn pixmap, pointer positions and the anchor are in device pixels, and the card is presented
    /// at the output's real integer or fractional scale (sharp). On Wayland that scale comes from the compositor,
    /// never from the X resource database (`Xft.dpi` belongs to XWayland clients).
    Applied,
}

/// Things the app can ask the window system to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Close,
    /// Ask the window manager to reveal/focus this window (subject to compositor policy).
    Activate,
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
        "dechaven"
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
    /// Popup mode: the monitor to open on. X11 defaults to the panels' monitor; a compact Wayland card matches the
    /// output name, else the output whose logical rectangle holds `point`, else lets the compositor place it (its
    /// focused output) and follows the output it really landed on.
    fn popup_monitor(&self) -> MonitorHint {
        MonitorHint::default()
    }
    /// Popup mode (X11): the root-window position of the monitor the popup opened on (called before `set_screen`).
    fn set_popup_origin(&mut self, _x: i32, _y: i32) {}
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
    /// Popup mode: cover the whole monitor (a dimmer with the UI centred on it) instead of being just the card.
    fn popup_fullscreen(&self) -> bool {
        false
    }
    /// An overlay that is only a card (anchored under a bar, a passive indicator, or sitting above the bottom edge)
    /// rather than a full-screen overlay. Wayland draws these as a card on a transparent layer surface; the
    /// full-screen overlays (Spotlight, the switcher, ...) never match.
    fn wants_compact_popup(&self) -> bool {
        self.overlay() && !self.popup_fullscreen() && (self.popup_passive() || self.popup_anchor().is_some() || self.popup_bottom_margin().is_some())
    }
    /// Popup mode: the card has translucent pixels. The backend uses a 32-bit visual when a compositor runs
    /// and tells the app through [`WindowApp::set_popup_alpha`]; without one the app must paint opaque.
    fn popup_translucent(&self) -> bool {
        false
    }
    /// Compact Wayland card: whether the app applies the output scale itself (see [`PopupScale`]).
    fn popup_scale(&self) -> PopupScale {
        PopupScale::Logical
    }
    /// Popup mode: whether the window really has an alpha channel (see [`WindowApp::popup_translucent`]).
    fn set_popup_alpha(&mut self, _alpha: bool) {}
    /// Popup mode, once the keyboard grab was tried: the X modifier state at that moment (`None` = the grab
    /// failed, e.g. another client holds it). An app that cannot work without the keyboard closes here.
    /// A compact Wayland card reports `Some` when the compositor really gave its surface the keyboard (with the
    /// modifiers translated to the X mask) and `None` when it did not within a short wait; a click on any output
    /// outside the card still closes it then.
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
    /// A normal window-manager close request. Editors may retain a failed draft.
    fn close_requested(&mut self) -> Response {
        Response::act(Action::Close)
    }
    /// Called after the waker fired: drain results from worker threads here.
    fn tick(&mut self) -> Response;
    /// Accessibility (AT-SPI2): the name of the application object when this window publishes a semantic tree.
    /// `None` (the default) keeps the accessibility bridge off for this window.
    fn a11y_name(&self) -> Option<String> {
        None
    }
    /// Describe the window as it was last drawn: roles, names, states, values, text and window-relative bounds.
    /// Called only while an assistive technology listens, right after [`WindowApp::draw`].
    fn a11y_tree(&mut self, _tree: &mut crate::a11y::TreeBuilder) {}
    /// Run one validated semantic action (the node still exists, is current, enabled and offers the operation).
    /// Implementations apply the app's own typed model actions; they never synthesise pointer or key events.
    fn a11y_action(&mut self, target: &crate::a11y::Target) -> Result<Response, crate::a11y::ActionError> {
        Err(crate::a11y::ActionError::Unsupported(format!("{} has no accessibility actions", target.node.role.name())))
    }
}

/// Runs `app` in a normal top-level window: Wayland (xdg-shell; layer-shell for overlays and cards) or X11, as
/// [`crate::backend::session::decide`] decides from `DECEBAL_BACKEND` (`x11` / `wayland` force one),
/// `XDG_SESSION_TYPE`, `DISPLAY` and `WAYLAND_DISPLAY`. The compositor is probed once, for the protocols this app
/// needs, and only when the policy would pick Wayland at all (never under a forced X11 backend).
pub fn run_window<A: WindowApp + 'static>(app: A) -> Result<(), String> {
    use crate::backend::session::{self, Mode};
    let forced = std::env::var("DECEBAL_BACKEND").ok();
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    // A server this build cannot talk to does not count as present.
    let display = cfg!(feature = "x11") && std::env::var_os("DISPLAY").is_some();
    let wayland = cfg!(feature = "wayland") && std::env::var_os("WAYLAND_DISPLAY").is_some();
    let (forced, session_type) = (forced.as_deref(), session_type.as_deref());
    // A card or HUD needs layer-shell and subsurfaces; as a plain xdg window it would be a stray, decorated window.
    // `decide` runs this probe at most once, and only when the policy reaches Wayland.
    #[cfg(feature = "wayland")]
    let probe = || crate::backend::wayland_window::probe().is_some_and(|s| s.runs(&app));
    #[cfg(not(feature = "wayland"))]
    let probe = || false;
    match session::decide(forced, session_type, display, wayland, probe) {
        #[cfg(feature = "wayland")]
        Some(Mode::Wayland) => crate::backend::wayland_window::run(app).map_err(|e| e.to_string()),
        #[cfg(feature = "x11")]
        Some(Mode::X11) => crate::backend::x11_window::run(app).map_err(|e| e.to_string()),
        #[allow(unreachable_patterns)]
        _ => {
            drop(app);
            Err("no usable display: need a Wayland compositor with xdg-shell (layer-shell and subsurfaces for popups), or an X11 DISPLAY".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_centres_and_stays_on_screen() {
        let a = PopupAnchor { top: 30, center_x: Some(800), edge: Edge::Top };
        assert_eq!(a.left(1600, 400), 600);
        assert_eq!(PopupAnchor { top: 30, center_x: Some(10), edge: Edge::Top }.left(1600, 400), 8, "kept 8 px from the left edge");
        assert_eq!(PopupAnchor { top: 30, center_x: Some(1590), edge: Edge::Top }.left(1600, 400), 1192, "kept 8 px from the right edge");
        assert_eq!(PopupAnchor { top: 30, center_x: None, edge: Edge::Top }.left(1000, 400), 300, "centred on the monitor when unset");
        // A window wider than the monitor must not panic.
        assert_eq!(PopupAnchor { top: 0, center_x: Some(50), edge: Edge::Top }.left(300, 400), 0);
    }
}
