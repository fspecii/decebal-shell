//! Themes compiled into the binaries.

use super::model::{Def, Mode};

pub const DEFAULT_ID: &str = "macos";

/// (id, theme file). The first entry is the default.
pub const BUILTIN: &[(&str, &str)] = &[
    ("macos", include_str!("builtin/macos.toml")),
    ("graphite", include_str!("builtin/graphite.toml")),
    ("nord", include_str!("builtin/nord.toml")),
    ("dracula", include_str!("builtin/dracula.toml")),
    ("solarized", include_str!("builtin/solarized.toml")),
    ("windows11", include_str!("builtin/windows11.toml")),
    ("high-contrast", include_str!("builtin/high-contrast.toml")),
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
                for (what, bg) in [
                    ("background", t.background),
                    ("sidebar", t.sidebar),
                    ("surface", t.surface),
                    ("bar", t.bar),
                    ("popup", t.popup),
                    ("dock", t.dock),
                    ("overlay", t.overlay),
                    ("titlebar", t.titlebar),
                ] {
                    let c = contrast(t.text, bg);
                    assert!(c >= 4.5, "{tag}: text on {what} is {c:.2}:1");
                }
                assert!(contrast(t.bar_text, t.bar) >= 4.5, "{tag}: bar text");
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
        for want in ["macos", "graphite", "nord", "dracula", "solarized", "windows11", "high-contrast"] {
            assert!(ids.contains(&want), "{want}");
        }
        assert!(def("macos").unwrap().has_dark() && def("macos").unwrap().has_light());
        assert!(def("windows11").unwrap().has_light() && def("windows11").unwrap().has_dark());
    }
}
