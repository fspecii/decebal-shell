//! Shell style presets: one word that sets the theme, the dock style and the menu bar coherently.
//!
//! | style | theme | dock (`dock-settings.json`) | menu bar (`macshell.json` `bar.enabled`) |
//! |---|---|---|---|
//! | `macos` | macOS | `style: mac`, theme-driven roundness | `true` |
//! | `windows` | Windows 11 | `style: windows`, `windowsAlign: center` (the Windows 11 taskbar) | `"auto"` (hidden while the taskbar is up) |
//! | `caelestia` | Caelestia | `style: mac`, theme-driven roundness (pill) | `true` (a floating pill, `bar_style`) |
//!
//! The pure parts ([`selection_for`], [`dock_edits`], [`bar_value`], [`detect`]) are shared by `macthemectl style`
//! and the Theme pane; [`apply`] is the CLI's transaction.

use super::live::Source;
use super::model::Selection;
use crate::config::DockStyle;
use crate::settings::io::{self, Paths};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Macos,
    Windows,
    Caelestia,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Macos, Style::Windows, Style::Caelestia];

    pub fn parse(s: &str) -> Option<Style> {
        match s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "macos" | "mac" | "osx" => Some(Style::Macos),
            "windows" | "windows11" | "win" | "win11" => Some(Style::Windows),
            "caelestia" | "material" | "materialyou" => Some(Style::Caelestia),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Style::Macos => "macos",
            Style::Windows => "windows",
            Style::Caelestia => "caelestia",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Style::Macos => "macOS",
            Style::Windows => "Windows",
            Style::Caelestia => "Caelestia",
        }
    }
    /// Id of the built-in theme the style uses.
    pub fn theme_id(self) -> &'static str {
        match self {
            Style::Macos => "macos",
            Style::Windows => "windows11",
            Style::Caelestia => "caelestia",
        }
    }
}

/// The selection after choosing `style`: its theme, every override kept, the wallpaper colours kept only for Caelestia.
pub fn selection_for(style: Style, sel: &Selection) -> Selection {
    let mut s = sel.clone();
    s.name = style.theme_id().into();
    if style != Style::Caelestia {
        s.dynamic = None;
    }
    s
}

/// `dock-settings.json` keys to set (`Some`) or remove (`None`).
pub fn dock_edits(style: Style) -> Vec<(&'static str, Option<Value>)> {
    match style {
        Style::Windows => vec![("style", Some(json!("windows"))), ("windowsAlign", Some(json!("center")))],
        // `roundness` is dropped so the theme's `dock_roundness` (the pill of Caelestia) decides
        Style::Macos | Style::Caelestia => vec![("style", Some(json!("mac"))), ("roundness", None)],
    }
}

/// `macshell.json` `bar` object with `enabled` set for `style`; other keys of `bar` are kept.
pub fn bar_value(style: Style, current: Option<&Value>) -> Value {
    let mut bar = current.and_then(Value::as_object).cloned().unwrap_or_default();
    bar.insert("enabled".into(), if style == Style::Windows { json!("auto") } else { json!(true) });
    Value::Object(bar)
}

/// Which preset the current files describe; `None` for anything custom (another theme, a mixed state).
pub fn detect(theme_id: &str, dock: DockStyle) -> Option<Style> {
    match (dock, theme_id) {
        (DockStyle::Windows, "windows11") => Some(Style::Windows),
        (DockStyle::Mac, "caelestia") => Some(Style::Caelestia),
        (DockStyle::Mac, "macos") => Some(Style::Macos),
        _ => None,
    }
}

/// The dock style written in `dock-settings.json` (as macdock reads it).
pub fn dock_style(doc: &Value) -> DockStyle {
    doc.get("style").and_then(|v| serde_json::from_value::<DockStyle>(v.clone()).ok()).unwrap_or_default()
}

/// What `macthemectl style` prints and the pane shows.
pub fn current(src: &Source, paths: &Paths) -> Option<Style> {
    let sel = src.selection();
    let id = src.find(&sel.name).map(|f| f.0).unwrap_or_default();
    detect(&id, dock_style(&io::json_load(&paths.dock_settings())))
}

