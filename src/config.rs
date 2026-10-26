//! Reads the same JSON files the original dock uses, with safe defaults when they are missing.

use serde::Deserialize;
use std::path::PathBuf;

/// `dock-settings.json`
#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DockSettings {
    pub app_spacing: f32,
    pub magnification: f32,
    pub transparency: f32,
    pub roundness: f32,
}

impl Default for DockSettings {
    fn default() -> Self {
        Self { app_spacing: 6.0, magnification: 1.4, transparency: 0.4, roundness: 0.45 }
    }
}

/// `dock-pinned-macos.json`
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Pinned {
    pub pinned: Vec<String>,
    pub order: Vec<String>,
}

impl Pinned {
    /// Apps in display order; unknown ids in `order` are skipped, missing ones appended.
    pub fn ordered(&self) -> Vec<String> {
        let mut out: Vec<String> = self.order.iter().filter(|id| self.pinned.contains(id)).cloned().collect();
        for id in &self.pinned {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }
}

/// Apps a fresh install gets when no pinned list exists: the first installed entry of each group,
/// with the built-in Launchpad first and Trash last.
const DEFAULT_GROUPS: &[&[&str]] = &[
    &["org.gnome.Nautilus", "thunar", "org.kde.dolphin", "pcmanfm", "nemo", "pcmanfm-qt"],
    &["firefox", "org.mozilla.firefox", "google-chrome", "chromium", "brave-browser", "org.chromium.Chromium"],
    &["foot", "Alacritty", "kitty", "org.wezfurlong.wezterm", "com.mitchellh.ghostty", "xterm", "org.gnome.Console"],
];

/// Pinned ids to show: the user's list, or sensible defaults when there is none.
pub fn default_pinned(exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = vec!["launchpad".to_string()];
    for group in DEFAULT_GROUPS {
        if let Some(id) = group.iter().find(|id| exists(id)) {
            out.push((*id).to_string());
        }
    }
    out.push("trash".to_string());
    out
}

impl Pinned {
    /// The configured order, or [`default_pinned`] when the config is missing or empty.
    pub fn or_default(&self, exists: &dyn Fn(&str) -> bool) -> Vec<String> {
        let ordered = self.ordered();
        if ordered.is_empty() {
            default_pinned(exists)
        } else {
            ordered
        }
    }
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("MACSHELL_CONFIG")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|p| PathBuf::from(p).join("omarchy")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/omarchy")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn load<T: Default + for<'de> Deserialize<'de>>(name: &str) -> T {
    std::fs::read_to_string(config_dir().join(name)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn dock_settings() -> DockSettings {
    load("dock-settings.json")
}

pub fn pinned() -> Pinned {
    load("dock-pinned-macos.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_original() {
        let s = DockSettings::default();
        assert_eq!((s.app_spacing, s.magnification, s.transparency, s.roundness), (6.0, 1.4, 0.4, 0.45));
    }

    #[test]
    fn parses_partial_json() {
        let s: DockSettings = serde_json::from_str(r#"{"magnification": 2.0}"#).unwrap();
        assert_eq!(s.magnification, 2.0);
        assert_eq!(s.app_spacing, 6.0);
    }

    #[test]
    fn fresh_install_gets_launchpad_apps_and_trash() {
        let installed = |id: &str| matches!(id, "thunar" | "firefox" | "kitty" | "foot");
        // the first installed entry of each group wins
        assert_eq!(default_pinned(&installed), vec!["launchpad", "thunar", "firefox", "foot", "trash"]);
        assert_eq!(default_pinned(&|_| false), vec!["launchpad", "trash"]);
    }

    #[test]
    fn empty_config_falls_back_to_defaults() {
        let none = |_: &str| false;
        assert_eq!(Pinned::default().or_default(&none), vec!["launchpad", "trash"]);
        let p = Pinned { pinned: vec!["a".into()], order: vec![] };
        assert_eq!(p.or_default(&none), vec!["a"]);
    }

    #[test]
    fn order_skips_unknown_and_appends_missing() {
        let p = Pinned { pinned: vec!["a".into(), "b".into(), "c".into()], order: vec!["c".into(), "x".into(), "a".into()] };
        assert_eq!(p.ordered(), vec!["c", "a", "b"]);
    }
}
