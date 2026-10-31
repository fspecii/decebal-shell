//! Themes compiled into the binaries.

use super::model::{Def, Mode};

pub const DEFAULT_ID: &str = "macos";
/// The theme "colours from wallpaper" recolours.
pub const DYNAMIC_THEME: &str = "caelestia";

/// (id, theme file). The first entry is the default.
pub const BUILTIN: &[(&str, &str)] = &[
    ("macos", include_str!("../../themes/macos/theme.toml")),
    ("aster", include_str!("../../themes/aster/theme.toml")),
    ("sweeteh", include_str!("../../themes/sweeteh/theme.toml")),
    ("mountain-neon-dot", include_str!("../../themes/mountain-neon-dot/theme.toml")),
    ("kiln-court", include_str!("../../themes/kiln-court/theme.toml")),
    ("tidal-loom", include_str!("../../themes/tidal-loom/theme.toml")),
    ("glyph-rice", include_str!("../../themes/glyph-rice/theme.toml")),
    ("nextkde", include_str!("../../themes/nextkde/theme.toml")),
    ("orbit-glass", include_str!("../../themes/orbit-glass/theme.toml")),
    ("paper-signal", include_str!("../../themes/paper-signal/theme.toml")),
    ("moss-atlas", include_str!("../../themes/moss-atlas/theme.toml")),
    ("signal-deck", include_str!("../../themes/signal-deck/theme.toml")),
    ("graphite", include_str!("../../themes/graphite/theme.toml")),
    ("nord", include_str!("../../themes/nord/theme.toml")),
    ("dracula", include_str!("../../themes/dracula/theme.toml")),
    ("solarized", include_str!("../../themes/solarized/theme.toml")),
    ("windows11", include_str!("../../themes/windows11/theme.toml")),
    ("gnome-adwaita", include_str!("../../themes/gnome-adwaita/theme.toml")),
    ("gnome-catppuccin-dock", include_str!("../../themes/gnome-catppuccin-dock/theme.toml")),
    ("openkylin", include_str!("../../themes/openkylin/theme.toml")),
    ("openkylin-islands", include_str!("../../themes/openkylin-islands/theme.toml")),
    ("openkylin-light-seeking", include_str!("../../themes/openkylin-light-seeking/theme.toml")),
    ("openkylin-heyin", include_str!("../../themes/openkylin-heyin/theme.toml")),
    ("openkylin-classic", include_str!("../../themes/openkylin-classic/theme.toml")),
    ("deepin", include_str!("../../themes/deepin/theme.toml")),
    ("lingmo", include_str!("../../themes/lingmo/theme.toml")),
    ("caelestia", include_str!("../../themes/caelestia/theme.toml")),
    ("high-contrast", include_str!("../../themes/high-contrast/theme.toml")),
    ("caelestia-mocha", include_str!("../../themes/caelestia-mocha/theme.toml")),
    ("rose-pine", include_str!("../../themes/rose-pine/theme.toml")),
    ("tokyo-night", include_str!("../../themes/tokyo-night/theme.toml")),
    ("kanagawa", include_str!("../../themes/kanagawa/theme.toml")),
    ("material-you-dynamic", include_str!("../../themes/material-you-dynamic/theme.toml")),
    ("nothing-dot", include_str!("../../themes/nothing-dot/theme.toml")),
    ("nothing-light", include_str!("../../themes/nothing-light/theme.toml")),
    ("brutalist", include_str!("../../themes/brutalist/theme.toml")),
    ("terminal-minimal", include_str!("../../themes/terminal-minimal/theme.toml")),
    ("terminal-amber", include_str!("../../themes/terminal-amber/theme.toml")),
];

pub fn source(id: &str) -> Option<&'static str> {
    BUILTIN.iter().find(|b| b.0 == id).map(|b| b.1)
}

pub fn def(id: &str) -> Option<Def> {
    source(id).map(|s| Def::parse(s).unwrap_or_else(|e| panic!("built-in theme {id} is invalid: {e}")))
}

