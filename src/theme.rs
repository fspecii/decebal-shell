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
/// Width of a Caelestia side bar, including the strip it reserves.
pub const PILL_BAR_THICKNESS: f32 = 52.0;
/// Width of the classic menu bar, the floating bar or a taskbar-less bar when it runs down a side edge (KDE's default
/// vertical panel thickness is 44; the stacked clock and an icon cell fit 40).
pub const CLASSIC_RAIL_THICKNESS: f32 = 40.0;
/// Horizontal bars need less height than a side bar needs width.
pub const HORIZONTAL_PILL_BAR_THICKNESS: f32 = 40.0;

pub fn pill_bar_thickness(position: crate::config::BarPosition) -> f32 {
    use crate::config::BarPosition;
    match position {
        BarPosition::Left | BarPosition::Right => PILL_BAR_THICKNESS,
        BarPosition::Top | BarPosition::Bottom | BarPosition::Split => HORIZONTAL_PILL_BAR_THICKNESS,
    }
}

/// Logical top-edge extent of a visible bar. Placement consumers share this with
/// the bar renderer instead of assuming the classic 26px height. Side/bottom rails
/// do not occupy the top edge. `visible` is explicit so forced native bar fixtures
/// can ignore the user's process gate without doing file I/O while painting.
pub fn bar_bottom_for(theme: &model::Theme, visible: bool) -> f32 {
    if !visible {
        return 0.0;
    }
    if theme.pill_bar().is_some() {
        theme.bar_insets().top as f32
    } else if theme.bar_style == "dux" {
        theme.bar_height + 2.0 * theme.bar_layout.margin
    } else {
        theme.flat_bar_height()
    }
}

/// The least height a declared classic or floating bar can have at this text scale: the bar's 12 px text, scaled, plus 10 px of room
/// around it (22 px at scale 1). A declared height below it grows to it, so scaled text and the controls never clip; an undeclared bar keeps
/// its default 26 px.
pub fn flat_bar_floor(font_scale: f32) -> f32 {
    (12.0 * font_scale.clamp(0.5, 3.0) + 10.0).ceil()
}

/// Current top-edge bar extent for popup placement (outside the paint path).
pub fn bar_bottom() -> f32 {
    bar_bottom_for(&th(), crate::config::shell_settings().bar_visible(crate::config::dock_settings().style))
}

pub const BAR_BG: (u8, u8, u8) = (19, 19, 19);

// Shared, switchable theme (colours, metrics, fonts): see docs/theming.md. Kept in `theme/` so this
// file (the dock's layout tokens) stays untouched by it.
pub mod builtin;
pub mod dynamic;
pub mod engine;
pub mod extract;
pub mod fontdb;
pub mod fonts;
pub mod frame;
pub mod live;
pub mod material;
pub mod model;
pub mod palette;
pub mod rdwm;
pub mod style;
pub use fonts::Role;
pub mod system;
pub mod wallpaper_follow;
pub use live::{current, current as th, generation, take_changed};
pub use model::{Dynamic, Insets, Mode, Rgb, Selection, Theme};

#[cfg(test)]
mod placement_tests {
    use super::*;
    #[test]
    fn top_edge_extent_covers_classic_dux_rails_and_disabled_bars() {
        let mut theme = model::Def::parse("name = 'fixture'").unwrap().resolve("fixture", true, &Selection::default());
        assert_eq!(bar_bottom_for(&theme, true), 26.0);
        theme.bar_style = "dux".into();
        theme.bar_height = 38.0;
        theme.bar_layout.margin = 4.0;
        assert_eq!(bar_bottom_for(&theme, true), 46.0);
        assert_eq!(bar_bottom_for(&theme, false), 0.0);
        theme.bar_style = "vertical".into();
        for (position, expected) in [
            (crate::config::BarPosition::Top, 40.0),
            (crate::config::BarPosition::Split, 40.0),
            (crate::config::BarPosition::Left, 0.0),
            (crate::config::BarPosition::Right, 0.0),
            (crate::config::BarPosition::Bottom, 0.0),
        ] {
            theme.bar_position = position;
            theme.bar_position_written = Some(position);
            assert_eq!(bar_bottom_for(&theme, true), expected);
        }
    }
}
