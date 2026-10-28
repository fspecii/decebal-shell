//! Shell style presets: one word that sets the theme, the dock style and the menu bar coherently.
//!
//! | style | theme | dock (`dock-settings.json`) | dock shown (`decebal.json` `dock.enabled`) | menu bar (`decebal.json` `bar.enabled`) |
//! |---|---|---|---|---|
//! | `macos` | macOS | `style: mac`, theme-driven roundness | `true` | `true` |
//! | `windows` | Windows 11 | `style: windows`, `windowsAlign: center` (the Windows 11 taskbar) | `true` | `"auto"` (hidden while the taskbar is up) |
//! | `caelestia` | Caelestia | `style: mac`, theme-driven roundness (pill) | `"auto"`: hidden, Caelestia has no dock | `true`, drawn as the pill bar (`bar_style = "vertical"`) on the edge `bar.position` says (left, right, top, bottom or split; kept, `caelestia-top` sets it) |
//!
//! The pure parts ([`selection_for`], [`dock_edits`], [`bar_value`], [`dock_value`], [`detect`]) are shared by `decthemectl style`
//! and the Theme pane; [`apply`] is the CLI's transaction. [`parse_with_layout`] reads `caelestia-top` and the like, [`position_value`] writes `bar.position`.

use super::live::Source;
use super::model::Selection;
use crate::config::{BarPosition, DockStyle};
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

/// A style word that may carry a Caelestia layout: `caelestia`, `caelestia-top`, `caelestia_split`, `caelestia bottom`.
/// Only Caelestia has layouts, so `macos-top` is not a style.
pub fn parse_with_layout(s: &str) -> Option<(Style, Option<BarPosition>)> {
    if let Some(st) = Style::parse(s) {
        return Some((st, None));
    }
    let norm = s.trim().to_ascii_lowercase();
    let (head, tail) = norm.rsplit_once(['-', '_', ' '])?;
    let (st, pos) = (Style::parse(head)?, BarPosition::parse(tail)?);
    (st == Style::Caelestia).then_some((st, Some(pos)))
}

/// `decebal.json` `bar` object with `position` set; other keys of `bar` (`enabled`) are kept.
pub fn position_value(pos: BarPosition, current: Option<&Value>) -> Value {
    let mut bar = current.and_then(Value::as_object).cloned().unwrap_or_default();
    bar.insert("position".into(), json!(pos.as_str()));
    Value::Object(bar)
}

/// The position written in a `decebal.json` document (left when absent).
pub fn position_of(shell: &Value) -> BarPosition {
    shell.get("bar").and_then(|b| b.get("position")).and_then(Value::as_str).and_then(BarPosition::parse).unwrap_or_default()
}

