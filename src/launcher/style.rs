//! The launcher style registry: every selectable launcher look, its stable key and what it can do.
//!
//! The user's choice (`decebal.json` -> `"launcher": {"style": "<key>"}`) is independent of the theme. Absent, empty,
//! `"theme"` or an unknown key means the theme's `launcher_style` token (`[launcher] style` in a manifest).

/// How a style places itself on the screen (see [`super::placement`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// A card centred in the space the panels leave free.
    Centered,
    /// A popup that opens from the panel's launcher applet, towards the screen.
    Anchored,
    /// The whole screen except the panel strut.
    FullScreen,
}

/// What a style offers; the Settings pane lists it and the tests check every renderer honours it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub placement: Placement,
    /// Freedesktop main categories (Development, Graphics, ...) as a filter.
    pub categories: bool,
    /// Pinned / favourite apps, toggled with a right click or Ctrl+P.
    pub favorites: bool,
    /// Recently used apps (the usage history Spotlight keeps too).
    pub recent: bool,
    /// Pages the grid sideways instead of scrolling it.
    pub paged: bool,
    /// A power button that opens the session menu (decsession).
    pub session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LauncherStyle {
    /// The Launchpad grid (the original look).
    Default,
    /// The Dux applications list.
    Dux,
    /// Windows 11 Start menu.
    Windows,
    /// GNOME Shell's Activities overview with the app grid.
    Gnome,
    /// KDE Plasma's Kickoff.
    Kde,
    /// Xfce's Whisker Menu.
    Xfce,
    /// A BlackBerry 10-inspired hub and app grid.
    BlackBerry,
}

/// Every style in the order Settings lists them.
pub const ALL: [LauncherStyle; 7] = [LauncherStyle::Default, LauncherStyle::Dux, LauncherStyle::Windows, LauncherStyle::Gnome, LauncherStyle::Kde, LauncherStyle::Xfce, LauncherStyle::BlackBerry];

/// The stable keys, for the theme token / manifest choice lists (old manifests use the first two only).
pub const KEYS: &[&str] = &["default", "dux", "windows", "gnome", "kde", "xfce", "blackberry"];

/// The `decebal.json` value that defers to the theme.
pub const THEME: &str = "theme";

impl LauncherStyle {
    pub fn key(self) -> &'static str {
        match self {
            LauncherStyle::Default => "default",
            LauncherStyle::Dux => "dux",
            LauncherStyle::Windows => "windows",
            LauncherStyle::Gnome => "gnome",
            LauncherStyle::Kde => "kde",
            LauncherStyle::Xfce => "xfce",
            LauncherStyle::BlackBerry => "blackberry",
        }
    }

    pub fn from_key(key: &str) -> Option<LauncherStyle> {
        ALL.into_iter().find(|s| s.key() == key.trim())
    }

    pub fn title(self) -> &'static str {
        match self {
            LauncherStyle::Default => "Launchpad",
            LauncherStyle::Dux => "Dux",
            LauncherStyle::Windows => "Windows Start",
            LauncherStyle::Gnome => "GNOME",
            LauncherStyle::Kde => "KDE Kickoff",
            LauncherStyle::Xfce => "Xfce Whisker",
            LauncherStyle::BlackBerry => "BlackBerry 10",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            LauncherStyle::Default => "A searchable grid of every app on a card in the middle of the screen.",
            LauncherStyle::Dux => "A compact searchable list of apps with their descriptions.",
            LauncherStyle::Windows => "Pinned apps, recommended (recently used) apps and an All apps list, opening from the taskbar.",
            LauncherStyle::Gnome => "A full-screen overview: search at the top, a paged app grid, favourites in a dash below.",
            LauncherStyle::Kde => "Categories on the left, apps on the right, favourites first, power actions in the footer.",
            LauncherStyle::Xfce => "A compact menu: apps on the left, categories on the right, search and session buttons.",
            LauncherStyle::BlackBerry => "A full-screen hub of categories beside a paged grid of large app tiles.",
        }
    }

    pub fn capabilities(self) -> Capabilities {
        let c = |placement, categories, favorites, recent, paged, session| Capabilities { placement, categories, favorites, recent, paged, session };
        match self {
            LauncherStyle::Default => c(Placement::Centered, false, false, false, false, false),
            LauncherStyle::Dux => c(Placement::Centered, false, false, false, false, false),
            LauncherStyle::Windows => c(Placement::Anchored, false, true, true, true, true),
            LauncherStyle::Gnome => c(Placement::FullScreen, false, true, false, true, false),
            LauncherStyle::Kde => c(Placement::Anchored, true, true, true, false, true),
            LauncherStyle::Xfce => c(Placement::Anchored, true, true, true, false, true),
            LauncherStyle::BlackBerry => c(Placement::FullScreen, true, true, true, true, false),
        }
    }

    /// The style in effect: the user's `choice` when it names a style, otherwise the theme's token (`Default` when
    /// that is unknown too). `choice` is the raw `decebal.json` value, so `"theme"`, `""` and typos all defer.
    pub fn resolve(choice: Option<&str>, theme_token: &str) -> LauncherStyle {
        choice.and_then(LauncherStyle::from_key).or_else(|| LauncherStyle::from_key(theme_token)).unwrap_or(LauncherStyle::Default)
    }

    /// The style for this process: `decebal.json`'s choice over the current theme's token.
    pub fn current() -> LauncherStyle {
        let choice = crate::config::shell_settings().launcher.style;
        LauncherStyle::resolve(Some(&choice), &crate::theme::th().launcher_style)
    }
}

