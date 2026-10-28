//! Decebal: a small, fast, customisable desktop environment for rdwm (dock, bar, launcher, settings, themes, widgets).
//! It used to be called macshell; [`compat`] keeps the old names working.
//!
//! Drawing is done on the CPU into a premultiplied RGBA [`tiny_skia::Pixmap`], so the
//! same pixels can be pushed to a Wayland `wl_shm` buffer or an X11 ARGB window.

pub mod app;
pub mod apps;
pub mod backend;
pub mod calc;
pub mod clip;
pub mod clipboard;
pub mod compat;
pub mod config;
pub mod controlcenter;
pub mod dash;
pub mod dbus;
pub mod dlog;
pub mod dock;
pub mod dock_app;
pub mod dock_menu;
pub mod expose;
pub mod fuzzy;
pub mod gfx;
pub mod hot;
pub mod hud;
pub mod icons;
pub mod keysym;
pub mod launch;
pub mod launcher;
pub mod lock;
pub mod mem;
pub mod menubar;
pub mod notify;
pub mod preset;
pub mod procmon;
pub mod session;
pub mod settings;
pub mod single;
pub mod spotlight;
pub mod switcher;
pub mod taskbar;
pub mod text;
pub mod textface;
pub mod theme;
pub mod units;
pub mod util;
pub mod wallhaven;
pub mod widgets;
pub mod window_app;
pub mod wm;
