//! Reads the same JSON files the original dock uses, with safe defaults when they are missing.

use serde::Deserialize;
use std::path::PathBuf;

/// Which look the dock has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockStyle {
    /// The floating, magnifying macOS dock.
    #[default]
    #[serde(alias = "macos", alias = "Mac", alias = "macOS")]
    Mac,
    /// A full-width Windows 10/11 style taskbar.
    #[serde(alias = "Windows", alias = "win", alias = "taskbar")]
    Windows,
}

impl DockStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            DockStyle::Mac => "mac",
            DockStyle::Windows => "windows",
        }
    }
}

/// Where the taskbar's icons sit (Windows 11 centres them, Windows 10 and classic shells go left).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WinAlign {
    #[default]
    Center,
    Left,
}

impl WinAlign {
    pub fn as_str(self) -> &'static str {
        match self {
            WinAlign::Center => "center",
            WinAlign::Left => "left",
        }
    }
}

/// Window titles next to the taskbar icons. `Always` is "never combine": one labelled button per window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WinLabels {
    #[default]
    Never,
    Always,
}

/// The taskbar's search entry next to the Start button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WinSearch {
    None,
    Button,
    #[default]
    Box,
}

/// The taskbar's old edge key (`windowsPosition`): top or bottom (unknown words give the bottom). The universal
/// `edge` key of the same file wins over it and also offers left and right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WinPosition {
    #[default]
    Bottom,
    Top,
}

/// Taskbar colour scheme: follow the system (`auto`), or force one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WinTheme {
    #[default]
    Auto,
    Dark,
    Light,
}

/// `dock-settings.json`. Keys are camelCase; the snake_case spellings (`windows_align`) are accepted too.
/// Unknown values of the Windows options fall back to their defaults instead of voiding the file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DockSettings {
    pub app_spacing: f32,
    pub magnification: f32,
    pub transparency: f32,
    pub roundness: f32,
    /// `"mac"` (default) or `"windows"`.
    #[serde(deserialize_with = "lenient")]
    pub style: DockStyle,
    /// `"center"` (default) or `"left"`; Windows style only.
    #[serde(alias = "windows_align", deserialize_with = "lenient")]
    pub windows_align: WinAlign,
    /// `"never"` (default, icons only) or `"always"` (a titled button per window).
    #[serde(alias = "windows_labels", deserialize_with = "lenient")]
    pub windows_labels: WinLabels,
    /// `"box"` (default), `"button"` or `"none"`.
    #[serde(alias = "windows_search", deserialize_with = "lenient")]
    pub windows_search: WinSearch,
    /// `"bottom"` (default) or `"top"`.
    #[serde(alias = "windows_position", deserialize_with = "lenient")]
    pub windows_position: WinPosition,
    /// `"auto"` (default, follows the system theme), `"dark"` or `"light"`.
    #[serde(alias = "windows_theme", deserialize_with = "lenient")]
    pub windows_theme: WinTheme,
    /// The user's screen edge for the dock or the taskbar (`top`, `bottom`, `left`, `right`); `None` = the theme's
    /// `[dock] position`, else `windowsPosition`, else the bottom. See [`crate::panel`].
    #[serde(deserialize_with = "lenient_edge")]
    pub edge: Option<crate::panel::Edge>,
}

fn lenient_edge<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<crate::panel::Edge>, D::Error> {
    Ok(serde_json::Value::deserialize(d)?.as_str().and_then(crate::panel::Edge::parse))
}

/// A value that falls back to the type's default when it is not one of the known words.
fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(T::deserialize(serde_json::Value::deserialize(d)?).unwrap_or_default())
}

