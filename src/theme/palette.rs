//! Colour palettes, separate from themes.
//!
//! A **theme** is the shape of the desktop (decoration style, radius, bar, dock, fonts, wallpaper, widgets); a **palette** is only
//! colours: a dark and a light set of the shell's colour tokens. The selection (`"theme": {"palette": "catppuccin"}` in decebal.json,
//! `decthemectl palette set catppuccin`, the Colours pane) lays a palette over whichever theme is active, so every palette works on every
//! theme and switching theme keeps the palette. Without a palette the theme's own colours are used.
//!
//! Palette files are small TOML files (`palettes/<id>.toml` in the source tree, compiled in; yours go in `~/.config/decebal/palettes/`,
//! `$DECEBAL_PALETTES`), in the same legacy layout as a theme's colours:
//!
//! ```toml
//! name = "My palette"
//! description = "..."
//! [dark]
//! background = "#1e1e2e"   # sidebar surface bar bar_text popup dock overlay titlebar titlebar_inactive border
//! text = "#cdd6f4"         # text_secondary text_faint accent accent_text danger ok warn
//! accent = "#cba6f7"
//! [light]
//! ...
//! ```
//! A palette may have only one variant; a theme then keeps its own colours in the other mode.

use super::model::{Def, Tokens};
use std::path::PathBuf;

/// (id, file text), compiled in.
pub const BUILTIN: &[(&str, &str)] = &[
    ("terminal-amber", include_str!("../../palettes/terminal-amber.toml")),
    ("graphite", include_str!("../../palettes/graphite.toml")),
    ("high-contrast", include_str!("../../palettes/high-contrast.toml")),
    ("catppuccin", include_str!("../../palettes/catppuccin.toml")),
    ("rose-pine", include_str!("../../palettes/rose-pine.toml")),
    ("gruvbox", include_str!("../../palettes/gruvbox.toml")),
    ("tokyo-night", include_str!("../../palettes/tokyo-night.toml")),
    ("everforest", include_str!("../../palettes/everforest.toml")),
    ("kanagawa", include_str!("../../palettes/kanagawa.toml")),
    ("nord", include_str!("../../palettes/nord.toml")),
    ("dracula", include_str!("../../palettes/dracula.toml")),
    ("solarized", include_str!("../../palettes/solarized.toml")),
    ("monochrome", include_str!("../../palettes/monochrome.toml")),
];

/// The colour tokens a palette sets (everything else a theme says stays the theme's).
pub const COLOR_KEYS: &[&str] = &[
    "background",
    "sidebar",
    "surface",
    "bar",
    "bar_text",
    "popup",
    "dock",
    "overlay",
    "titlebar",
    "titlebar_inactive",
    "border",
    "text",
    "text_secondary",
    "text_faint",
    "accent",
    "accent_text",
    "danger",
    "ok",
    "warn",
];

pub struct Info {
    pub id: String,
    pub name: String,
    pub description: String,
    pub builtin: bool,
    pub has_dark: bool,
    pub has_light: bool,
}

/// Select a colour source explicitly. A named palette or theme colours replaces wallpaper
/// recolouring and a custom accent; the desktop design and all non-colour overrides stay intact.
pub fn select(base: &super::model::Selection, id: Option<&str>) -> super::model::Selection {
    let mut sel = super::dynamic::disable(base);
    sel.palette = id.map(str::to_string);
    sel.accent = None;
    sel
}

/// Old colour-first theme packages remain loadable for compatibility, but belong in Palettes.
/// User packages are never hidden by this classification.
pub fn legacy_theme(id: &str) -> bool {
    BUILTIN.iter().any(|(p, _)| *p == id) || matches!(id, "graphite" | "high-contrast" | "caelestia-mocha" | "material-you-dynamic" | "nothing-light" | "terminal-amber")
}

/// `$DECEBAL_PALETTES`, else `$XDG_CONFIG_HOME/decebal/palettes`, else `~/.config/decebal/palettes`.
pub fn user_dir() -> PathBuf {
    std::env::var_os("DECEBAL_PALETTES")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(|x| PathBuf::from(x).join("decebal/palettes")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/decebal/palettes")))
        .unwrap_or_else(|| PathBuf::from("palettes"))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.')) && !id.starts_with('.')
}

fn parse(text: &str) -> Result<Def, String> {
    let d = Def::parse(text)?;
    if d.dark.is_none() && d.light.is_none() {
        return Err("a palette needs a [dark] or a [light] table".into());
    }
    Ok(d)
}

/// The palette `id` (user files win over built-ins; the id is matched ignoring case, spaces and hyphens), or `None`.
pub fn load(id: &str) -> Option<Def> {
    let key = super::builtin::norm(id);
    if key.is_empty() {
        return None;
    }
    let all = ids();
    let real = all.iter().find(|i| i.as_str() == id).or_else(|| all.iter().find(|i| super::builtin::norm(i) == key))?;
    if valid_id(real) {
        if let Ok(text) = std::fs::read_to_string(user_dir().join(format!("{real}.toml"))) {
            match parse(&text) {
                Ok(d) => return Some(d),
                Err(e) => crate::log_warn!("palette `{real}` in {}: {e}", user_dir().display()),
            }
        }
    }
    BUILTIN.iter().find(|b| b.0 == real).and_then(|b| parse(b.1).ok())
}

/// Every id: built-ins first, then the user's files.
pub fn ids() -> Vec<String> {
    let mut v: Vec<String> = BUILTIN.iter().map(|b| b.0.to_string()).collect();
    let mut mine: Vec<String> = std::fs::read_dir(user_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".toml").map(String::from))
        .filter(|id| valid_id(id) && !v.contains(id))
        .collect();
    mine.sort();
    v.extend(mine);
    v
}

