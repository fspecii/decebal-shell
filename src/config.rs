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
        Self {
            app_spacing: 6.0,
            magnification: 1.4,
            transparency: 0.4,
            roundness: 0.45,
        }
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
        let mut out: Vec<String> = self
            .order
            .iter()
            .filter(|id| self.pinned.contains(id))
            .cloned()
            .collect();
        for id in &self.pinned {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
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
    std::fs::read_to_string(config_dir().join(name))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
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
        assert_eq!(
            (s.app_spacing, s.magnification, s.transparency, s.roundness),
            (6.0, 1.4, 0.4, 0.45)
        );
    }

    #[test]
    fn parses_partial_json() {
        let s: DockSettings = serde_json::from_str(r#"{"magnification": 2.0}"#).unwrap();
        assert_eq!(s.magnification, 2.0);
        assert_eq!(s.app_spacing, 6.0);
    }

    #[test]
    fn order_skips_unknown_and_appends_missing() {
        let p = Pinned {
            pinned: vec!["a".into(), "b".into(), "c".into()],
            order: vec!["c".into(), "x".into(), "a".into()],
        };
        assert_eq!(p.ordered(), vec!["c", "a", "b"]);
    }
}
