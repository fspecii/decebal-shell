//! The presets that ship with decebal. They are generated from the built-in themes and the shell styles
//! (`theme::style`), live in memory only and are read-only; "Duplicate" makes an editable copy.
//!
//! They apply in merge mode: only the keys they mention change, `null` removes a key, so they never wipe settings that
//! have nothing to do with the look (workspaces, clock format, pinned apps, keyboard shortcuts, autostart).

use super::manifest::{Manifest, Origin};
use super::parts::{Part, PartSet};
use super::store::{Preset, BUILTIN_PREFIX};
use crate::settings::tomlcfg::{Cfg, Val};
use crate::theme::model::Selection;
use crate::theme::style::{self, Style};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// rdwm colour and titlebar keys of a built-in theme, plus extra `(section, key, value)` settings.
fn rdwm_toml(theme: &str, dark: bool, extra: &[(&str, &str, Val)]) -> String {
    let mut cfg = Cfg::default();
    if let Some(def) = crate::theme::builtin::def(theme) {
        let t = def.resolve(theme, dark, &Selection::default());
        let _ = crate::theme::rdwm::apply_to(&mut cfg, &t);
    }
    for (sec, key, v) in extra {
        let _ = cfg.set(sec, key, v);
    }
    cfg.text()
}

/// `decebal.json` share: the theme with every override reset, plus the style's bar and dock switches.
fn shell_json(theme: &str, style: Option<Style>, bar: Value, dock: Value) -> Value {
    let _ = style;
    json!({
        "theme": {"name": theme, "mode": "dark", "accent": null, "opacity": null, "radius": null, "dynamic": null},
        "bar": bar,
        "dock": dock,
    })
}

fn make(slug: &str, name: &str, description: &str, parts: &[Part], files: Vec<(&str, String)>) -> Preset {
    let mut m = Manifest::new(name, PartSet::new(parts.iter().copied()));
    m.description = description.into();
    m.author = "decebal".into();
    m.merge = true;
    m.origin = Origin::Builtin;
    Preset { id: format!("{BUILTIN_PREFIX}{slug}"), manifest: m, files: files.into_iter().map(|(n, t)| (n.to_string(), t.into_bytes())).collect::<BTreeMap<_, _>>() }
}

fn pretty(v: &Value) -> String {
    super::parts::pretty_json(v)
}