impl Default for DockSettings {
    fn default() -> Self {
        Self {
            app_spacing: 6.0,
            magnification: 1.4,
            transparency: 0.4,
            // roundness follows the theme until dock-settings.json sets it
            roundness: crate::theme::th().dock_roundness,
            style: DockStyle::Mac,
            windows_align: WinAlign::Center,
            windows_labels: WinLabels::Never,
            windows_search: WinSearch::Box,
            windows_position: WinPosition::Bottom,
            windows_theme: WinTheme::Auto,
            edge: None,
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
        let mut out: Vec<String> = self.order.iter().filter(|id| self.pinned.contains(id)).cloned().collect();
        for id in &self.pinned {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }
}

/// Whether a feature is on, off, or follows the environment. In JSON: `true`, `false` or `"auto"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Toggle {
    Flag(bool),
    Word(Word),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Word {
    Auto,
    On,
    Off,
}

impl Toggle {
    /// `auto_on` is what "auto" resolves to in the current environment.
    pub fn resolve(self, auto_on: bool) -> bool {
        match self {
            Toggle::Flag(b) => b,
            Toggle::Word(Word::On) => true,
            Toggle::Word(Word::Off) => false,
            Toggle::Word(Word::Auto) => auto_on,
        }
    }
}

/// `decebal.json`: shell-wide switches.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShellSettings {
    /// Workspace/tag dots in the menu bar. "auto" = only under rdwm.
    pub workspaces: Toggle,
    pub bar: BarSettings,
    pub dock: DockGateSettings,
    /// Menu bar clock: 12-hour format (decsettings: Date & Time).
    pub clock_hour12: bool,
    /// `weekday-day-month` (default), `day-month`, `weekday-day`, `none`.
    pub clock_date: String,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self { workspaces: Toggle::Word(Word::Auto), bar: BarSettings::default(), dock: DockGateSettings::default(), clock_hour12: false, clock_date: "weekday-day-month".into() }
    }
}

/// Where the Caelestia pill bar sits (`decebal.json` -> `"bar": { "position": "left" }`). Only the Caelestia theme
/// (`bar_style = "vertical"`) draws that bar; every other theme keeps the menu bar across the top and ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarPosition {
    /// A vertical bar on the left screen edge (Caelestia's own layout).
    #[default]
    Left,
    /// A vertical bar on the right edge; popouts open leftwards.
    Right,
    /// A horizontal pill bar along the top edge.
    Top,
    /// A horizontal pill bar along the bottom edge; popouts open upwards.
    Bottom,
    /// Both: the top bar holds the OS icon, workspaces and window title, the bottom bar the tray, status icons, clock and power.
    Split,
}

impl BarPosition {
    pub const ALL: [BarPosition; 5] = [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom, BarPosition::Split];

    pub fn parse(s: &str) -> Option<BarPosition> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" => Some(BarPosition::Left),
            "right" => Some(BarPosition::Right),
            "top" => Some(BarPosition::Top),
            "bottom" => Some(BarPosition::Bottom),
            "split" | "both" | "top-bottom" | "top+bottom" => Some(BarPosition::Split),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            BarPosition::Left => "left",
            BarPosition::Right => "right",
            BarPosition::Top => "top",
            BarPosition::Bottom => "bottom",
            BarPosition::Split => "split",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            BarPosition::Left => "Left",
            BarPosition::Right => "Right",
            BarPosition::Top => "Top",
            BarPosition::Bottom => "Bottom",
            BarPosition::Split => "Split",
        }
    }

    /// The bar runs down a side edge.
    pub fn is_vertical(self) -> bool {
        matches!(self, BarPosition::Left | BarPosition::Right)
    }
}

impl<'de> Deserialize<'de> for BarPosition {
    /// Lenient: a misspelt value (or a number) must not make the whole `decebal.json` unreadable, it means the default.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(v.as_str().and_then(BarPosition::parse).unwrap_or_default())
    }
}

/// `decebal.json` -> `"bar": { "enabled": true | false | "auto", "position": "left" | ... }`: whether decbar shows at all,
/// and where the Caelestia pill bar sits. `enabled = "auto"` hides it while the dock is in the Windows style.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct BarSettings {
    pub enabled: Toggle,
    pub position: BarPosition,
}

impl Default for BarSettings {
    fn default() -> Self {
        Self { enabled: Toggle::Flag(true), position: BarPosition::Left }
    }
}

