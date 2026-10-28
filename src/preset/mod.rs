//! Presets: named, shareable snapshots of the decebal + rdwm configuration.
//!
//! A preset is a folder `~/.config/decebal/presets/<id>/` (or one `.decebal-preset` bundle file, a gzip-compressed
//! tar) holding `manifest.toml` and up to four fixed files plus an optional custom theme and wallpaper. See
//! `docs/presets.md`.
//!
//! * `parts`: which keys belong to which selectable part; cutting a part out of a file and putting it back.
//! * `bundle`: the strict bundle reader and the writer.
//! * `manifest`: `manifest.toml`.
//! * `validate`: file checks with line numbers (shared with the settings editor).
//! * `fsx`: atomic saves with timestamped backups and revert.
//! * `store`: capture, list, save, import, plan (preview) and apply.
//! * `builtin`: the presets that ship with decebal.
//! * `cli`: `decthemectl profile ...`.

pub mod builtin;
pub mod bundle;
pub mod cli;
pub mod fsx;
pub mod manifest;
pub mod parts;
pub mod store;
pub mod validate;

pub use manifest::{Manifest, Origin};
pub use parts::{Part, PartSet};
pub use store::{Env, Preset};

/// File extension of a bundle.
pub const EXT: &str = "decebal-preset";
