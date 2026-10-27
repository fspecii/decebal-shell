//! macshell: a small, fast macOS-style desktop shell (dock, menu bar, wallpaper browser).
//!
//! Drawing is done on the CPU into a premultiplied RGBA [`tiny_skia::Pixmap`], so the
//! same pixels can be pushed to a Wayland `wl_shm` buffer or an X11 ARGB window.

pub mod app;
pub mod apps;
pub mod backend;
pub mod calc;
pub mod clipboard;
pub mod config;
pub mod controlcenter;
pub mod dock;
pub mod dock_app;
pub mod dock_menu;
pub mod fuzzy;
pub mod gfx;
pub mod icons;
pub mod keysym;
pub mod launch;
pub mod launcher;
pub mod mem;
pub mod menubar;
pub mod settings;
pub mod single;
pub mod spotlight;
pub mod taskbar;
pub mod text;
pub mod theme;
pub mod units;
pub mod wallhaven;
pub mod window_app;
pub mod wm;