pub fn list() -> Vec<Info> {
    ids()
        .into_iter()
        .filter_map(|id| {
            let builtin = BUILTIN.iter().any(|b| b.0 == id);
            match load(&id) {
                Some(d) => Some(Info { name: d.name.clone(), description: d.description.clone(), has_dark: d.dark.is_some(), has_light: d.light.is_some(), builtin, id }),
                None => None,
            }
        })
        .collect()
}

impl Def {
    /// This theme with the palette's colours laid over it. A variant the palette has replaces the theme's colour tokens of that
    /// variant (and is created when the theme lacks it); a variant it lacks stays the theme's.
    pub fn with_palette(&self, p: &Def) -> Def {
        let lay = |own: &Option<Tokens>, pal: &Option<Tokens>| -> Option<Tokens> {
            let Some(pal) = pal else { return own.clone() };
            let mut t = own.clone().unwrap_or_default();
            for k in COLOR_KEYS {
                if let Some(v) = pal.get(*k).or_else(|| p.common.get(*k)) {
                    t.insert((*k).to_string(), v.clone());
                }
            }
            Some(t)
        };
        let mut d = self.clone();
        d.dark = lay(&self.dark, &p.dark);
        d.light = lay(&self.light, &p.light);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin::{self, BUILTIN as THEMES};
    use crate::theme::model::{contrast, Selection};

    #[test]
    fn every_builtin_palette_parses_with_both_variants_and_all_tokens() {
        for (id, text) in BUILTIN {
            let d = parse(text).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert!(d.dark.is_some() && d.light.is_some(), "{id}: dark and light");
            for v in [d.dark.as_ref().unwrap(), d.light.as_ref().unwrap()] {
                for k in COLOR_KEYS {
                    assert!(v.contains_key(*k), "{id}: missing `{k}`");
                }
            }
            assert_eq!(load(id).unwrap().name, d.name);
        }
        assert_eq!(list().iter().filter(|i| i.builtin).count(), BUILTIN.len());
        assert!(load("Tokyo Night").is_some() && load("tokyo_night").is_some() && load("nope").is_none());
    }

    /// The point of the separation: every palette on every theme, in both modes, is readable.
    #[test]
    fn every_palette_is_readable_on_every_theme_in_both_modes() {
        for (pid, _) in BUILTIN {
            let pal = load(pid).unwrap();
            for (tid, _) in THEMES {
                let original = builtin::def(tid).unwrap();
                let def = original.with_palette(&pal);
                for dark in [true, false] {
                    let sel = Selection { palette: Some((*pid).into()), ..Selection::default() };
                    let t = def.resolve(tid, dark, &sel);
                    let tag = format!("{pid} on {tid} {}", if dark { "dark" } else { "light" });
                    assert_eq!(t.dark, dark, "{tag}");
                    for (what, bg) in [("background", t.background), ("sidebar", t.sidebar), ("surface", t.surface), ("bar", t.bar), ("popup", t.popup), ("dock", t.dock), ("titlebar", t.titlebar)] {
                        assert!(contrast(t.text, bg) >= 4.5, "{tag}: text on {what} {:.2}", contrast(t.text, bg));
                    }
                    assert!(contrast(t.bar_text, t.bar) >= 4.5, "{tag}: bar text");
                    assert!(contrast(t.text_secondary, t.surface) >= 3.0, "{tag}: secondary text");
                    assert!(contrast(t.accent_text, t.accent) >= 3.0, "{tag}: accent text");
                    assert!(contrast(t.accent, t.background) >= 2.0, "{tag}: accent on background");
                    // colours come from the palette, the shape from the theme
                    let variant = if dark { pal.dark.as_ref() } else { pal.light.as_ref() }.unwrap();
                    assert_eq!(Some(&crate::theme::model::V::S(t.background.hex())), variant.get("background"), "{tag}");
                    let base = original.resolve(tid, dark, &Selection::default());
                    assert_eq!((t.radius, t.dock_roundness, t.bar_height), (base.radius, base.dock_roundness, base.bar_height), "{tag}: theme geometry");
                    assert_eq!((&t.bar_style, &t.titlebar_style, &t.fonts), (&base.bar_style, &base.titlebar_style, &base.fonts), "{tag}: theme design");
                }
            }
        }
    }

    #[test]
    fn selecting_a_palette_replaces_other_colour_sources_without_changing_design() {
        use crate::theme::model::{Dynamic, Rgb};
        let base = Selection { name: "windows11".into(), accent: Some(Rgb(1, 2, 3)), radius: Some(17.0), dynamic: Some(Dynamic { enabled: true, ..Dynamic::default() }), ..Selection::default() };
        for id in [Some("nord"), None] {
            let sel = select(&base, id);
            assert_eq!(sel.name, base.name);
            assert_eq!(sel.radius, base.radius);
            assert_eq!(sel.palette.as_deref(), id);
            assert!(sel.accent.is_none());
            assert!(!sel.dynamic.as_ref().is_some_and(|d| d.enabled));
        }
    }

    #[test]
    fn a_one_variant_palette_leaves_the_other_mode_to_the_theme() {
        let p = Def::parse("name = \"Dark only\"\n[dark]\nbackground = \"#101010\"\ntext = \"#ffffff\"\naccent = \"#ff0000\"\n").unwrap();
        let base = builtin::def("macos").unwrap();
        let d = base.with_palette(&p);
        let sel = Selection::default();
        assert_eq!(d.resolve("macos", true, &sel).background.hex(), "#101010");
        assert_eq!(d.resolve("macos", false, &sel).background, base.resolve("macos", false, &sel).background);
    }
}
