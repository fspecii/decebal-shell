#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]
//! decsettings: native settings app for rdwm and the decebal panels.
pub mod app;
pub mod appearance;
pub mod binds;
pub mod ccpane;
pub mod configpane;
pub mod decorations;
pub mod editor;
pub mod fontpane;
pub mod icons;
pub mod iconspane;
pub mod io;
pub mod keys;
pub mod model;
pub mod navigation;
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
pub mod wallhaven;
pub mod wallpapercolors;
pub mod widgetbuilder;
pub mod widgetchoice;
pub mod widgetspane;