/// Case/space/hyphen-insensitive key: "Windows 11" == "windows-11" == "windows11".
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// Splits "macOS Dark" / "windows-11-light" into the theme name and the mode it implies.
pub fn split_mode(name: &str) -> (String, Option<Mode>) {
    let n = name.trim();
    let low = n.to_ascii_lowercase();
    for (suffix, mode) in [("dark", Mode::Dark), ("light", Mode::Light)] {
        if let Some(base) = low.strip_suffix(suffix) {
            let base = base.trim_end_matches([' ', '-', '_']);
            if !base.is_empty() && base != low {
                return (n[..base.len()].to_string(), Some(mode));
            }
        }
    }
    (n.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses_and_has_a_name() {
        for (id, _) in BUILTIN {
            let d = def(id).unwrap();
            assert!(!d.name.is_empty(), "{id}");
        }
        assert_eq!(BUILTIN[0].0, DEFAULT_ID);
    }

    #[test]
    fn mode_suffixes() {
        assert_eq!(split_mode("macOS Dark"), ("macOS".into(), Some(Mode::Dark)));
        assert_eq!(split_mode("windows-11-light"), ("windows-11".into(), Some(Mode::Light)));
        assert_eq!(split_mode("nord"), ("nord".into(), None));
        assert_eq!(split_mode("dark"), ("dark".into(), None));
        assert_eq!(norm("Windows 11"), norm("windows-11"));
    }
}

#[cfg(test)]
mod contrast_tests {
    use super::super::model::{contrast, Selection};
    use super::*;

    #[test]
    fn text_is_readable_on_every_surface_of_every_builtin() {
        for (id, _) in BUILTIN {
            let d = def(id).unwrap();
            for dark in [true, false] {
                if (dark && !d.has_dark()) || (!dark && !d.has_light()) {
                    continue;
                }
                let t = d.resolve(id, dark, &Selection::default());
                let tag = format!("{id} {}", if dark { "dark" } else { "light" });
                for (what, bg) in [("background", t.background), ("sidebar", t.sidebar), ("surface", t.surface), ("popup", t.popup), ("overlay", t.overlay), ("titlebar", t.titlebar)] {
                    let c = contrast(t.text, bg);
                    assert!(c >= 4.5, "{tag}: text on {what} is {c:.2}:1");
                }
                assert!(contrast(t.bar_text, t.bar) >= 4.5, "{tag}: bar text");
                assert!(contrast(t.dock_foreground(), t.dock) >= 4.5, "{tag}: dock text");
                assert!(contrast(t.dock_secondary(), t.dock) >= 3.0, "{tag}: dock secondary text");
                assert!(contrast(t.text_secondary, t.background) >= 3.0, "{tag}: secondary text {:.2}", contrast(t.text_secondary, t.background));
                assert!(contrast(t.text_secondary, t.surface) >= 3.0, "{tag}: secondary text on surface");
                assert!(contrast(t.accent_text, t.accent) >= 3.0, "{tag}: accent text {:.2}", contrast(t.accent_text, t.accent));
                assert!(contrast(t.accent, t.background) >= 2.0, "{tag}: accent visible on background {:.2}", contrast(t.accent, t.background));
                assert_eq!(t.dark, dark);
                let luminous = t.background.luminance() > 0.3;
                assert_eq!(luminous, !dark, "{tag}: variant brightness matches its label");
            }
        }
    }

    #[test]
    fn high_contrast_is_really_high() {
        let d = def("high-contrast").unwrap();
        for dark in [true, false] {
            let t = d.resolve("hc", dark, &Selection::default());
            assert!(contrast(t.text, t.background) >= 15.0);
            assert!(contrast(t.accent_text, t.accent) >= 7.0);
            assert!(t.outline >= 0.5 && t.popup_alpha == 1.0);
        }
    }

    #[test]
    fn the_shipped_set_is_complete() {
        let ids: Vec<&str> = BUILTIN.iter().map(|b| b.0).collect();
        for want in ["macos", "graphite", "nord", "dracula", "solarized", "windows11", "caelestia", "high-contrast"] {
            assert!(ids.contains(&want), "{want}");
        }
        assert!(def("macos").unwrap().has_dark() && def("macos").unwrap().has_light());
        assert!(def("windows11").unwrap().has_light() && def("windows11").unwrap().has_dark());
        assert!(def("caelestia").unwrap().has_light() && def("caelestia").unwrap().has_dark());
        for want in NEW_PACKAGES {
            assert!(ids.contains(want), "{want}");
            assert!(def(want).unwrap().has_light() && def(want).unwrap().has_dark(), "{want} has both variants");
        }
    }

    /// The pill-bar theme packages added after the first eight: each one is a package folder with a wallpaper under `themes/<id>/`.
    const NEW_PACKAGES: &[&str] = &["caelestia-mocha", "rose-pine", "tokyo-night", "kanagawa", "material-you-dynamic"];

    #[test]
    fn the_pill_bar_packages_are_distinct_and_keep_wallpaper_independent() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut seen = std::collections::HashSet::new();
        for id in NEW_PACKAGES {
            let d = def(id).unwrap();
            let get = |sec: &str, key: &str| d.extra.get(sec).and_then(|s| s.get(key)).cloned().unwrap_or_default().to_string();
            assert_eq!(get("bar", "style"), "\"vertical\"", "{id}: a pill bar");
            assert!(seen.insert((get("bar", "position"), get("dock", "style"), get("decoration", "style"))), "{id}: bar position, dock and titlebar style must differ from the other themes");
            assert!(!d.extra.contains_key("wallpaper") && !d.extra.contains_key("screenshots"), "{id}: retired artwork defaults");
            assert!(root.join("themes").join(id).join("README.md").is_file(), "{id}: compatibility guidance");
            assert!(!root.join("docs/themes").join(format!("{id}.png")).exists(), "{id}: retired documentation artwork");
        }
        assert!(!def("material-you-dynamic").unwrap().extra.contains_key("wallpaper"), "wallpaper colour matching is an explicit independent preference");
    }
}

