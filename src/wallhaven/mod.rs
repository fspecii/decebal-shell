//! Wallhaven wallpaper browser: API, results model, layout, networking and UI.
//!
//! Everything except [`net`], [`wallpaper`] (process/network I/O) and [`ui`] (drawing) is
//! pure logic with unit tests.

pub mod api;
pub mod cache;
pub mod layout;
pub mod model;
pub mod net;
pub mod textfield;
pub mod ui;
pub mod wallpaper;
