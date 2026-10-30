//! Offline curation/package contracts; no network, live application or compositor.
use decebal::{
    settings::{
        io::Paths,
        tomlcfg::{Cfg, Val},
    },
    theme::{
        engine::{apply, archive::Limits, package},
        live::Source,
        model::{Def, Mode, Selection},
        palette,
    },
    widgets::{data, layout, paint, samples},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).into()
}

#[test]
fn catalogue_covers_shipped_designs_with_reviewed_sfw_provenance() {
    let catalogue: Value = serde_json::from_str(include_str!("../data/wallhaven-curated.json")).unwrap();
    assert_eq!(catalogue["format"], 1);
    let images = catalogue["images"].as_object().unwrap();
    let themes = catalogue["themes"].as_object().unwrap();
    let shipped: BTreeSet<_> = fs::read_dir(root().join("themes"))
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let text = fs::read_to_string(e.path().join("theme.toml")).ok()?;
            let doc = text.parse::<toml_edit::DocumentMut>().unwrap();
            let id = doc["id"].as_str().unwrap().to_string();
            (!palette::legacy_theme(&id)).then_some(id)
        })
        .collect();
    assert_eq!(themes.keys().cloned().collect::<BTreeSet<_>>(), shipped);
    assert_eq!(themes.len(), 21);
    assert!(catalogue["scope"]["uncovered_theme_ids"].as_array().unwrap().is_empty());
    let mut used = BTreeSet::new();
    for (theme, record) in themes {
        assert!(!record["placement"].as_str().unwrap().is_empty(), "{theme}");
        for mode in ["dark", "light"] {
            let id = record[mode]["image_id"].as_str().unwrap();
            assert!(used.insert(id), "each choice in this batch is individually distinct: {id}");
            let image = &images[id];
            assert_eq!(image["id"], id);
            assert_eq!(image["page_url"], format!("https://wallhaven.cc/w/{id}"));
            assert!(image["full_url"].as_str().unwrap().starts_with("https://w.wallhaven.cc/full/"));
            assert!(image["thumbnail_url"].as_str().unwrap().starts_with("https://th.wallhaven.cc/lg/"));
            assert_eq!(image["purity"], "sfw");
            assert_eq!(image["ai_review"]["flagged_ai"], false);
            assert!(image["ai_review"]["returned_ai_boolean"].is_null() || image["ai_review"]["returned_ai_boolean"] == false);
            for tag in image["tags"].as_array().unwrap() {
                assert_ne!(tag["id"], 133451);
                let name = tag["name"].as_str().unwrap().to_lowercase();
                assert!(!["ai art", "ai-generated", "midjourney", "stable diffusion", "dall-e", "digital art", "illustration", "space art"].contains(&name.as_str()), "{id}: {name}");
            }
            assert!(image["uploader"]["is_author"].is_null());
            assert!(image["author"]["name"].is_null(), "unverified candidates must not become credited authors");
            assert!(image["license"]["identifier"].is_null());
            assert!(image["source_url"].is_null() || image["source_url"].as_str().unwrap().starts_with("https://"));
            let full = &image["full_resolution_review"];
            assert_eq!(full["dimensions_verified"], true);
            assert!(full["bytes"].as_u64().unwrap() <= 32 << 20);
            assert_eq!(full["sha256"].as_str().unwrap().len(), 64);
            assert!(!full["notes"].as_str().unwrap().is_empty());
            for key in ["detail_sha256", "thumbnail_sha256"] {
                let digest = image["evidence"][key].as_str().unwrap();
                assert!(digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit()));
            }
        }
    }
    assert_eq!(used.len(), images.len());
}