pub fn all() -> Vec<Preset> {
    let dock_mac = json!({"style": "mac", "roundness": null, "appSpacing": 6, "magnification": 1.4, "transparency": 0.4});
    let macos = make(
        "macos",
        "macOS default",
        "The macOS look: menu bar on top, floating magnifying dock, macOS theme and traffic-light titlebars.",
        &[Part::Shell, Part::Dock, Part::Rdwm],
        vec![
            ("decebal.json", pretty(&shell_json("macos", Some(Style::Macos), style::bar_value(Style::Macos, None), style::dock_value(Style::Macos, None)))),
            ("dock-settings.json", pretty(&dock_mac)),
            ("rdwm.toml", rdwm_toml("macos", true, &[("general", "gap_inner", Val::Int(6)), ("general", "gap_outer", Val::Int(6)), ("general", "border_px", Val::Int(1))])),
        ],
    );
    let caelestia = make(
        "caelestia",
        "Caelestia",
        "Material You colours, a vertical bar down the left edge and no dock (docs/caelestia.md).",
        &[Part::Shell, Part::Dock, Part::Rdwm],
        vec![
            ("decebal.json", pretty(&shell_json("caelestia", Some(Style::Caelestia), style::bar_value(Style::Caelestia, None), style::dock_value(Style::Caelestia, None)))),
            ("dock-settings.json", pretty(&json!({"style": "mac", "roundness": null}))),
            ("rdwm.toml", rdwm_toml("caelestia", true, &[("general", "gap_inner", Val::Int(10)), ("general", "gap_outer", Val::Int(10)), ("general", "border_px", Val::Int(2))])),
        ],
    );
    let windows = make(
        "windows",
        "Windows-like",
        "A Windows 11 taskbar along the bottom with centred icons, the Windows 11 theme and classic window buttons; the menu bar hides.",
        &[Part::Shell, Part::Dock, Part::Rdwm],
        vec![
            ("decebal.json", pretty(&shell_json("windows11", Some(Style::Windows), style::bar_value(Style::Windows, None), style::dock_value(Style::Windows, None)))),
            ("dock-settings.json", pretty(&json!({"style": "windows", "windowsAlign": "center"}))),
            ("rdwm.toml", rdwm_toml("windows11", true, &[("general", "gap_inner", Val::Int(0)), ("general", "gap_outer", Val::Int(0)), ("general", "border_px", Val::Int(1))])),
        ],
    );
    let minimal = make(
        "minimal-tiling",
        "Minimal tiling",
        "Keyboard-driven tiling: no dock, no decebal menu bar, rdwm's own slim bar, thin borders, no titlebars, small gaps.",
        &[Part::Shell, Part::Rdwm],
        vec![
            ("decebal.json", pretty(&shell_json("graphite", None, json!({"enabled": false}), json!({"enabled": false})))),
            (
                "rdwm.toml",
                rdwm_toml(
                    "graphite",
                    true,
                    &[
                        ("general", "gap_inner", Val::Int(4)),
                        ("general", "gap_outer", Val::Int(4)),
                        ("general", "border_px", Val::Int(2)),
                        ("general", "smart_gaps", Val::Bool(true)),
                        ("general", "smart_borders", Val::Bool(true)),
                        ("general", "layouts", Val::List(vec!["tile".into(), "monocle".into(), "bsp".into()])),
                        ("decoration", "enabled", Val::Bool(false)),
                        ("bar", "show", Val::Bool(true)),
                    ],
                ),
            ),
        ],
    );
    vec![macos, caelestia, windows, minimal]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::store::{apply, plan, Env};

    #[test]
    fn built_ins_are_valid_and_marked() {
        let all = all();
        assert_eq!(all.len(), 4);
        let names: Vec<&str> = all.iter().map(|p| p.manifest.name.as_str()).collect();
        assert_eq!(names, ["macOS default", "Caelestia", "Windows-like", "Minimal tiling"]);
        for p in &all {
            p.validate().unwrap_or_else(|e| panic!("{}: {e}", p.manifest.name));
            assert!(p.is_builtin() && p.id.starts_with(BUILTIN_PREFIX));
            assert!(p.manifest.merge);
            assert!(p.commands().is_empty(), "{}: a built-in must not run anything", p.manifest.name);
            assert!(!p.manifest.parts.has(Part::Autostart) && !p.manifest.parts.has(Part::Keys), "built-ins leave shortcuts and autostart alone");
            // bundles round trip too (export of a built-in)
            let back = Preset::from_bundle(&p.to_bundle().unwrap()).unwrap();
            assert_eq!(back.files, p.files);
        }
    }

    #[test]
    fn themes_exist_and_styles_match() {
        for (id, theme) in [("macos", "macos"), ("caelestia", "caelestia"), ("windows", "windows11"), ("minimal-tiling", "graphite")] {
            let p = all().into_iter().find(|p| p.id.ends_with(id)).unwrap();
            let v: Value = serde_json::from_str(p.text("decebal.json").unwrap()).unwrap();
            assert_eq!(v["theme"]["name"], theme);
            assert!(crate::theme::builtin::source(theme).is_some(), "{theme}");
        }
        let w = all().into_iter().find(|p| p.id.ends_with("windows")).unwrap();
        let dock: Value = serde_json::from_str(w.text("dock-settings.json").unwrap()).unwrap();
        assert_eq!(dock["style"], "windows");
        let c = all().into_iter().find(|p| p.id.ends_with("caelestia")).unwrap();
        let shell: Value = serde_json::from_str(c.text("decebal.json").unwrap()).unwrap();
        assert_eq!(shell["dock"]["enabled"], "auto");
        assert_eq!(shell["bar"]["enabled"], true);
    }

    #[test]
    fn applying_a_built_in_keeps_unrelated_settings() {
        let d = std::env::temp_dir().join(format!("decebal-builtin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        let env = Env {
            shell_dir: d.join("shell"),
            rdwm_config: d.join("config.toml"),
            presets_dir: d.join("presets"),
            themes_dir: d.join("themes"),
            backgrounds_dir: d.join("bg"),
            rdwmctl: "/nonexistent/rdwmctl".into(),
        };
        std::fs::write(
            env.shell_dir.join("decebal.json"),
            r##"{"theme": {"name": "nord", "accent": "#ff0000", "font": "Inter"}, "workspaces": "off", "clockHour12": true, "hot_corners": {"top_left": "lock"}}"##,
        )
        .unwrap();
        std::fs::write(env.shell_dir.join("dock-settings.json"), r#"{"magnification": 2.0, "style": "windows", "roundness": 0.3, "widgets": []}"#).unwrap();
        std::fs::write(&env.rdwm_config, "# mine\n[general]\ntags = [\"1\", \"2\"]\nborder_px = 5\n\n[[bind]]\nkeys = [\"super+q\"]\naction = \"close\"\n\n[[autostart]]\ncommand = \"decbar &\"\n")
            .unwrap();
        let p = all().into_iter().find(|p| p.id.ends_with("caelestia")).unwrap();
        let pl = plan(&env, &p);
        assert!(pl.errors.is_empty(), "{:?}", pl.errors);
        assert!(pl.commands.is_empty());
        apply(&env, &p, true).unwrap();
        let shell: Value = serde_json::from_str(&std::fs::read_to_string(env.shell_dir.join("decebal.json")).unwrap()).unwrap();
        assert_eq!(shell["theme"], json!({"name": "caelestia", "mode": "dark", "font": "Inter"}), "overrides reset, the font part is not ours");
        assert_eq!((shell["workspaces"].as_str(), shell["clockHour12"].as_bool()), (Some("off"), Some(true)));
        assert_eq!(shell["hot_corners"]["top_left"], "lock");
        assert_eq!(shell["dock"]["enabled"], "auto");
        let dock: Value = serde_json::from_str(&std::fs::read_to_string(env.shell_dir.join("dock-settings.json")).unwrap()).unwrap();
        assert_eq!(dock, json!({"magnification": 2.0, "style": "mac", "widgets": []}), "roundness removed by null, the rest kept");
        let rd = std::fs::read_to_string(&env.rdwm_config).unwrap();
        assert!(rd.contains("tags = [\"1\", \"2\"]") && rd.contains("super+q") && rd.contains("decbar &"), "{rd}");
        assert!(rd.contains("border_px = 2") && rd.contains("gap_inner = 10") && rd.contains("[colors]") && rd.contains("# mine"), "{rd}");
    }

    #[test]
    fn minimal_tiling_turns_things_off() {
        let p = all().into_iter().find(|p| p.id.ends_with("minimal-tiling")).unwrap();
        let rd = p.text("rdwm.toml").unwrap();
        let flat = crate::preset::parts::flatten_toml(rd);
        assert_eq!(flat.get("decoration.enabled").map(String::as_str), Some("false"));
        assert_eq!(flat.get("bar.show").map(String::as_str), Some("true"));
        assert!(flat.get("general.layouts").is_some_and(|l| l.contains("bsp")));
        let shell: Value = serde_json::from_str(p.text("decebal.json").unwrap()).unwrap();
        assert_eq!(shell["bar"]["enabled"], false);
        assert_eq!(shell["dock"]["enabled"], false);
    }
}
