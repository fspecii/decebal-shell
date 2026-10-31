//! The contract between a UI (dock, menu bar, ...) and a display backend.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;
use tiny_skia::Pixmap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// Keys the shell UIs care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Escape,
}

/// Navigation keys for menus (kept apart from [`Key`] so existing exhaustive matches keep compiling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Enter,
    /// Along a horizontal panel (or between menus).
    Left,
    Right,
}

/// A rectangle in surface coordinates that should receive pointer input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Lets worker threads wake the backend's event loop (e.g. when async data arrives).
#[derive(Clone)]
pub struct Waker(Arc<UnixStream>);

impl Waker {
    pub(crate) fn new(stream: UnixStream) -> Self {
        Self(Arc::new(stream))
    }

    /// Never blocks and never fails loudly: a full pipe already means "a wake is pending".
    pub fn wake(&self) {
        let _ = (&*self.0).write(&[1]);
    }
}

/// What a backend should do after calling [`App::tick`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tick {
    pub redraw: bool,
    /// Call `tick` again after this long even if nothing else happens (None = only on events/wakes).
    pub next: Option<Duration>,
}

pub trait App {
    /// Desired surface size in pixels. A width of 0 means "stretch to the output".
    /// May change at runtime (the backend resizes the surface).
    fn size(&self) -> (u32, u32);
    /// The surface's real size is now `w` x `h` (called after every configure).
    fn resized(&mut self, _w: u32, _h: u32) {}
    /// Known screen-space surface origin (X11). Wayland may not expose one.
    fn surface_origin(&mut self, _x: i32, _y: i32) {}
    /// Paint the whole surface. `pm` is the real surface size.
    fn draw(&mut self, pm: &mut Pixmap);
    /// Visible material bounds, separate from foreground content and unused canvas.
    fn glass_regions(&self) -> Vec<crate::glass::Region> {
        Vec::new()
    }
    /// Pointer moved; return true when a redraw is needed.
    fn pointer_motion(&mut self, x: f32, y: f32) -> bool;
    fn pointer_leave(&mut self) -> bool;
    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool;
    fn pointer_release(&mut self, _x: f32, _y: f32, _button: Button) -> bool {
        false
    }
    /// Scroll wheel / touchpad: `dy` > 0 means scroll down.
    fn scroll(&mut self, _x: f32, _y: f32, _dy: f32) -> bool {
        false
    }
    fn key(&mut self, _key: Key) -> bool {
        false
    }
    /// Up / Down / Enter while [`App::wants_keyboard`] is true.
    fn nav_key(&mut self, _key: NavKey) -> bool {
        false
    }
    /// Home (`end` false) or End (`end` true) while [`App::wants_keyboard`] is true; apps without a use for them ignore them.
    fn home_end(&mut self, _end: bool) -> bool {
        false
    }
    /// True while a popup is open that must see clicks anywhere on screen (to close on an outside click).
    /// X11 grabs the pointer for it; Wayland cannot, so the UI closes when the pointer leaves instead.
    fn wants_pointer_grab(&self) -> bool {
        false
    }
    /// True while the UI needs keyboard input (a menu is open).
    fn wants_keyboard(&self) -> bool {
        false
    }
    /// Single area that takes input; everything else lets clicks fall through.
    fn input_rect(&self) -> Option<InputRect> {
        None
    }
    /// Several input areas (preferred over `input_rect` when non-empty).
    fn input_rects(&self) -> Vec<InputRect> {
        self.input_rect().into_iter().collect()
    }
    /// Where the surface wants to sit (edge, margin, reserved space). Asked on every loop turn, so an app
    /// can change shape at runtime (the dock switching between its macOS and Windows styles); the backend
    /// then moves the surface and republishes the reserved space. `None` keeps the spec given at start.
    fn surface_spec(&self) -> Option<crate::backend::SurfaceSpec> {
        None
    }
    /// True when the backend should close the surface and return from `run` (the menu bar switched off).
    fn quit_requested(&self) -> bool {
        false
    }
    /// Called once at start; keep it to wake the loop from threads.
    fn set_waker(&mut self, _waker: Waker) {}
    /// The backend is about to block waiting for events: drop transient memory (parsed fonts and the
    /// like) that input handling or layout may have loaded since the last frame.
    ///
    /// Returns how soon the backend should call `idle` again even if nothing else happens (for
    /// deferred clean-up), or `None`.
    fn idle(&mut self) -> Option<Duration> {
        None
    }
    /// A tick wanted sooner than the last [`Tick::next`] said, because input started a timer (e.g. a close
    /// delay). Asked before the backend blocks; `None` = nothing pending.
    fn tick_in(&self) -> Option<Duration> {
        None
    }
    /// Called on wake-ups and timeouts.
    fn tick(&mut self) -> Tick {
        Tick::default()
    }
}