/// Writes the user's choice into `decebal.json` (`None` = follow the theme), keeping every other key of the file and
/// of its `launcher` object.
pub fn save_choice(path: &std::path::Path, style: Option<LauncherStyle>) -> std::io::Result<()> {
    let doc: serde_json::Value = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut launcher = doc.get("launcher").filter(|v| v.is_object()).cloned().unwrap_or_else(|| serde_json::json!({}));
    launcher["style"] = serde_json::json!(style.map_or(THEME, LauncherStyle::key));
    crate::config::write_setting(path, "launcher", launcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_stable_unique_and_round_trip() {
        assert_eq!(ALL.map(LauncherStyle::key), *KEYS);
        for s in ALL {
            assert_eq!(LauncherStyle::from_key(s.key()), Some(s));
            assert!(!s.title().is_empty() && !s.description().is_empty());
        }
        assert_eq!(LauncherStyle::from_key("Windows"), None, "keys are exact");
    }

    #[test]
    fn the_users_choice_wins_and_theme_or_unknown_defers_to_the_theme() {
        assert_eq!(LauncherStyle::resolve(Some("kde"), "dux"), LauncherStyle::Kde);
        assert_eq!(LauncherStyle::resolve(Some("theme"), "dux"), LauncherStyle::Dux);
        assert_eq!(LauncherStyle::resolve(Some(""), "gnome"), LauncherStyle::Gnome);
        assert_eq!(LauncherStyle::resolve(Some("bogus"), "xfce"), LauncherStyle::Xfce);
        assert_eq!(LauncherStyle::resolve(None, "default"), LauncherStyle::Default);
        assert_eq!(LauncherStyle::resolve(None, "something-newer"), LauncherStyle::Default, "an unknown token falls back");
    }

    #[test]
    fn anchored_and_full_screen_styles_are_declared() {
        use Placement::*;
        let p = |s: LauncherStyle| s.capabilities().placement;
        assert_eq!([LauncherStyle::Windows, LauncherStyle::Kde, LauncherStyle::Xfce].map(p), [Anchored; 3]);
        assert_eq!([LauncherStyle::Gnome, LauncherStyle::BlackBerry].map(p), [FullScreen; 2]);
        assert!(LauncherStyle::Kde.capabilities().categories && LauncherStyle::Xfce.capabilities().categories);
    }

    #[test]
    fn saving_keeps_other_keys_and_theme_clears_the_choice() {
        let dir = std::env::temp_dir().join(format!("decebal-launcher-style-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("decebal.json");
        std::fs::write(&path, r#"{"theme":{"name":"macos"},"launcher":{"other":1},"bar":{"position":"top"}}"#).unwrap();
        save_choice(&path, Some(LauncherStyle::Xfce)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["launcher"], serde_json::json!({"other": 1, "style": "xfce"}));
        assert_eq!(v["theme"]["name"], "macos");
        assert_eq!(v["bar"]["position"], "top");
        save_choice(&path, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["launcher"]["style"], "theme");
        let _ = std::fs::remove_dir_all(dir);
    }
}
