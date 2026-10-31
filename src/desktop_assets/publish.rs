//! The one place where a change of the icon or cursor theme made by the asset workflow (a background install that
//! applied a theme's matching pack) is handed to app integration, which the GTK lane owns (`theme::system`: GTK
//! `settings.ini`, gsettings, Qt, X resources). Nothing here writes toolkit files.
//!
//! App integration is enabled by default; explicit master and target opt-outs remain honored. The narrow
//! publication updates icon/cursor settings without reapplying unrelated application colours or fonts. A pending
//! full theme update keeps its wider intent. A queued result is not proof that applications already changed.

use std::path::Path;

/// Notify app integration of an icon/cursor change; true when it accepted asynchronous publication.
/// Successful application is reported separately by the toolkit integration status.
pub fn selection_changed(shell_json: &Path) -> bool {
    matches!(crate::theme::system::publish_assets(shell_json), crate::theme::system::Publication::Requested)
}

#[cfg(test)]
mod tests {
    #[test]
    fn suppressed_asset_publication_reports_no_request_without_touching_files() {
        let path = std::env::temp_dir().join(format!("decebal-no-publication-{}", std::process::id())).join("decebal.json");
        assert!(!super::selection_changed(&path));
        assert!(!path.exists());
    }
}
