//! The `icons` object of `macshell.json` and the automatic theme choice.
//!
//! ```json
//! "icons": { "theme": "WhiteSur-dark", "size_scale": 1.0, "monochrome_tint": false, "fallback": "Adwaita" }
//! ```
//! Every key is optional. `theme` absent, empty or `"auto"` follows the desktop (GSettings, then GTK's
//! `settings.ini`); `size_scale` (0.6 - 1.25) shrinks or grows the artwork inside its square; `monochrome_tint`
//! repaints symbolic icons in the theme's text colour; `fallback` names the theme tried after the chosen one,
//! its parents and hicolor (`"none"` for no extra theme).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The key in `macshell.json`.
pub const KEY: &str = "icons";
pub const DEFAULT_FALLBACK: &str = "Adwaita";
pub const SCALE_MIN: f32 = 0.6;
pub const SCALE_MAX: f32 = 1.25;

#[derive(Debug, Clone, PartialEq)]
pub struct IconConfig {
    /// Theme directory name or display name; empty = automatic.
    pub theme: String,
    pub size_scale: f32,
    pub monochrome_tint: bool,
    pub fallback: String,
}

impl Default for IconConfig {
    fn default() -> Self {
        IconConfig { theme: String::new(), size_scale: 1.0, monochrome_tint: false, fallback: DEFAULT_FALLBACK.into() }
    }
}

impl IconConfig {
    pub fn is_auto(&self) -> bool {
        self.theme.is_empty() || self.theme.eq_ignore_ascii_case("auto")
    }

    /// Reads the `icons` object of a parsed `macshell.json`; anything missing or of the wrong type is the default.
    pub fn from_json(root: &Value) -> IconConfig {
        let mut c = IconConfig::default();
        let Some(o) = root.get(KEY).and_then(Value::as_object) else { return c };
        if let Some(t) = o.get("theme").and_then(Value::as_str) {
            c.theme = t.trim().to_string();
        }
        if let Some(s) = o.get("size_scale").and_then(Value::as_f64).filter(|s| s.is_finite()) {
            c.size_scale = (s as f32).clamp(SCALE_MIN, SCALE_MAX);
        }
        if let Some(b) = o.get("monochrome_tint").and_then(Value::as_bool) {
            c.monochrome_tint = b;
        }
        if let Some(f) = o.get("fallback").and_then(Value::as_str) {
            c.fallback = f.trim().to_string();
        }
        c
    }

    /// Only what differs from the default (plus `theme`, so a set theme is always visible in the file).
    pub fn to_json(&self) -> Value {
        let d = IconConfig::default();
        let mut o = serde_json::Map::new();
        o.insert("theme".into(), json!(if self.is_auto() { "auto" } else { self.theme.as_str() }));
        if (self.size_scale - d.size_scale).abs() > 1e-4 {
            o.insert("size_scale".into(), json!((f64::from(self.size_scale) * 100.0).round() / 100.0));
        }
        if self.monochrome_tint {
            o.insert("monochrome_tint".into(), json!(true));
        }
        if self.fallback != d.fallback {
            o.insert("fallback".into(), json!(self.fallback));
        }
        Value::Object(o)
    }

    pub fn read(path: &Path) -> IconConfig {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| IconConfig::from_json(&v)).unwrap_or_default()
    }

    /// Stores the object (atomically, other keys of the file kept).
    pub fn write(&self, path: &Path) -> Result<(), String> {
        crate::settings::io::json_set(path, KEY, Some(self.to_json()))
    }

    /// Removes the `icons` object: everything automatic again.
    pub fn reset(path: &Path) -> Result<(), String> {
        crate::settings::io::json_set(path, KEY, None)
    }
}

pub fn shell_json() -> PathBuf {
    crate::config::config_dir().join("macshell.json")
}

// ---------------------------------------------------------------- the desktop's own choice

/// `gsettings get org.gnome.desktop.interface icon-theme` prints `'Name'`.
pub fn parse_gsettings(out: &str) -> Option<String> {
    let t = out.trim();
    let t = t.strip_prefix("@s ").unwrap_or(t);
    let name = t.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')).or_else(|| t.strip_prefix('"').and_then(|r| r.strip_suffix('"')))?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `gtk-icon-theme-name=Name` in a GTK `settings.ini`.
pub fn parse_gtk_ini(text: &str) -> Option<String> {
    let mut in_settings = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_settings = line.eq_ignore_ascii_case("[Settings]");
        } else if in_settings {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim().eq_ignore_ascii_case("gtk-icon-theme-name") {
                    let v = v.trim().trim_matches(['"', '\'']).trim();
                    return (!v.is_empty()).then(|| v.to_string());
                }
            }
        }
    }
    None
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).unwrap_or_default()
}

/// The files whose change can change [`desktop_choice`] (for the hot-reload stamps).
pub fn desktop_files() -> Vec<PathBuf> {
    let c = config_home();
    vec![c.join("gtk-3.0/settings.ini"), c.join("gtk-4.0/settings.ini"), c.join("dconf/user")]
}