/// Native Dux bar item IDs. Lists are declarative; no command or widget execution.
pub const DUX_BAR_ITEMS: &[&str] = &[
    "launcherButton",
    "workspaceSwitcher",
    "focusedWindow",
    "music",
    "clock",
    "weather",
    "systemTray",
    "clipboard",
    "notificationButton",
    "battery",
    "controlCenterButton",
    "network",
    "audio",
    "bluetooth",
    "dashboard",
    "power",
    "layout",
    "cpuUsage",
    "memUsage",
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct BarLayout {
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    pub margin: f32,
    pub spacing: f32,
    pub radius: f32,
    pub background: String,
    pub band_radius: f32,
}
impl Default for BarLayout {
    fn default() -> Self {
        let names = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect();
        Self {
            left: names(&["launcherButton", "workspaceSwitcher", "focusedWindow"]),
            center: names(&["music", "clock", "weather"]),
            right: names(&["systemTray", "clipboard", "cpuUsage", "memUsage", "notificationButton", "battery", "controlCenterButton"]),
            margin: 4.0,
            spacing: 4.0,
            radius: 14.0,
            background: "band".into(),
            band_radius: 16.0,
        }
    }
}
impl BarLayout {
    pub fn from_json(v: &serde_json::Value) -> Self {
        let mut out: Self = serde_json::from_value(v.clone()).unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        for group in [&mut out.left, &mut out.center, &mut out.right] {
            group.retain(|id| DUX_BAR_ITEMS.contains(&id.as_str()) && seen.insert(id.clone()));
            group.truncate(24);
        }
        out.margin = out.margin.clamp(0.0, 48.0);
        out.spacing = out.spacing.clamp(0.0, 24.0);
        out.radius = out.radius.clamp(0.0, 48.0);
        out.band_radius = out.band_radius.clamp(0.0, 48.0);
        if !matches!(out.background.as_str(), "band" | "islands") {
            out.background = "band".into();
        }
        out
    }
}

/// `decebal.json` -> `"dock": { "enabled": true | false | "auto" }`: whether decdock shows at all. `"auto"` hides it while the
/// theme's bar is the vertical one (`bar_style = "vertical"`: Caelestia has no dock). Default `true`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct DockGateSettings {
    pub enabled: Toggle,
}

impl Default for DockGateSettings {
    fn default() -> Self {
        Self { enabled: Toggle::Flag(true) }
    }
}

impl ShellSettings {
    /// Whether the macOS menu bar should be on screen given the dock's style.
    pub fn bar_visible(&self, dock: DockStyle) -> bool {
        self.bar.enabled.resolve(dock != DockStyle::Windows)
    }