#[cfg(test)]
mod caelestia_tests {
    use super::super::material::{self, Variant};
    use super::super::model::{Selection, Tokens, V};
    use super::*;

    fn colours(t: &Tokens) -> Vec<(String, String)> {
        t.iter().filter_map(|(k, v)| if let V::S(s) = v { s.starts_with('#').then(|| (k.clone(), s.clone())) } else { None }).collect()
    }

    /// `cargo test --lib print_caelestia_toml -- --ignored --nocapture` prints the two colour tables of caelestia.toml.
    #[test]
    #[ignore]
    fn print_caelestia_toml() {
        for (name, roles) in [("dark", material::CAELESTIA_DARK), ("light", material::generate(material::DEFAULT_SEED, Variant::TonalSpot, false))] {
            println!("[{name}]");
            let t = roles.tokens();
            for key in [
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
            ] {
                if let V::S(s) = &t[key] {
                    println!("{key} = \"{s}\"");
                }
            }
            println!();
        }
    }

    #[test]
    fn caelestia_colours_are_the_material_roles() {
        let d = def("caelestia").unwrap();
        let dark = d.dark.clone().unwrap();
        let light = d.light.clone().unwrap();
        // dark: caelestia-shell's shipped default scheme, light: the same seed's tonal-spot light scheme
        assert_eq!(colours(&dark), colours(&material::CAELESTIA_DARK.tokens()));
        assert_eq!(colours(&light), colours(&material::generate(material::DEFAULT_SEED, Variant::TonalSpot, false).tokens()));
    }

    #[test]
    fn caelestia_shape_metrics() {
        let t = def("caelestia").unwrap().resolve("caelestia", true, &Selection::default());
        assert!(t.bar_vertical() && !t.bar_floating(), "caelestia is a vertical bar");
        assert_eq!(t.dock_roundness, 1.0, "pill dock");
        assert!(t.radius >= 16.0, "generous radii");
        assert_eq!(t.name, "Caelestia");
        assert!(!def("macos").unwrap().resolve("macos", true, &Selection::default()).bar_floating());
        assert!(!def("windows11").unwrap().resolve("w", true, &Selection::default()).bar_floating());
    }
}
