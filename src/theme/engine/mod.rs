//! The theme engine: theme packages (`format = 1`), their validation, storage, application and undo.
//!
//! * [`manifest`] / [`schema`]: `theme.toml` and its key table (one source for the validator, `macthemectl theme schema` and the docs).
//! * [`archive`], [`sha256`], [`package`]: `.dtheme` files and package directories, with every safety rule in one place.
//! * [`store`]: the user and system theme folders; install, uninstall, list.
//! * [`apply`]: apply a theme to every config file it owns, atomically, with undo and timed preview.
//! * [`capture`]: snapshot the current look into a new package.
//! * [`cli`]: the `macthemectl theme ...` commands.
//!
//! Themes are data. Nothing here runs a command named by a theme, opens a path outside the package, or evaluates code.

pub mod apply;
pub mod archive;
pub mod capture;
pub mod cli;
pub mod manifest;
pub mod package;
pub mod schema;
pub mod sha256;
pub mod store;
pub mod writer;
