//! Design tokens, measured from the original Quickshell dock so the Rust port looks the same.

/// Visible icon size in logical pixels (measured from the original dock).
pub const ICON_SIZE: f32 = 47.0;
/// Width of one dock cell; the icon is centred inside it.
pub const CELL_W: f32 = 60.0;
/// Horizontal gap between dock cells.
pub const APP_SPACING: f32 = 6.0;
/// Panel padding left/right of the first/last cell.
pub const PANEL_PAD_X: f32 = 16.0;
/// The original dock leaves a little more room on the right; shift the content left by this much.
pub const CONTENT_NUDGE: f32 = 5.0;
/// Panel padding above the icons.
pub const PANEL_PAD_TOP: f32 = 7.0;
/// Total panel height at rest.
pub const PANEL_HEIGHT: f32 = 71.0;
/// Distance from the icon bottom to the running-app dot centre.
pub const DOT_OFFSET: f32 = 4.0;
/// Radius of the running-app dot.
pub const DOT_RADIUS: f32 = 2.0;

/// Dark dock background, `rgb(30,30,30)`, alpha = 1 - transparency.
pub const DOCK_BG: (u8, u8, u8) = (30, 30, 30);
/// Hairline border colour alpha (white).
pub const DOCK_BORDER_ALPHA: f32 = 0.05;

/// Menu bar height and background (`#131313`).
pub const BAR_HEIGHT: f32 = 26.0;
pub const BAR_BG: (u8, u8, u8) = (19, 19, 19);

// Shared, switchable theme (colours, metrics, fonts): see docs/theming.md. Kept in `theme/` so this
// file (the dock's layout tokens) stays untouched by it.
pub mod builtin;
pub mod dynamic;
pub mod engine;
pub mod live;
pub mod material;
pub mod model;
pub mod rdwm;
pub mod style;
pub use live::{current, current as th, generation, take_changed};
pub use model::{Dynamic, Mode, Rgb, Selection, Theme};
