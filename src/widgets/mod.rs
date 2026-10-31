//! Declarative desktop and bar widgets, written in TOML (`widgets/<name>.toml`), styled by the theme.
//!
//! A widget is a tree of layout containers (`row`, `col`, `stack`, `spacer`) and leaves (`text`, `icon`, `bar`, `ring`, `graph`,
//! `image`, `ascii`, `workspaces`) whose strings and numbers come from named data sources (`{cpu.usage:pct}`, `{clock:%H:%M}`). There is no
//! scripting: expressions are `path`, `path:format`, `path|fallback`. Clicks run an *action* from a fixed allowlist
//! ([`actions`]). A widget that comes from a theme package ([`Trust::Theme`]) can never run a command; a widget in the user's
//! own folder can use `exec` and `[commands]` only when `widgets.allow_exec = true` in `decebal.json` ([`Trust::UserExec`]).
//!
//! * [`dsl`]: the file format and its validator.  * [`fmt`]: format strings.  * [`data`]: values and live sources.
//! * [`layout`]: measuring and arranging.  * [`paint`]: drawing.  * [`config`]: which widgets run where.  * [`samples`]: shipped examples.
//! * `host`: the `decwidgets` daemon, on Wayland (`host_wayland`, layer-shell surfaces) or X11 (`host_x11`, windows).

pub mod actions;
pub mod catalog;
pub mod config;
pub mod data;
pub mod data_json;
pub mod dsl;
pub mod fetch;
pub mod fmt;
pub mod layout;
pub mod paint;
pub mod samples;

pub mod host;
#[cfg(feature = "wayland")]
mod host_wayland;
#[cfg(feature = "x11")]
mod host_x11;

/// Where a widget definition came from, which decides what it may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Shipped inside a theme package (or any file of unknown origin): safe actions only, no commands.
    Theme,
    /// The user's own widget folder, `widgets.allow_exec` off: same as `Theme` but the error says how to enable `exec`.
    User,
    /// The user's own widget folder with `widgets.allow_exec = true`: `exec` actions and `[commands.*]` are allowed.
    UserExec,
}

mod dots;
