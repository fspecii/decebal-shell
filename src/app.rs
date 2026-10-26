//! The contract between a UI (dock, menu bar, ...) and a display backend.

use tiny_skia::Pixmap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
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

pub trait App {
    /// Desired surface size in pixels.
    fn size(&self) -> (u32, u32);
    /// Paint the whole surface. `pm` is `size()` pixels.
    fn draw(&mut self, pm: &mut Pixmap);
    /// Pointer moved; return true when a redraw is needed.
    fn pointer_motion(&mut self, x: f32, y: f32) -> bool;
    fn pointer_leave(&mut self) -> bool;
    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool;
    /// Area that takes input; everything else lets clicks fall through to windows below.
    fn input_rect(&self) -> Option<InputRect> {
        None
    }
}