/// Asks `gsettings`, at most 1.5 s (it may sit on a missing D-Bus); `None` when it is not installed or says nothing.
fn gsettings() -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok());
        let _ = tx.send(out);
    });
    rx.recv_timeout(std::time::Duration::from_millis(1500)).ok().flatten().and_then(|o| parse_gsettings(&o))
}

/// What the desktop says the icon theme is: GSettings first, then GTK 3 / 4 `settings.ini`.
pub fn desktop_choice() -> Option<String> {
    gsettings().or_else(|| {
        let files = desktop_files();
        files[..2].iter().find_map(|f| std::fs::read_to_string(f).ok().and_then(|t| parse_gtk_ini(&t)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsettings_output_is_unquoted() {
        assert_eq!(parse_gsettings("'WhiteSur-dark'\n").as_deref(), Some("WhiteSur-dark"));
        assert_eq!(parse_gsettings("  'Papirus Dark' ").as_deref(), Some("Papirus Dark"));
        assert_eq!(parse_gsettings("@s 'Adwaita'").as_deref(), Some("Adwaita"));
        assert_eq!(parse_gsettings("''\n"), None, "unset");
        assert_eq!(parse_gsettings(""), None);
        assert_eq!(parse_gsettings("No such schema"), None);
    }

    #[test]
    fn gtk_settings_ini_is_read() {
        let ini = "[Settings]\ngtk-theme-name=Adw\ngtk-icon-theme-name = Tela-dark\ngtk-font-name=Sans 10\n";
        assert_eq!(parse_gtk_ini(ini).as_deref(), Some("Tela-dark"));
        assert_eq!(parse_gtk_ini("[Settings]\ngtk-icon-theme-name=\"Papirus\"\n").as_deref(), Some("Papirus"));
        assert_eq!(parse_gtk_ini("[Other]\ngtk-icon-theme-name=X\n"), None, "only the [Settings] group counts");
        assert_eq!(parse_gtk_ini("[Settings]\ngtk-theme-name=X\n"), None);
        assert_eq!(parse_gtk_ini("[Settings]\ngtk-icon-theme-name=\n"), None);
        assert_eq!(parse_gtk_ini("[Settings]\nGTK-ICON-THEME-NAME=Mixed\n").as_deref(), Some("Mixed"));
    }

    #[test]
    fn config_defaults_and_clamping() {
        let c = IconConfig::from_json(&json!({}));
        assert_eq!(c, IconConfig::default());
        assert!(c.is_auto());
        let c = IconConfig::from_json(&json!({"icons": {"theme": " Tela ", "size_scale": 9, "monochrome_tint": true, "fallback": "none"}}));
        assert_eq!((c.theme.as_str(), c.size_scale, c.monochrome_tint, c.fallback.as_str()), ("Tela", SCALE_MAX, true, "none"));
        let c = IconConfig::from_json(&json!({"icons": {"size_scale": 0.1}}));
        assert_eq!(c.size_scale, SCALE_MIN);
        let c = IconConfig::from_json(&json!({"icons": {"theme": 5, "size_scale": "big", "monochrome_tint": "yes", "fallback": null}}));
        assert_eq!(c, IconConfig::default(), "wrong types are ignored, not errors");
        assert_eq!(IconConfig::from_json(&json!({"icons": "Tela"})), IconConfig::default());
        assert!(IconConfig::from_json(&json!({"icons": {"theme": "Auto"}})).is_auto());
    }

    #[test]
    fn json_round_trip_writes_only_what_differs() {
        let d = IconConfig::default();
        assert_eq!(d.to_json(), json!({"theme": "auto"}));
        let c = IconConfig { theme: "Tela".into(), size_scale: 0.9, monochrome_tint: true, fallback: "none".into() };
        assert_eq!(c.to_json(), json!({"theme": "Tela", "size_scale": 0.9, "monochrome_tint": true, "fallback": "none"}));
        assert_eq!(IconConfig::from_json(&json!({"icons": c.to_json()})), c);
    }

    #[test]
    fn file_round_trip_keeps_other_keys_and_reset_removes_the_object() {
        let dir = std::env::temp_dir().join(format!("macshell-iconcfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("macshell.json");
        std::fs::write(&f, r#"{"workspaces": "off", "theme": {"name": "nord"}}"#).unwrap();
        let c = IconConfig { theme: "Papirus".into(), ..IconConfig::default() };
        c.write(&f).unwrap();
        assert_eq!(IconConfig::read(&f), c);
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(doc["workspaces"], "off");
        assert_eq!(doc["theme"]["name"], "nord", "the theme selection is untouched");
        IconConfig::reset(&f).unwrap();
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert!(doc.get("icons").is_none() && doc["workspaces"] == "off");
        assert_eq!(IconConfig::read(&f), IconConfig::default());
        assert_eq!(IconConfig::read(&dir.join("missing.json")), IconConfig::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
