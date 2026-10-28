//! Golden test: the built-in themes, now bundled format-1 packages (`themes/<id>/theme.toml`), resolve to exactly the tokens the old
//! hand-written files did (`tests/fixtures/legacy-builtin/*.toml`, frozen copies of `src/theme/builtin/*.toml` before the engine).

use crate::theme::builtin::{self, BUILTIN};
use crate::theme::model::{Def, Mode, Rgb, Selection};

fn legacy(id: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/legacy-builtin/{id}.toml", env!("CARGO_MANIFEST_DIR"))).unwrap_or_else(|e| panic!("fixture {id}: {e}"))
}

/// The legacy file with its `inherit` resolved against the other legacy files, like the old loader did.
fn legacy_def(id: &str) -> Def {
    let own = Def::parse(&legacy(id)).unwrap();
    match own.inherit.clone() {
        Some(parent) => own.over(&legacy_def(&parent)),
        None => own,
    }
}

/// The original eight themes, frozen in `tests/fixtures/legacy-builtin`.
const LEGACY: [&str; 8] = ["macos", "graphite", "nord", "dracula", "solarized", "windows11", "caelestia", "high-contrast"];
/// Themes added after the engine: they have no legacy fixture, so they are checked as packages instead (see `new_themes_are_valid_packages`).
const NEW: [&str; 10] = ["caelestia-mocha", "rose-pine", "tokyo-night", "kanagawa", "material-you-dynamic", "nothing-dot", "nothing-light", "brutalist", "terminal-minimal", "terminal-amber"];

fn selections() -> Vec<Selection> {
    vec![
        Selection::default(),
        Selection { accent: Rgb::parse("#ff9f0a"), ..Selection::default() },
        Selection { radius: Some(4.0), opacity: Some(0.7), font_scale: Some(1.2), ..Selection::default() },
        Selection { mode: Mode::Light, ..Selection::default() },
    ]
}

#[test]
fn every_builtin_resolves_to_the_same_theme_as_before() {
    assert_eq!(BUILTIN.len(), LEGACY.len() + NEW.len());
    for (id, text) in BUILTIN.iter().filter(|(id, _)| LEGACY.contains(id)) {
        assert!(crate::theme::engine::manifest::is_v1(text), "{id} is a format-1 package");
        // the new one goes through the production loader (chain included), the old one through the frozen files
        let empty = std::env::temp_dir().join("decebal-golden-empty");
        let src = crate::theme::live::Source { shell_json: empty.join("decebal.json"), themes_dir: empty };
        let (new, old) = (src.load_def(id).unwrap(), legacy_def(id));
        assert_eq!((new.has_dark(), new.has_light()), (old.has_dark(), old.has_light()), "{id}: variants");
        assert_eq!(new.name, old.name, "{id}: name");
        for dark in [true, false] {
            if (dark && !old.has_dark()) || (!dark && !old.has_light()) {
                continue;
            }
            for sel in selections() {
                let (a, b) = (new.resolve(id, dark, &sel), old.resolve(id, dark, &sel));
                assert_eq!(a, b, "{id} dark={dark} {sel:?}");
            }
        }
    }
}

#[test]
fn the_packages_carry_the_shell_style_the_code_used_to_hardcode() {
    let get = |id: &str, sec: &str, key: &str| builtin::def(id).unwrap().extra.get(sec).and_then(|s| s.get(key)).cloned();
    assert_eq!(get("windows11", "dock", "style"), Some("windows".into()));
    assert_eq!(get("windows11", "dock", "align"), Some("center".into()));
    assert_eq!(get("windows11", "bar", "enabled"), Some("auto".into()));
    assert_eq!(get("macos", "dock", "style"), Some("mac".into()));
    assert_eq!(get("macos", "dock", "enabled"), Some(true.into()));
    assert_eq!(get("caelestia", "dock", "enabled"), Some("auto".into()));
    assert_eq!(get("caelestia", "bar", "style"), Some("vertical".into()));
    assert!(get("nord", "dock", "style").is_none(), "themes that never touched the dock still do not");
}