/// The selection after choosing `style`: its theme, every override kept (including "match wallpaper", which works with every style).
pub fn selection_for(style: Style, sel: &Selection) -> Selection {
    let mut s = sel.clone();
    s.name = style.theme_id().into();
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

/// `decebal.json` `bar` object with `enabled` set for `style`; other keys of `bar` are kept.
pub fn bar_value(style: Style, current: Option<&Value>) -> Value {
    let mut bar = current.and_then(Value::as_object).cloned().unwrap_or_default();
    bar.insert("enabled".into(), if style == Style::Windows { json!("auto") } else { json!(true) });
    Value::Object(bar)
}

/// `decebal.json` `dock` object with `enabled` set for `style`; other keys of `dock` are kept. Caelestia has no dock, so it
/// writes `"auto"` (the dock hides while the theme's bar is the vertical one and returns with any other theme); the other
/// styles write `true`, so going back brings the dock back.
pub fn dock_value(style: Style, current: Option<&Value>) -> Value {
    let mut dock = current.and_then(Value::as_object).cloned().unwrap_or_default();
    dock.insert("enabled".into(), if style == Style::Caelestia { json!("auto") } else { json!(true) });
    Value::Object(dock)
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

/// The dock style written in `dock-settings.json` (as decdock reads it).
pub fn dock_style(doc: &Value) -> DockStyle {
    doc.get("style").and_then(|v| serde_json::from_value::<DockStyle>(v.clone()).ok()).unwrap_or_default()
}

/// What `decthemectl style` prints and the pane shows.
pub fn current(src: &Source, paths: &Paths) -> Option<Style> {
    let sel = src.selection();
    let id = src.find(&sel.name).map(|f| f.0).unwrap_or_default();
    detect(&id, dock_style(&io::json_load(&paths.dock_settings())))
}

/// The CLI transaction: theme (and rdwm's colours) first, which validates everything, then the dock and bar files.
/// Running programs follow within half a second; nothing is restarted. The bar's position is kept.
pub fn apply(src: &Source, paths: &Paths, style: Style, sel: &Selection) -> Result<super::rdwm::Report, String> {
    apply_with(src, paths, style, sel, None)
}

/// [`apply`] that also sets `bar.position` (`decthemectl style caelestia-top`) when `position` is given.
pub fn apply_with(src: &Source, paths: &Paths, style: Style, sel: &Selection, position: Option<BarPosition>) -> Result<super::rdwm::Report, String> {
    let new = selection_for(style, sel);
    let report = super::rdwm::apply(src, paths, &new)?;
    for (key, val) in dock_edits(style) {
        io::json_set(&paths.dock_settings(), key, val)?;
    }
    let mut bar = bar_value(style, io::json_load(&paths.shell_json()).get("bar"));
    if let Some(p) = position {
        bar = position_value(p, Some(&bar));
    }
    io::json_set(&paths.shell_json(), "bar", Some(bar))?;
    let dock = dock_value(style, io::json_load(&paths.shell_json()).get("dock"));
    io::json_set(&paths.shell_json(), "dock", Some(dock))?;
    Ok(report)
}

/// `decthemectl layout`: only `bar.position`; the running bars re-lay out within about a second.
pub fn apply_layout(paths: &Paths, position: BarPosition) -> Result<(), String> {
    let bar = position_value(position, io::json_load(&paths.shell_json()).get("bar"));
    io::json_set(&paths.shell_json(), "bar", Some(bar))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::material::Variant;
    use crate::theme::model::{Dynamic, Mode, Rgb};

    fn sandbox(name: &str) -> (std::path::PathBuf, Source, Paths) {
        let d = std::env::temp_dir().join(format!("decebal-style-{name}-{}", std::process::id()));
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
        std::fs::write(paths.shell_json(), r#"{"workspaces": "off", "bar": {"enabled": "auto", "x": 1}, "dock": {"enabled": true, "y": 2}}"#).unwrap();
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
        assert_eq!(shell["dock"]["enabled"], "auto", "Caelestia has no dock");
        assert_eq!(shell["dock"]["y"], 2, "other dock keys are kept");
        assert_eq!(shell["workspaces"], "off");
        assert_eq!(current(&src, &paths), Some(Style::Caelestia));

        apply(&src, &paths, Style::Windows, &Selection { sync_rdwm: false, ..src.selection() }).unwrap();
        assert_eq!(src.selection().name, "windows11");
        let dock = io::json_load(&paths.dock_settings());
        assert_eq!((dock["style"].as_str(), dock["windowsAlign"].as_str()), (Some("windows"), Some("center")));
        assert_eq!(io::json_load(&paths.shell_json())["bar"]["enabled"], "auto");
        assert_eq!(io::json_load(&paths.shell_json())["dock"]["enabled"], true, "the dock comes back");
        assert_eq!(current(&src, &paths), Some(Style::Windows));

        apply(&src, &paths, Style::Macos, &Selection { sync_rdwm: false, ..src.selection() }).unwrap();
        assert_eq!(src.selection().name, "macos");
        assert_eq!(io::json_load(&paths.dock_settings())["style"], "mac");
        assert_eq!(io::json_load(&paths.shell_json())["bar"]["enabled"], true);
        assert_eq!(io::json_load(&paths.shell_json())["dock"]["enabled"], true);
        assert_eq!(current(&src, &paths), Some(Style::Macos));
    }

    #[test]
    fn the_dock_is_hidden_by_the_vertical_bar_theme_only_for_caelestia() {
        use crate::theme::builtin;
        let shell: crate::config::ShellSettings = serde_json::from_value(json!({"dock": dock_value(Style::Caelestia, None)})).unwrap();
        let vertical = |id: &str| builtin::def(id).unwrap().resolve(id, true, &Selection::default()).bar_vertical();
        assert!(vertical("caelestia") && !vertical("macos") && !vertical("windows11"));
        assert!(!shell.dock_visible(vertical("caelestia")), "Caelestia: no dock");
        assert!(shell.dock_visible(vertical("nord")), "auto brings the dock back with a flat-bar theme");
        for st in [Style::Macos, Style::Windows] {
            let s: crate::config::ShellSettings = serde_json::from_value(json!({"dock": dock_value(st, Some(&json!({"enabled": false})))})).unwrap();
            assert!(s.dock_visible(false), "{st:?} shows the dock again");
        }
    }

    #[test]
    fn the_bar_follows_the_dock_style_under_auto() {
        let shell: crate::config::ShellSettings = serde_json::from_value(json!({"bar": bar_value(Style::Windows, None)})).unwrap();
        assert!(!shell.bar_visible(DockStyle::Windows) && shell.bar_visible(DockStyle::Mac));
        let shell: crate::config::ShellSettings = serde_json::from_value(json!({"bar": bar_value(Style::Caelestia, None)})).unwrap();
        assert!(shell.bar_visible(DockStyle::Mac) && shell.bar_visible(DockStyle::Windows));
    }

    #[test]
    fn overrides_and_match_wallpaper_survive_every_style() {
        let dynamic = Dynamic { enabled: true, scheme: Variant::Vibrant, ..Dynamic::default() };
        let sel = Selection { accent: Some(Rgb(9, 9, 9)), mode: Mode::Light, dynamic: Some(dynamic.clone()), ..Selection::default() };
        for (style, id) in [(Style::Caelestia, "caelestia"), (Style::Windows, "windows11"), (Style::Macos, "macos")] {
            let c = selection_for(style, &sel);
            assert_eq!((c.name.as_str(), c.accent, c.mode, c.dynamic.as_ref()), (id, Some(Rgb(9, 9, 9)), Mode::Light, Some(&dynamic)));
        }
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
    fn style_words_may_name_a_caelestia_layout() {
        assert_eq!(parse_with_layout("caelestia"), Some((Style::Caelestia, None)));
        assert_eq!(parse_with_layout("macos"), Some((Style::Macos, None)));
        for p in BarPosition::ALL {
            assert_eq!(parse_with_layout(&format!("caelestia-{}", p.as_str())), Some((Style::Caelestia, Some(p))));
        }
        assert_eq!(parse_with_layout("Caelestia_Split"), Some((Style::Caelestia, Some(BarPosition::Split))));
        assert_eq!(parse_with_layout("caelestia top"), Some((Style::Caelestia, Some(BarPosition::Top))));
        assert_eq!(parse_with_layout("caelestia-diagonal"), None);
        assert_eq!(parse_with_layout("macos-top"), None, "only Caelestia has layouts");
        assert_eq!(parse_with_layout("windows-left"), None);
        assert_eq!(parse_with_layout("top"), None);
    }

    #[test]
    fn the_position_is_kept_by_the_style_and_set_by_layout_words() {
        let (_d, src, paths) = sandbox("pos");
        std::fs::write(paths.shell_json(), r#"{"bar": {"enabled": "auto", "x": 1, "position": "bottom"}, "dock": {"enabled": true}}"#).unwrap();
        let sel = Selection { sync_rdwm: false, ..Selection::default() };
        apply(&src, &paths, Style::Caelestia, &sel).unwrap();
        let shell = io::json_load(&paths.shell_json());
        assert_eq!(position_of(&shell), BarPosition::Bottom, "`style caelestia` keeps the position");
        assert_eq!((shell["bar"]["enabled"].as_bool(), shell["bar"]["x"].as_i64()), (Some(true), Some(1)));
        // a style word with a layout sets it, in the same transaction
        apply_with(&src, &paths, Style::Caelestia, &Selection { sync_rdwm: false, ..src.selection() }, Some(BarPosition::Split)).unwrap();
        assert_eq!(position_of(&io::json_load(&paths.shell_json())), BarPosition::Split);
        // macOS leaves the setting alone (it is inactive without the Caelestia theme) so coming back restores the layout
        apply(&src, &paths, Style::Macos, &Selection { sync_rdwm: false, ..src.selection() }).unwrap();
        assert_eq!(position_of(&io::json_load(&paths.shell_json())), BarPosition::Split);
        // `layout` writes only bar.position
        apply_layout(&paths, BarPosition::Right).unwrap();
        let shell = io::json_load(&paths.shell_json());
        assert_eq!((position_of(&shell), shell["bar"]["enabled"].as_bool(), shell["bar"]["x"].as_i64()), (BarPosition::Right, Some(true), Some(1)));
        assert_eq!(shell["dock"]["enabled"], true);
        assert_eq!(src.selection().name, "macos", "the theme is not touched");
        // from nothing at all
        let _ = std::fs::remove_file(paths.shell_json());
        apply_layout(&paths, BarPosition::Top).unwrap();
        assert_eq!(io::json_load(&paths.shell_json()), json!({"bar": {"position": "top"}}));
        assert_eq!(position_of(&json!({})), BarPosition::Left);
        assert_eq!(position_of(&json!({"bar": {"position": "nope"}})), BarPosition::Left);
    }

    #[test]
    fn a_failing_theme_apply_leaves_the_dock_alone() {
        let (_d, src, paths) = sandbox("fail");
        std::fs::write(&paths.rdwm_config, "this is [not toml").unwrap();
        assert!(apply(&src, &paths, Style::Windows, &Selection::default()).is_err());
        assert!(!paths.dock_settings().exists() && !paths.shell_json().exists());
    }
}
