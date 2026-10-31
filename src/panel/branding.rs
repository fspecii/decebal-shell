//! Theme-owned panel branding (`[branding]` in `theme.toml`): the logo every renderer draws for its launcher / system
//! menu / Start button, an optional text label for the launcher (GNOME's "Activities"), and the name the menu bar shows
//! when no window is focused (macOS: "Finder"). Data only: a package image, an icon-theme name or a built-in glyph; no
//! commands. Without `[branding]` every renderer keeps its own default (Apple and "Finder" on the menu bar, four squares
//! on the taskbar, the distribution logo on the rail, the apps grid on the Dux bar).

use serde_json::Value;
use std::path::PathBuf;

/// Glyphs Decebal draws itself. Trademarked shapes are drawn only by the styles that imitate their owner by default.
pub const BUILTINS: &[&str] = &["apple", "windows", "os", "grid", "none"];

/// What an explicit logo becomes when it cannot be had at run time (a missing icon, a missing or unreadable image, an
/// invalid value that slipped past validation): a neutral glyph, never a style's trademark logo.
pub const NEUTRAL: Logo = Logo::Builtin("grid");

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Logo {
    /// The renderer's own (no `[branding] logo`).
    Style,
    /// No logo: the launcher shows its label only (or the grid when it has none).
    None,
    Builtin(&'static str),
    /// A name looked up in the user's icon theme (`icon:start-here-kde`).
    Icon(String),
    /// A package image: relative as written, absolute once [`crate::theme::live::Source`] resolved it inside the package.
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Branding {
    pub logo: Logo,
    /// Repaint the logo in the bar's text colour (symbolic logos).
    pub tint: bool,
    pub launcher_label: Option<String>,
    pub desktop_label: Option<String>,
}

impl Default for Branding {
    fn default() -> Self {
        Branding { logo: Logo::Style, tint: false, launcher_label: None, desktop_label: None }
    }
}

/// Parses a `logo` value: `builtin:<name>`, `icon:<name>` or a package-relative image path.
pub fn parse_logo(s: &str) -> Result<Logo, String> {
    let s = s.trim();
    if let Some(name) = s.strip_prefix("builtin:") {
        return match BUILTINS.iter().find(|b| **b == name) {
            Some(&"none") => Ok(Logo::None),
            Some(b) => Ok(Logo::Builtin(b)),
            None => Err(format!("`{name}` is not a built-in logo ({})", BUILTINS.join(", "))),
        };
    }
    if let Some(name) = s.strip_prefix("icon:") {
        let ok = !name.is_empty() && name.len() <= 96 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        return if ok { Ok(Logo::Icon(name.to_string())) } else { Err(format!("`{name}` is not an icon name (letters, digits, `-`, `_`, `.`)")) };
    }
    if s.is_empty() {
        return Err("is empty".into());
    }
    Ok(Logo::File(PathBuf::from(s)))
}

fn label(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty() && s.chars().count() <= 24 && !s.chars().any(char::is_control)).map(str::to_string)
}

impl Branding {
    /// From a theme's extra sections (invalid values mean the renderer's default, never an error here; the manifest
    /// validator reports them).
    pub fn from_extra(extra: &serde_json::Map<String, Value>) -> Branding {
        let Some(b) = extra.get("branding") else { return Branding::default() };
        Branding {
            // absent: the style's own logo; present but unusable: the neutral one (the validator rejects it at install)
            logo: match b.get("logo") {
                None => Logo::Style,
                Some(v) => v.as_str().and_then(|s| parse_logo(s).ok()).unwrap_or(NEUTRAL),
            },
            tint: b.get("logo_tint").and_then(Value::as_bool).unwrap_or(false),
            launcher_label: label(b.get("launcher_label")),
            desktop_label: label(b.get("desktop_label")),
        }
    }
    /// The name shown for the desktop when no window is focused.
    pub fn desktop_label<'a>(&'a self, style_default: &'a str) -> &'a str {
        self.desktop_label.as_deref().unwrap_or(style_default)
    }
    /// A package image path the theme names (checked for existence by the package validator).
    pub fn asset(&self) -> Option<&std::path::Path> {
        match &self.logo {
            Logo::File(p) => Some(p),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn logos_parse_each_form_and_reject_junk() {
        assert_eq!(parse_logo("builtin:os"), Ok(Logo::Builtin("os")));
        assert_eq!(parse_logo("builtin:none"), Ok(Logo::None));
        assert_eq!(parse_logo("icon:start-here-kde"), Ok(Logo::Icon("start-here-kde".into())));
        assert_eq!(parse_logo("icons/logo.png"), Ok(Logo::File("icons/logo.png".into())));
        assert!(parse_logo("builtin:tux").is_err());
        assert!(parse_logo("icon:../../etc/passwd").is_err());
        assert!(parse_logo("icon:").is_err());
    }

    #[test]
    fn only_an_absent_logo_keeps_the_style_default() {
        let none = serde_json::Map::new();
        assert_eq!(Branding::from_extra(&none), Branding::default());
        let extra: serde_json::Map<String, Value> = serde_json::from_value(json!({"branding": {"launcher_label": "", "desktop_label": "A label far too long to fit any bar"}})).unwrap();
        assert_eq!(Branding::from_extra(&extra), Branding::default(), "no logo: the style's own");
        let extra: serde_json::Map<String, Value> = serde_json::from_value(json!({"branding": {"logo": "builtin:tux"}})).unwrap();
        assert_eq!(Branding::from_extra(&extra).logo, NEUTRAL, "an explicit logo never falls back to a style's trademark");
        let extra: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"branding": {"logo": "icon:start-here", "logo_tint": true, "launcher_label": "Activities", "desktop_label": "Desktop"}})).unwrap();
        let b = Branding::from_extra(&extra);
        assert_eq!((b.logo.clone(), b.tint, b.launcher_label.as_deref(), b.desktop_label("Finder")), (Logo::Icon("start-here".into()), true, Some("Activities"), "Desktop"));
    }
}
