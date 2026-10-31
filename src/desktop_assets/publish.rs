//! The one place where a change of the icon or cursor theme made by the asset workflow (a background install that
//! applied a theme's matching pack) is handed to app integration, which the GTK lane owns (`theme::system`: GTK
//! `settings.ini`, gsettings, Qt, X resources). Nothing here writes toolkit files.
//!
//! App integration is enabled by default; explicit master and target opt-outs must remain honored. The intended
//! narrow publication updates icon/cursor settings without reapplying unrelated application colours or fonts.
//! Until that entry point is integrated and accepted, this uses the existing full debounced request. Its boolean
//! reports the configured master switch, not proof that a worker started or that applications changed.

use std::path::Path;

/// Notify app integration of an icon/cursor change; returns whether its master switch is enabled.
/// Worker dispatch and successful application are asynchronous and are not proven by this return value.
pub fn selection_changed(shell_json: &Path) -> bool {
    crate::theme::system::request(shell_json);
    crate::theme::system::Config::load(shell_json).enabled
}
