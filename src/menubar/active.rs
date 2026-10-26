//! The frontmost application's display name for the bold menu-bar title.
//!
//! The window-manager side (which window is focused) is supplied by the lead through
//! [`ActiveWindow`]; until it is wired the bar shows "Finder", like the original.

use std::path::PathBuf;

pub trait ActiveWindow {
    /// App id or title of the focused window (e.g. `foot`, `org.gnome.Nautilus`), if any.
    fn active_app_name(&self) -> Option<String>;
}

/// Always reports no focused app, so the bar falls back to "Finder".
pub struct NoActiveWindow;

impl ActiveWindow for NoActiveWindow {
    fn active_app_name(&self) -> Option<String> {
        None
    }
}

/// A fixed app id, for tests and screenshots (`macbar --app foot`).
pub struct StaticApp(pub String);

impl ActiveWindow for StaticApp {
    fn active_app_name(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

fn desktop_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(h).join("applications"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("applications")));
    dirs
}

/// `Name=` from the `[Desktop Entry]` group.
pub fn parse_name(desktop: &str) -> Option<String> {
    let mut in_entry = false;
    for line in desktop.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("Name=") {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// `org.gnome.Nautilus` -> `Nautilus`, `foot` -> `Foot`.
pub fn fallback_name(app_id: &str) -> String {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    let mut chars = last.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Finder".to_string(),
    }
}

/// Human-readable app name: the `.desktop` entry's `Name=` when found, otherwise a tidied id.
pub fn display_name(app_id: &str) -> String {
    if app_id.is_empty() {
        return "Finder".to_string();
    }
    for dir in desktop_dirs() {
        for file in [format!("{app_id}.desktop"), format!("{}.desktop", app_id.to_lowercase())] {
            if let Ok(text) = std::fs::read_to_string(dir.join(file)) {
                if let Some(name) = parse_name(&text) {
                    return name;
                }
            }
        }
    }
    fallback_name(app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_capitalises_last_segment() {
        assert_eq!(fallback_name("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(fallback_name("foot"), "Foot");
        assert_eq!(fallback_name(""), "Finder");
    }

    #[test]
    fn parses_name_from_entry_group_only() {
        let t = "[Desktop Action x]\nName=Wrong\n[Desktop Entry]\nName=Files\nExec=nautilus\n";
        assert_eq!(parse_name(t).as_deref(), Some("Files"));
    }

    #[test]
    fn empty_id_is_finder() {
        assert_eq!(display_name(""), "Finder");
    }

    #[test]
    fn static_app_reports_its_id() {
        assert_eq!(StaticApp("foot".into()).active_app_name().as_deref(), Some("foot"));
    }

    #[test]
    fn no_active_window_reports_none() {
        assert_eq!(NoActiveWindow.active_app_name(), None);
    }
}