#[test]
fn aster_package_preserves_wallpaper_and_custom_widget_review() {
    let dir = std::env::temp_dir().join(format!("aster-contract-{}", std::process::id()));
    fs::create_dir_all(dir.join("shell")).unwrap();
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("rdwm.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("none.sock") };
    let src = Source { shell_json: paths.shell_json(), themes_dir: root().join("themes") };
    let cfg = "[general]\n[wallpaper]\nimage = \"/private/selected-photo.jpg\"\nmode = \"fit\"\ncolor = \"#987654\"\n[decoration]\nenabled = true\nbuttons = false\nzoom_action = \"maximize\"\n";
    fs::write(&paths.rdwm_config, cfg).unwrap();
    let custom = json!({"enabled":true,"items":[{"use":"builtin:clock-card","x":123,"options":{"keep":"yes"}}]});
    fs::write(paths.shell_json(), json!({"theme":{"name":"macos"},"widgets":custom,"wallhaven":{"selection":"retain"}}).to_string()).unwrap();
    let before = fs::read(paths.shell_json()).unwrap();
    let (packed, report) = package::pack_dir(&root().join("themes/aster"), &Limits::default()).unwrap();
    assert!(report.ok(), "{:?}", report.errors());
    assert_eq!(packed, package::pack_dir(&root().join("themes/aster"), &Limits::default()).unwrap().0);
    let def = src.load_def("aster").unwrap();
    assert!(!def.extra.contains_key("wallpaper") && def.inherit.is_none());
    for mode in [Mode::Dark, Mode::Light] {
        let selection = Selection { mode, ..Default::default() };
        assert!(apply::plan(&src, &paths, "aster", &selection).unwrap_err().contains("custom layout"));
        for choice in [apply::WidgetChoice::Keep, apply::WidgetChoice::Replace] {
            let plan = apply::plan_with_widgets(&src, &paths, "aster", &selection, choice).unwrap();
            let config = Cfg::parse(&plan.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new).unwrap();
            for (key, expected) in [("image", "/private/selected-photo.jpg"), ("mode", "fit"), ("color", "#987654")] {
                assert_eq!(config.get("wallpaper", key), Some(Val::Str(expected.into())));
            }
            assert_eq!(config.get("decoration", "style"), Some(Val::Str("breeze".into())));
            assert_eq!(config.get("decoration", "buttons"), Some(Val::Bool(true)));
            assert_eq!(config.get("decoration", "enabled"), Some(Val::Bool(true)));
            assert_eq!(config.get("decoration", "zoom_action"), Some(Val::Str("maximize".into())));
            let shell: Value = serde_json::from_str(&plan.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
            assert_eq!(shell["bar"]["position"], "top");
            assert_eq!(shell["dock"]["enabled"], true);
            assert_eq!(shell["dock"]["position"], "bottom");
            assert_eq!(shell["wallhaven"]["selection"], "retain");
            if choice == apply::WidgetChoice::Keep {
                assert_eq!(shell["widgets"]["items"], custom["items"]);
            } else {
                assert_eq!(shell["widgets"]["items"].as_array().unwrap().len(), 2);
            }
        }
    }
    assert_eq!(fs::read(paths.shell_json()).unwrap(), before, "review/cancellation performs no writes");
    assert_eq!(fs::read_to_string(&paths.rdwm_config).unwrap(), cfg);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn aster_native_widgets_render_sample_and_absent_sources_in_both_modes() {
    let def = Def::parse(include_str!("../themes/aster/theme.toml")).unwrap();
    let mut fonts = decebal::text::Fonts::load().unwrap();
    for dark in [true, false] {
        let th = def.resolve("aster", dark, &Selection::default());
        for absent in [true, false] {
            let mut data = data::sample();
            if absent {
                data.vals.clear();
                data.tags.clear();
            }
            for id in ["clock-card", "now-playing-mini"] {
                let widget = samples::get(id).unwrap();
                assert!(widget.commands.is_empty());
                let tree = layout::layout(&widget, &layout::Ctx { data: &data, metrics: &paint::FontMetrics(&fonts), theme_radius: th.radius }, None);
                let dump = layout::dump(&tree);
                if absent && id == "now-playing-mini" {
                    assert!(dump.contains("Nothing playing"));
                    assert!(!dump.contains("PlayPause") && !dump.contains("Prev") && !dump.contains("Next"));
                }
                if absent && id == "clock-card" {
                    assert!(!dump.contains("°C"));
                }
                let image = paint::render(&widget, &data, &th, &mut fonts, &root().join("widgets"), 0).unwrap();
                assert!(image.data().iter().any(|c| *c != 0));
                if let Some(out) = std::env::var_os("DECEBAL_ASTER_EVIDENCE") {
                    let out = PathBuf::from(out);
                    fs::create_dir_all(&out).unwrap();
                    image.save_png(out.join(format!("{id}-{}-{}.png", if dark { "dark" } else { "light" }, if absent { "empty" } else { "sample" }))).unwrap();
                }
            }
        }
    }
}

#[test]
fn every_visible_package_preserves_wallpaper_preferences_in_both_mode_plans() {
    let catalogue: Value = serde_json::from_str(include_str!("../data/wallhaven-curated.json")).unwrap();
    let dir = std::env::temp_dir().join(format!("visible-wallpaper-contract-{}", std::process::id()));
    fs::create_dir_all(dir.join("shell")).unwrap();
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("rdwm.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("none.sock") };
    let src = Source { shell_json: paths.shell_json(), themes_dir: root().join("themes") };
    let original_config = "[general]\n[wallpaper]\nimage = \"/private/independent-photo.jpg\"\nmode = \"fit\"\ncolor = \"#123456\"\n";
    fs::write(&paths.rdwm_config, original_config).unwrap();
    let custom = json!({"enabled":true,"items":[{"use":"builtin:clock-card","x":213,"options":{"retain":true}}]});
    fs::write(paths.shell_json(), json!({"theme":{"name":"macos"},"widgets":custom,"wallhaven":{"selection":"keep-me"}}).to_string()).unwrap();
    let before = fs::read(paths.shell_json()).unwrap();
    for (id, _) in catalogue["themes"].as_object().unwrap() {
        let package_path = root().join("themes").join(id);
        let entries = package::read_dir(&package_path, &Limits::default()).unwrap();
        let report = package::check(&entries, &Limits::default());
        assert!(report.ok(), "{id}: {:?}", report.errors());
        assert!(!src.load_def(id).unwrap().extra.contains_key("wallpaper"));
        for mode in [Mode::Dark, Mode::Light] {
            let plan = apply::plan_with_widgets(&src, &paths, id, &Selection { mode, ..Default::default() }, apply::WidgetChoice::Keep).unwrap();
            let config = Cfg::parse(&plan.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new).unwrap();
            for (key, want) in [("image", "/private/independent-photo.jpg"), ("mode", "fit"), ("color", "#123456")] {
                assert_eq!(config.get("wallpaper", key), Some(Val::Str(want.into())), "{id}: {key}");
            }
            assert!(!plan.notes.iter().any(|n| n.contains("wallpaper") && (n.contains("missing") || n.contains("download"))), "{id}: {:?}", plan.notes);
            let shell: Value = serde_json::from_str(&plan.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
            assert_eq!(shell["widgets"]["items"], custom["items"], "{id}: Keep custom layout");
            assert_eq!(shell["wallhaven"]["selection"], "keep-me");
        }
    }
    assert_eq!(fs::read(paths.shell_json()).unwrap(), before);
    assert_eq!(fs::read_to_string(paths.rdwm_config).unwrap(), original_config);
    fs::remove_dir_all(dir).unwrap();
}