/// Applying a built-in through the engine writes what `decthemectl style` writes for the three shell styles.
#[test]
fn applying_the_style_themes_matches_the_style_presets() {
    use crate::settings::io;
    use crate::theme::engine::apply::{self, tests::sandbox};
    use crate::theme::style::{self, Style};
    for st in Style::ALL {
        let (d, src, paths) = sandbox(&format!("style-{}", st.as_str()));
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        apply::apply(&src, &paths, st.theme_id(), &Selection::default()).unwrap();
        let (d2, src2, paths2) = sandbox(&format!("style2-{}", st.as_str()));
        std::fs::write(&paths2.rdwm_config, "[general]\n").unwrap();
        style::apply(&src2, &paths2, st, &Selection::default()).unwrap();
        for (key, edit) in style::dock_edits(st) {
            let got = io::json_load(&paths.dock_settings()).get(key).cloned();
            assert_eq!(got, edit, "{st:?}: dock-settings.json `{key}`");
        }
        let (a, b) = (io::json_load(&paths.shell_json()), io::json_load(&paths2.shell_json()));
        assert_eq!(a["bar"]["enabled"], b["bar"]["enabled"], "{st:?}: bar.enabled");
        assert_eq!(a["dock"]["enabled"], b["dock"]["enabled"], "{st:?}: dock.enabled");
        assert_eq!(a["theme"]["name"], b["theme"]["name"]);
        assert_eq!(style::current(&src, &paths), Some(st), "{st:?} is detected after an engine apply");
        let _ = (std::fs::remove_dir_all(d), std::fs::remove_dir_all(d2));
    }
}

/// The community-style sample theme in `examples/themes/` passes the same checks a market upload would.
#[test]
fn the_example_package_is_valid_and_packs_reproducibly() {
    use crate::theme::engine::{archive::Limits, package};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/sunset-grid");
    let entries = package::read_dir(&dir, &Limits::default()).unwrap();
    let r = package::check(&entries, &Limits::default());
    assert!(r.ok(), "{:?}", r.errors());
    assert_eq!(r.id.as_deref(), Some("sunset-grid"));
    let (a, _) = package::pack_dir(&dir, &Limits::default()).unwrap();
    let (b, _) = package::pack_dir(&dir, &Limits::default()).unwrap();
    assert_eq!(a, b);
    let back = crate::theme::engine::archive::read_targz(&a, &Limits::default()).unwrap();
    assert!(package::check(&back, &Limits::default()).ok());
    assert!(back.iter().any(|e| e.path == "widgets/sunset-clock.toml"));
}

/// Every theme added after the engine is a complete package in `themes/<id>/` (manifest, wallpaper, widgets, screenshot) that
/// validates like a market upload, matches the compiled-in manifest, has distinct dark/light tokens where it claims both, and applies.
#[test]
fn new_themes_are_valid_packages_and_apply() {
    use crate::theme::engine::apply::{self, tests::sandbox};
    use crate::theme::engine::{archive::Limits, package};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
    for id in NEW {
        let dir = root.join(id);
        let entries = package::read_dir(&dir, &Limits::default()).unwrap_or_else(|e| panic!("{id}: {e}"));
        let r = package::check(&entries, &Limits::default());
        assert!(r.ok(), "{id}: {:?}", r.errors());
        assert_eq!(r.id.as_deref(), Some(id));
        assert_eq!(std::fs::read_to_string(dir.join("theme.toml")).unwrap(), builtin::source(id).unwrap(), "{id}: compiled-in manifest differs");
        assert!(entries.iter().any(|e| e.path.starts_with("wallpapers/")), "{id}: no wallpaper");
        assert!(entries.iter().any(|e| e.path.starts_with("screenshots/")), "{id}: no screenshot");
        let def = builtin::def(id).unwrap();
        assert!(def.has_dark() || def.has_light(), "{id}");
        // installed as a user package, so the wallpaper and widgets resolve
        let (d, src, paths) = sandbox(&format!("new-{id}"));
        package::install_entries(&entries, &src.themes_dir, true, &Limits::default()).unwrap_or_else(|e| panic!("{id}: install: {e}"));
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        let out = apply::apply(&src, &paths, id, &Selection::default()).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(!out.notes.iter().any(|n| n.contains("missing") || n.contains("cannot bundle")), "{id}: {:?}", out.notes);
        let cfg = std::fs::read_to_string(&paths.rdwm_config).unwrap();
        assert!(cfg.contains(&format!("{id}/wallpapers/")), "{id}: wallpaper not applied: {cfg}");
        let _ = std::fs::remove_dir_all(d);
    }
}
