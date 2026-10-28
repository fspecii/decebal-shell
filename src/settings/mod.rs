#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]
//! macsettings: native settings app for rdwm and the macshell panels.
pub mod app;
pub mod binds;
pub mod fontpane;
pub mod icons;
pub mod iconspane;
pub mod io;
pub mod keys;
pub mod model;
pub mod packages;
#[cfg(test)]
mod packages_tests;
pub mod panes;
pub mod schema;
pub mod system;
pub mod systempane;
pub mod themecards;
pub mod themepane;
pub mod tomlcfg;
pub mod ui;
pub mod wallpapercolors;