/// The CLI transaction: theme (and rdwm's colours) first, which validates everything, then the dock and bar files.
/// Running programs follow within half a second; nothing is restarted.
pub fn apply(src: &Source, paths: &Paths, style: Style, sel: &Selection) -> Result<super::rdwm::Report, String> {
    let new = selection_for(style, sel);
    let report = super::rdwm::apply(src, paths, &new)?;
    for (key, val) in dock_edits(style) {
        io::json_set(&paths.dock_settings(), key, val)?;
    }
    let bar = bar_value(style, io::json_load(&paths.shell_json()).get("bar"));
    io::json_set(&paths.shell_json(), "bar", Some(bar))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::material::Variant;
    use crate::theme::model::{Dynamic, Mode, Rgb};

    fn sandbox(name: &str) -> (std::path::PathBuf, Source, Paths) {
        let d = std::env::temp_dir().join(format!("macshell-style-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: d.join("themes") };
        (d, src, paths)
    }

    #[test]
    fn names_parse() {
        assert_eq!(Style::parse("macOS"), Some(Style::Macos));
        assert_eq!(Style::parse("Windows"), Some(Style::Windows));
        assert_eq!(Style::parse("windows-11"), Some(Style::Windows));
        assert_eq!(Style::parse("Caelestia"), Some(Style::Caelestia));
        assert_eq!(Style::parse("nope"), None);
        for s in Style::ALL {
            assert_eq!(Style::parse(s.as_str()), Some(s));
            assert!(crate::theme::builtin::source(s.theme_id()).is_some());
        }
    }

    #[test]
    fn each_style_sets_theme_dock_and_bar() {
        let (_d, src, paths) = sandbox("each");
        std::fs::write(paths.dock_settings(), r#"{"magnification": 1.8, "roundness": 0.2, "style": "windows"}"#).unwrap();
        std::fs::write(paths.shell_json(), r#"{"workspaces": "off", "bar": {"enabled": "auto", "x": 1}}"#).unwrap();
        let sel = Selection::default();

        apply(&src, &paths, Style::Caelestia, &Selection { sync_rdwm: false, ..sel.clone() }).unwrap();
        assert_eq!(src.selection().name, "caelestia");
        let dock = io::json_load(&paths.dock_settings());
        assert_eq!(dock["style"], "mac");
        assert!(dock.get("roundness").is_none(), "the pill comes from the theme");
        assert_eq!(dock["magnification"], 1.8, "other dock keys are kept");
        let shell = io::json_load(&paths.shell_json());
        assert_eq!(shell["bar"]["enabled"], true);
        assert_eq!(shell["bar"]["x"], 1, "other bar keys are kept");
        assert_eq!(shell["workspaces"], "off");
        assert_eq!(current(&src, &paths), Some(Style::Caelestia));

        apply(&src, &paths, Style::Windows, &Selection { sync_rdwm: false, ..src.selection() }).unwrap();
        assert_eq!(src.selection().name, "windows11");
        let dock = io::json_load(&paths.dock_settings());
        assert_eq!((dock["style"].as_str(), dock["windowsAlign"].as_str()), (Some("windows"), Some("center")));
        assert_eq!(io::json_load(&paths.shell_json())["bar"]["enabled"], "auto");
        assert_eq!(current(&src, &paths), Some(Style::Windows));

        apply(&src, &paths, Style::Macos, &Selection { sync_rdwm: false, ..src.selection() }).unwrap();
        assert_eq!(src.selection().name, "macos");
        assert_eq!(io::json_load(&paths.dock_settings())["style"], "mac");
        assert_eq!(io::json_load(&paths.shell_json())["bar"]["enabled"], true);
        assert_eq!(current(&src, &paths), Some(Style::Macos));
    }

    #[test]
    fn the_bar_follows_the_dock_style_under_auto() {
        let shell: crate::config::ShellSettings = serde_json::from_value(json!({"bar": bar_value(Style::Windows, None)})).unwrap();
        assert!(!shell.bar_visible(DockStyle::Windows) && shell.bar_visible(DockStyle::Mac));
        let shell: crate::config::ShellSettings = serde_json::from_value(json!({"bar": bar_value(Style::Caelestia, None)})).unwrap();
        assert!(shell.bar_visible(DockStyle::Mac) && shell.bar_visible(DockStyle::Windows));
    }

    #[test]
    fn overrides_survive_and_wallpaper_colours_belong_to_caelestia() {
        let dynamic = Dynamic { seed: Rgb(1, 2, 3), variant: Variant::Vibrant, wallpaper: None, follow: true };
        let sel = Selection { accent: Some(Rgb(9, 9, 9)), mode: Mode::Light, dynamic: Some(dynamic.clone()), ..Selection::default() };
        let c = selection_for(Style::Caelestia, &sel);
        assert_eq!((c.name.as_str(), c.accent, c.mode, c.dynamic.as_ref()), ("caelestia", Some(Rgb(9, 9, 9)), Mode::Light, Some(&dynamic)));
        assert!(selection_for(Style::Windows, &sel).dynamic.is_none());
        assert!(selection_for(Style::Macos, &sel).dynamic.is_none());
    }

    #[test]
    fn detection_needs_theme_and_dock_to_agree() {
        assert_eq!(detect("windows11", DockStyle::Windows), Some(Style::Windows));
        assert_eq!(detect("windows11", DockStyle::Mac), None);
        assert_eq!(detect("nord", DockStyle::Mac), None);
        assert_eq!(detect("caelestia", DockStyle::Windows), None);
        assert_eq!(dock_style(&json!({"style": "taskbar"})), DockStyle::Windows);
        assert_eq!(dock_style(&json!({})), DockStyle::Mac);
    }

    #[test]
    fn a_failing_theme_apply_leaves_the_dock_alone() {
        let (_d, src, paths) = sandbox("fail");
        std::fs::write(&paths.rdwm_config, "this is [not toml").unwrap();
        assert!(apply(&src, &paths, Style::Windows, &Selection::default()).is_err());
        assert!(!paths.dock_settings().exists() && !paths.shell_json().exists());
    }
}