    /// Whether the dock should be on screen given whether the current theme has a vertical bar.
    pub fn dock_visible(&self, vertical_bar: bool) -> bool {
        self.dock.enabled.resolve(!vertical_bar)
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
    std::env::var_os("DECEBAL_CONFIG")
        .map(PathBuf::from)
        // Unit tests never read the developer's own config (their theme, fonts, bar position): an empty folder, unless a test sets DECEBAL_CONFIG.
        .or_else(|| cfg!(test).then(|| std::env::temp_dir().join(format!("decebal-test-config-{}", std::process::id()))))
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

pub fn shell_settings() -> ShellSettings {
    load("decebal.json")
}

pub fn pinned() -> Pinned {
    load("dock-pinned-macos.json")
}

/// File name of the pinned list inside [`config_dir`].
pub const PINNED_FILE: &str = "dock-pinned-macos.json";
/// File name of the dock settings inside [`config_dir`].
pub const SETTINGS_FILE: &str = "dock-settings.json";

/// Cheap change detector for a config file: modification time and length (`None` = missing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp(Option<(std::time::SystemTime, u64)>);

impl Stamp {
    pub fn of(path: &std::path::Path) -> Stamp {
        Stamp(std::fs::metadata(path).ok().and_then(|m| m.modified().ok().map(|t| (t, m.len()))))
    }
}

/// Writes the pinned list (both `pinned` and `order`) to `path`, keeping any other keys of an
/// existing file. The write is atomic (temporary file in the same directory, then rename) so
/// a reader never sees a half-written file.
pub fn write_pinned(path: &std::path::Path, list: &[String]) -> std::io::Result<()> {
    let mut doc: serde_json::Value =
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({ "version": 1 }));
    doc["pinned"] = serde_json::json!(list);
    doc["order"] = serde_json::json!(list);
    let mut text = serde_json::to_string_pretty(&doc).map_err(std::io::Error::other)?;
    text.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Sets one key of the JSON object in `path` (creating the file), keeping every other key. Atomic like
/// [`write_pinned`].
pub fn write_setting(path: &std::path::Path, key: &str, value: serde_json::Value) -> std::io::Result<()> {
    let mut doc: serde_json::Value = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
    doc[key] = value;
    let mut text = serde_json::to_string_pretty(&doc).map_err(std::io::Error::other)?;
    text.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Pure list edits behind the dock menu and drag-and-drop.
pub mod edit {
    /// Adds `id` (before the trash when that is last); no-op when already present.
    pub fn add(list: &mut Vec<String>, id: &str) {
        if list.iter().any(|x| x == id) {
            return;
        }
        match list.last() {
            Some(last) if last == "trash" => list.insert(list.len() - 1, id.to_string()),
            _ => list.push(id.to_string()),
        }
    }

    /// Removes every entry equal to `id`; true when something was removed.
    pub fn remove(list: &mut Vec<String>, id: &str) -> bool {
        let before = list.len();
        list.retain(|x| x != id);
        list.len() != before
    }

    /// Moves the entry at `from` so it ends up at index `to` (both clamped); true when the order changed.
    pub fn move_to(list: &mut Vec<String>, from: usize, to: usize) -> bool {
        if from >= list.len() {
            return false;
        }
        let to = to.min(list.len() - 1);
        if from == to {
            return false;
        }
        let item = list.remove(from);
        list.insert(to, item);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::edit::add;
    use super::*;

    #[test]
    fn defaults_match_original() {
        let s = DockSettings::default();
        assert_eq!((s.app_spacing, s.magnification, s.transparency, s.roundness), (6.0, 1.4, 0.4, crate::theme::th().dock_roundness));
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
    fn dux_bar_contract_is_bounded_declarative_and_keeps_explicit_empty_slots() {
        let layout = BarLayout::from_json(
            &serde_json::json!({"left": [], "center": ["clock", "clock", "exec:sh"], "right": ["clock", "notificationButton", "widget:unknown"], "margin": 999, "spacing": -1, "radius": 999}),
        );
        assert!(layout.left.is_empty());
        assert_eq!(layout.center, ["clock"]);
        assert_eq!(layout.right, ["notificationButton"]);
        assert_eq!((layout.margin, layout.spacing, layout.radius), (48.0, 0.0, 48.0));
        let default = BarLayout::default();
        assert_eq!(default.left, ["launcherButton", "workspaceSwitcher", "focusedWindow"]);
        assert!(default.right.iter().any(|id| id == "cpuUsage"));
    }

    #[test]
    fn bar_position_parses_every_spelling_and_defaults_to_left() {
        for p in BarPosition::ALL {
            assert_eq!(BarPosition::parse(p.as_str()), Some(p));
            assert!(!p.title().is_empty());
        }
        assert_eq!(BarPosition::parse(" Top "), Some(BarPosition::Top));
        assert_eq!(BarPosition::parse("both"), Some(BarPosition::Split));
        assert_eq!(BarPosition::parse("diagonal"), None);
        assert!(BarPosition::Left.is_vertical() && BarPosition::Right.is_vertical());
        assert!(!BarPosition::Top.is_vertical() && !BarPosition::Bottom.is_vertical() && !BarPosition::Split.is_vertical());
        assert_eq!(ShellSettings::default().bar.position, BarPosition::Left);
    }

    #[test]
    fn bar_position_round_trips_through_decebal_json() {
        for p in BarPosition::ALL {
            let s: ShellSettings = serde_json::from_str(&format!(r#"{{"bar": {{"enabled": true, "position": "{}"}}, "dock": {{"enabled": "auto"}}}}"#, p.as_str())).unwrap();
            assert_eq!(s.bar.position, p);
            assert_eq!(s.bar.enabled, Toggle::Flag(true));
        }
        // a missing key, a misspelt value and a wrong type all mean left, and never spoil the other keys
        for text in [r#"{"bar": {"enabled": false}}"#, r#"{"bar": {"enabled": false, "position": "sideways"}}"#, r#"{"bar": {"enabled": false, "position": 3}}"#] {
            let s: ShellSettings = serde_json::from_str(text).unwrap();
            assert_eq!((s.bar.position, s.bar.enabled), (BarPosition::Left, Toggle::Flag(false)), "{text}");
        }
    }

    #[test]
    fn order_skips_unknown_and_appends_missing() {
        let p = Pinned { pinned: vec!["a".into(), "b".into(), "c".into()], order: vec!["c".into(), "x".into(), "a".into()] };
        assert_eq!(p.ordered(), vec!["c", "a", "b"]);
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decebal-cfg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn list(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn write_pinned_is_atomic_and_keeps_other_keys() {
        let d = tmp_dir("write");
        let f = d.join(PINNED_FILE);
        std::fs::write(&f, r#"{"version": 1, "extra": {"a": 1}, "pinned": ["x"], "order": ["x"]}"#).unwrap();
        write_pinned(&f, &list(&["launchpad", "foot", "trash"])).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["extra"]["a"], 1);
        assert_eq!(v["pinned"], serde_json::json!(["launchpad", "foot", "trash"]));
        assert_eq!(v["order"], v["pinned"]);
        // no temporary files are left behind
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let p: Pinned = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(p.ordered(), list(&["launchpad", "foot", "trash"]));
    }

    #[test]
    fn write_pinned_creates_missing_file_and_replaces_garbage() {
        let d = tmp_dir("create");
        let f = d.join("sub/dock-pinned-macos.json");
        write_pinned(&f, &list(&["a"])).unwrap();
        assert!(f.is_file());
        std::fs::write(&f, "not json").unwrap();
        write_pinned(&f, &list(&["b"])).unwrap();
        let p: Pinned = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(p.ordered(), list(&["b"]));
    }

    #[test]
    fn stamp_changes_with_the_file() {
        let d = tmp_dir("stamp");
        let f = d.join("x.json");
        let missing = Stamp::of(&f);
        std::fs::write(&f, "{}").unwrap();
        let one = Stamp::of(&f);
        assert_ne!(missing, one);
        assert_eq!(one, Stamp::of(&f));
        std::fs::write(&f, "{ }").unwrap();
        assert_ne!(one, Stamp::of(&f));
    }

    #[test]
    fn list_edits() {
        let mut l = list(&["launchpad", "a", "trash"]);
        add(&mut l, "b");
        assert_eq!(l, list(&["launchpad", "a", "b", "trash"]));
        add(&mut l, "b");
        assert_eq!(l.len(), 4, "no duplicates");
        assert!(edit::remove(&mut l, "a"));
        assert!(!edit::remove(&mut l, "a"));
        assert_eq!(l, list(&["launchpad", "b", "trash"]));
        assert!(edit::move_to(&mut l, 2, 1));
        assert_eq!(l, list(&["launchpad", "trash", "b"]));
        assert!(!edit::move_to(&mut l, 1, 1));
        assert!(!edit::move_to(&mut l, 9, 0));
        assert!(edit::move_to(&mut l, 0, 99), "target clamps to the end");
        assert_eq!(l, list(&["trash", "b", "launchpad"]));
        let mut empty: Vec<String> = vec![];
        add(&mut empty, "z");
        assert_eq!(empty, list(&["z"]));
    }

    #[test]
    fn style_defaults_to_mac_and_old_files_still_parse() {
        let s: DockSettings = serde_json::from_str(r#"{"appSpacing": 8, "magnification": 1.2, "transparency": 0.1, "roundness": 0.3}"#).unwrap();
        assert_eq!((s.style, s.windows_align, s.windows_labels, s.windows_search), (DockStyle::Mac, WinAlign::Center, WinLabels::Never, WinSearch::Box));
        assert_eq!(s.app_spacing, 8.0);
        assert_eq!(DockSettings::default().style, DockStyle::Mac);
    }

    #[test]
    fn windows_options_parse_in_both_spellings() {
        let s: DockSettings =
            serde_json::from_str(r#"{"style":"windows","windowsAlign":"left","windows_labels":"always","windowsSearch":"button","windowsPosition":"top","windowsTheme":"light"}"#).unwrap();
        assert_eq!(s.style, DockStyle::Windows);
        assert_eq!(
            (s.windows_align, s.windows_labels, s.windows_search, s.windows_position, s.windows_theme),
            (WinAlign::Left, WinLabels::Always, WinSearch::Button, WinPosition::Top, WinTheme::Light)
        );
    }

    #[test]
    fn unknown_option_words_fall_back_without_voiding_the_file() {
        let s: DockSettings = serde_json::from_str(r#"{"style":"beos","windowsAlign":"right","magnification":2.0,"windowsLabels":3}"#).unwrap();
        assert_eq!((s.style, s.windows_align, s.windows_labels), (DockStyle::Mac, WinAlign::Center, WinLabels::Never));
        assert_eq!(s.magnification, 2.0, "the other settings survive");
        let s: DockSettings = serde_json::from_str(r#"{"style":"macos"}"#).unwrap();
        assert_eq!(s.style, DockStyle::Mac);
        let s: DockSettings = serde_json::from_str(r#"{"windowsPosition":"left"}"#).unwrap();
        assert_eq!(s.windows_position, WinPosition::Bottom);
    }

    #[test]
    fn style_round_trips_through_write_setting() {
        let d = tmp_dir("style");
        let f = d.join(SETTINGS_FILE);
        std::fs::write(&f, r#"{"appSpacing": 9, "custom": [1]}"#).unwrap();
        write_setting(&f, "style", serde_json::json!(DockStyle::Windows.as_str())).unwrap();
        write_setting(&f, "windowsAlign", serde_json::json!("left")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["custom"], serde_json::json!([1]));
        let s: DockSettings = serde_json::from_value(v).unwrap();
        assert_eq!((s.style, s.windows_align, s.app_spacing), (DockStyle::Windows, WinAlign::Left, 9.0));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
    }

    #[test]
    fn dock_enabled_defaults_on_and_auto_follows_the_vertical_bar() {
        let parse = |s: &str| serde_json::from_str::<ShellSettings>(s).unwrap();
        assert!(parse("{}").dock_visible(false) && parse("{}").dock_visible(true), "on by default, whatever the theme");
        assert!(!parse(r#"{"dock":{"enabled":false}}"#).dock_visible(false));
        let auto = parse(r#"{"dock":{"enabled":"auto"}}"#);
        assert!(auto.dock_visible(false) && !auto.dock_visible(true), "hidden while the theme has a vertical bar");
        assert!(parse(r#"{"dock":{"enabled":true}}"#).dock_visible(true));
        assert!(parse(r#"{"dock":{}}"#).dock_visible(true), "a missing key keeps the dock");
    }

    #[test]
    fn bar_enabled_defaults_on_and_auto_follows_the_dock_style() {
        let parse = |j: &str| serde_json::from_str::<ShellSettings>(j).unwrap();
        assert!(parse("{}").bar_visible(DockStyle::Windows));
        assert!(!parse(r#"{"bar":{"enabled":false}}"#).bar_visible(DockStyle::Mac));
        let auto = parse(r#"{"bar":{"enabled":"auto"}}"#);
        assert!(auto.bar_visible(DockStyle::Mac) && !auto.bar_visible(DockStyle::Windows));
        assert!(parse(r#"{"workspaces":"on","bar":{}}"#).bar_visible(DockStyle::Mac));
    }

    #[test]
    fn workspaces_default_to_auto() {
        let s = ShellSettings::default();
        assert!(s.workspaces.resolve(true));
        assert!(!s.workspaces.resolve(false));
    }

    #[test]
    fn toggle_accepts_bools_and_words() {
        let parse = |j: &str| serde_json::from_str::<ShellSettings>(j).unwrap().workspaces;
        assert!(parse(r#"{"workspaces": true}"#).resolve(false));
        assert!(!parse(r#"{"workspaces": false}"#).resolve(true));
        assert!(parse(r#"{"workspaces": "on"}"#).resolve(false));
        assert!(!parse(r#"{"workspaces": "off"}"#).resolve(true));
        assert!(parse(r#"{"workspaces": "auto"}"#).resolve(true));
        assert!(parse("{}").resolve(true));
    }
}
