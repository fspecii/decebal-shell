//! Every package under `themes/` (the built-ins and the asset-carrying showcase themes) is a valid, applicable, readable package.

use crate::theme::builtin;
use crate::theme::engine::apply::{self, tests::sandbox};
use crate::theme::engine::archive::Limits;
use crate::theme::engine::package;
use crate::theme::model::{contrast, Selection};
use std::path::PathBuf;

const SHOWCASE: &[&str] = &["gruvbox-dark", "everforest", "synthwave", "frutiger-aero", "gnome-adwaita-like"];

fn themes_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes")
}

fn package_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(themes_root()).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

#[test]
fn every_theme_folder_is_a_valid_package_whose_id_is_its_folder() {
    let dirs = package_dirs();
    assert!(dirs.len() >= builtin::BUILTIN.len() + SHOWCASE.len());
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let entries = package::read_dir(&dir, &Limits::default()).unwrap();
        let r = package::check(&entries, &Limits::default());
        assert!(r.ok(), "{name}: {:?}", r.errors());
        assert_eq!(r.id.as_deref(), Some(name.as_str()), "{name}: id matches the folder");
        // the pack is reproducible
        let (a, _) = package::pack_dir(&dir, &Limits::default()).unwrap();
        let (b, _) = package::pack_dir(&dir, &Limits::default()).unwrap();
        assert_eq!(a, b, "{name}");
    }
}

#[test]
fn showcase_themes_ship_descriptions_and_native_layouts_without_wallpaper_defaults() {
    for id in SHOWCASE {
        let dir = themes_root().join(id);
        let entries = package::read_dir(&dir, &Limits::default()).unwrap();
        assert!(!entries.iter().any(|e| e.path.starts_with("wallpapers/") || e.path.starts_with("screenshots/")), "{id}: retired generated assets");
        let text = std::fs::read_to_string(dir.join("theme.toml")).unwrap();
        assert!(text.contains("\ndescription = \"") && text.contains("\ntags = ["), "{id}: description and tags");
        let def = src_def(id);
        assert!(!def.extra.contains_key("wallpaper") && !def.extra.contains_key("screenshots"), "{id}: independent wallpaper");
        assert!(dir.join("README.md").is_file(), "{id}: package guidance");
        assert!(std::fs::read_to_string(themes_root().join("../docs/themes.md")).unwrap().contains(&format!("decthemectl theme apply {id}")), "{id} in docs/themes.md");
    }
}

#[test]
fn showcase_themes_are_readable_in_every_variant_they_declare() {
    for id in SHOWCASE {
        let (d, src, _) = sandbox(&format!("read-{id}"));
        let def = src_def(id);
        let mut variants = 0;
        for dark in [true, false] {
            if (dark && !def.has_dark()) || (!dark && !def.has_light()) {
                continue;
            }
            variants += 1;
            let t = def.resolve(id, dark, &Selection::default());
            for (what, bg) in [("background", t.background), ("surface", t.surface), ("popup", t.popup), ("titlebar", t.titlebar), ("sidebar", t.sidebar)] {
                assert!(contrast(t.text, bg) >= 4.5, "{id} dark={dark}: text on {what} {:.2}", contrast(t.text, bg));
            }
            assert!(contrast(t.bar_text, t.bar) >= 4.5, "{id} dark={dark}: bar text");
            assert!(contrast(t.accent_text, t.accent) >= 3.0, "{id} dark={dark}: accent text");
            assert_eq!(t.background.luminance() > 0.3, !dark, "{id} dark={dark}: variant brightness");
        }
        assert!(variants >= 1);
        let _ = (src, std::fs::remove_dir_all(d));
    }
}

fn src_def(id: &str) -> crate::theme::model::Def {
    let text = std::fs::read_to_string(themes_root().join(id).join("theme.toml")).unwrap();
    crate::theme::model::Def::parse(&text).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// Install each showcase package like a user would, apply it in a sandbox, and verify its style applies while the chosen wallpaper stays independent.
#[test]
fn every_showcase_theme_installs_and_applies() {
    for id in SHOWCASE {
        let (d, src, paths) = sandbox(&format!("ship-{id}"));
        std::fs::write(&paths.rdwm_config, "[general]\n[wallpaper]\nimage = \"/my/background.png\"\nmode = \"center\"\ncolor = \"#123456\"\n").unwrap();
        let entries = package::read_dir(&themes_root().join(id), &Limits::default()).unwrap();
        package::install_entries(&entries, &src.themes_dir, false, &Limits::default()).unwrap();
        for sel in [Selection::default(), Selection { mode: crate::theme::model::Mode::Light, ..Selection::default() }] {
            let a = apply::apply(&src, &paths, id, &sel).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(a.theme_id, *id);
            let cfg = std::fs::read_to_string(&paths.rdwm_config).unwrap();
            let parsed = crate::settings::tomlcfg::Cfg::parse(&cfg).unwrap();
            for (key, expected) in [("image", "/my/background.png"), ("mode", "center"), ("color", "#123456")] {
                assert_eq!(parsed.get("wallpaper", key), Some(crate::settings::tomlcfg::Val::Str(expected.into())), "{id}: independent wallpaper {key}");
            }
            assert!(cfg.contains("[decoration]") && cfg.contains("style = "), "{id}: decoration\n{cfg}");
        }
        let _ = std::fs::remove_dir_all(d);
    }
}
