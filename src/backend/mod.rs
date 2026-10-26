//! Display backends. Each one runs an [`App`](crate::app::App) until the surface closes.

#[cfg(feature = "wayland")]
pub mod wayland;
#[cfg(feature = "x11")]
pub mod x11;

use crate::app::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Bottom,
    Top,
}

#[derive(Debug, Clone, Copy)]
pub struct SurfaceSpec {
    pub anchor: Anchor,
    /// Gap between the screen edge and the surface, in pixels.
    pub margin: i32,
    /// Space other windows must leave free (0 = float above them).
    pub reserve: i32,
    pub namespace: &'static str,
}

/// Picks Wayland when a compositor with layer-shell is reachable, otherwise X11.
/// Set `MACSHELL_BACKEND=x11` or `wayland` to force one.
pub fn run<A: App + 'static>(app: A, spec: SurfaceSpec) -> Result<(), String> {
    let forced = std::env::var("MACSHELL_BACKEND").ok();
    #[cfg(feature = "wayland")]
    if forced.as_deref() != Some("x11")
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && wayland::supported()
    {
        return wayland::run(app, spec).map_err(|e| e.to_string());
    }
    #[cfg(feature = "x11")]
    if forced.as_deref() != Some("wayland") && std::env::var_os("DISPLAY").is_some() {
        return x11::run(app, spec).map_err(|e| e.to_string());
    }
    let _ = (app, spec, forced);
    Err(
        "no usable display: need a Wayland compositor with wlr-layer-shell, or an X11 DISPLAY"
            .into(),
    )
}
