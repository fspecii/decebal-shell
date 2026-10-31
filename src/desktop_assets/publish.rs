//! The one place where a change of the icon or cursor theme made by the asset workflow (a background install that
//! applied a theme's matching pack) is handed to app integration, which the GTK lane owns (`theme::system`: GTK
//! `settings.ini`, gsettings, Qt, X resources). Nothing here writes toolkit files.
//!
//! Contract with the GTK lane: publishing the *icon and cursor theme* of a Follow-theme slot must not depend on the
//! unrelated global "Apply to all apps" opt-in. Until the GTK lane provides that entry point, this hands the change to
//! the existing debounced request, which publishes only when "Apply to all apps" is on (an honest limit, reported by
//! `selection_changed`'s caller).

use std::path::Path;

/// The icon / cursor selection in `shell_json` changed; returns whether app publication was requested on every path.
pub fn selection_changed(shell_json: &Path) -> bool {
    crate::theme::system::request(shell_json);
    crate::theme::system::Config::load(shell_json).enabled
}
