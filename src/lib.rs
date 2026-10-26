//! macshell: a small, fast macOS-style desktop shell (dock, menu bar, wallpaper browser).
//!
//! Drawing is done on the CPU into a premultiplied RGBA [`tiny_skia::Pixmap`], so the
//! same pixels can be pushed to a Wayland `wl_shm` buffer or an X11 ARGB window.

pub mod app;
pub mod backend;
pub mod config;
pub mod controlcenter;
pub mod dock;
pub mod dock_app;
pub mod gfx;
pub mod icons;
pub mod launch;
pub mod menubar;
pub mod text;
pub mod theme;
