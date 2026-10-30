//! Dux package contracts: private plans only, never a live apply.
use decebal::{
    settings::{
        io::Paths,
        tomlcfg::{Cfg, Val},
    },
    theme::{
        engine::{apply, archive::Limits, package},
        live::Source,
        model::{contrast, Def, Mode, Selection},
    },
    widgets::{data, layout, paint, samples},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).into()
}
#[test]
fn dux_package_preserves_wallpaper_and_custom_widget_review() {
    let dir = std::env::temp_dir().join(format!("dux-lavender-contract-{}", std::process::id()));
    fs::create_dir_all(dir.join("shell")).unwrap();
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("rdwm.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("none.sock") };
    let src = Source { shell_json: paths.shell_json(), themes_dir: root().join("themes") };
    let cfg = "[general]\n[wallpaper]\nimage = \"/private/selected-photo.jpg\"\nmode = \"fit\"\ncolor = \"#987654\"\n[decoration]\nenabled = true\nbuttons = false\nzoom_action = \"maximize\"\n";
    fs::write(&paths.rdwm_config, cfg).unwrap();
    let custom = json!({"enabled":true,"items":[{"use":"builtin:clock-card","x":123,"options":{"keep":"yes"}}]});
    fs::write(paths.shell_json(), json!({"theme":{"name":"macos"},"widgets":custom,"wallhaven":{"selection":"retain"}}).to_string()).unwrap();
    let before = fs::read(paths.shell_json()).unwrap();
    let (packed, report) = package::pack_dir(&root().join("themes/dux-lavender"), &Limits::default()).unwrap();
    assert!(report.ok(), "{:?}", report.errors());
    assert_eq!(packed, package::pack_dir(&root().join("themes/dux-lavender"), &Limits::default()).unwrap().0);
    assert!(src.list().iter().any(|entry| entry.id == "dux-lavender"));
    let def = src.load_def("dux-lavender").unwrap();
    assert!(!def.extra.contains_key("wallpaper") && def.inherit.is_none());
    for mode in [Mode::Dark, Mode::Light] {
        let selection = Selection { mode, ..Default::default() };
        assert!(apply::plan(&src, &paths, "dux-lavender", &selection).unwrap_err().contains("custom layout"));
        for choice in [apply::WidgetChoice::Keep, apply::WidgetChoice::Replace] {
            let plan = apply::plan_with_widgets(&src, &paths, "dux-lavender", &selection, choice).unwrap();
            let config = Cfg::parse(&plan.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new).unwrap();
            for (key, expected) in [("image", "/private/selected-photo.jpg"), ("mode", "fit"), ("color", "#987654")] {
                assert_eq!(config.get("wallpaper", key), Some(Val::Str(expected.into())));
            }
            assert_eq!(config.get("decoration", "style"), Some(Val::Str("materia".into())));
            assert_eq!(config.get("decoration", "buttons"), Some(Val::Bool(true)));
            assert_eq!(config.get("decoration", "enabled"), Some(Val::Bool(true)));
            assert_eq!(config.get("decoration", "zoom_action"), Some(Val::Str("maximize".into())));
            let shell: Value = serde_json::from_str(&plan.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
            assert_eq!(shell["bar"]["position"], "top");
            assert_eq!(shell["dock"]["enabled"], false);
            assert_eq!(shell["wallhaven"]["selection"], "retain");
            if choice == apply::WidgetChoice::Keep {
                assert_eq!(shell["widgets"]["items"], custom["items"]);
            } else {
                assert_eq!(shell["widgets"]["items"].as_array().unwrap().len(), 4);
            }
        }
    }
    assert_eq!(fs::read(paths.shell_json()).unwrap(), before, "review/cancellation performs no writes");
    assert_eq!(fs::read_to_string(&paths.rdwm_config).unwrap(), cfg);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dux_native_cards_use_safe_shared_definitions_and_honest_missing_states() {
    let def = Def::parse(include_str!("../themes/dux-lavender/theme.toml")).unwrap();
    let mut fonts = decebal::text::Fonts::load().unwrap();
    for dark in [true, false] {
        let th = def.resolve("dux-lavender", dark, &Selection::default());
        for bg in [th.background, th.surface, th.popup, th.bar, th.titlebar] {
            assert!(contrast(th.text, bg) >= 4.5);
        }
        assert!(contrast(th.accent_text, th.accent) >= 4.5);
        for id in ["clock-card", "now-playing-mini", "resource-overview", "focus-hub"] {
            let widget = samples::get(id).unwrap_or_else(|| panic!("missing shared builtin:{id}"));
            assert!(widget.commands.is_empty());
            for missing in [false, true] {
                let mut data = data::sample();
                if missing {
                    data.vals.clear();
                    data.tags.clear();
                }
                let tree = layout::layout(&widget, &layout::Ctx { data: &data, metrics: &paint::FontMetrics(&fonts), theme_radius: th.radius }, None);
                let dump = layout::dump(&tree);
                if missing && id == "now-playing-mini" {
                    assert!(dump.contains("Nothing playing"));
                    assert!(!dump.contains("PlayPause"));
                }
                if missing && id == "resource-overview" {
                    assert!(dump.contains("Unavailable"));
                }
                let card = paint::render(&widget, &data, &th, &mut fonts, &root().join("widgets"), 0).unwrap();
                assert!(card.width() > 0 && card.height() > 0);
                if let Ok(out) = std::env::var("DECEBAL_DUX_EVIDENCE") {
                    let dir = PathBuf::from(out);
                    fs::create_dir_all(&dir).unwrap();
                    card.save_png(dir.join(format!("{id}-{}-{}.png", if dark { "dark" } else { "light" }, if missing { "missing" } else { "sample" }))).unwrap();
                    fs::write(dir.join(format!("{id}-{}-{}.txt", if dark { "dark" } else { "light" }, if missing { "missing" } else { "sample" })), dump).unwrap();
                }
            }
        }
    }
}

// Process-local theme publication controls native font measurement. This test owns it;
// run the integration binary with --test-threads=1 when collecting evidence.
#[test]
fn dux_placement_uses_native_card_bounds_and_allows_accessible_new_cards() {
    use decebal::widgets::config::place_clear;
    let def = Def::parse(include_str!("../themes/dux-lavender/theme.toml")).unwrap();
    let mut fonts = decebal::text::Fonts::load().unwrap();
    for scale in [1.0, 2.0] {
        for dark in [true, false] {
            let th = decebal::accessibility::Preferences { font_scale: Some(scale), ..Default::default() }.apply(def.resolve("dux-lavender", dark, &Selection::default()));
            decebal::theme::live::publish_theme(th.clone());
            let mut cards = Vec::new();
            for spec in def.extra["widgets"].as_array().unwrap() {
                let id = spec["use"].as_str().unwrap().strip_prefix("builtin:").unwrap();
                let widget = samples::get(id).unwrap();
                let card = paint::render(&widget, &data::sample(), &th, &mut fonts, &root().join("widgets"), 0).unwrap();
                if scale > 1.0 {
                    if let Ok(out) = std::env::var("DECEBAL_DUX_EVIDENCE") {
                        card.save_png(PathBuf::from(out).join(format!("{id}-{}-200-sample.png", if dark { "dark" } else { "light" }))).unwrap();
                    }
                }
                cards.push((spec, card.width() as i32, card.height() as i32, card));
            }
            for (width, height) in [(1920, 1080), (1280, 720)] {
                let rects: Vec<_> = cards
                    .iter()
                    .map(|(spec, w, h, _)| {
                        let (x, y) =
                            place_clear(spec["anchor"].as_str().unwrap(), spec["x"].as_i64().unwrap() as i32, spec["y"].as_i64().unwrap() as i32, (0, 0, width, height), *w, *h, th.bar_insets());
                        assert!(x >= 0 && y >= th.bar_insets().top && x + w <= width && y + h <= height);
                        (x, y, x + w, y + h)
                    })
                    .collect();
                if let Ok(out) = std::env::var("DECEBAL_DUX_EVIDENCE") {
                    let mut canvas = decebal::gfx::pixmap(width as u32, height as u32).unwrap();
                    canvas.fill(th.background.color(1.0));
                    for ((_, _, _, card), r) in cards.iter().zip(&rects) {
                        canvas.draw_pixmap(r.0, r.1, card.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
                    }
                    let stem = format!("combined-{width}x{height}-{}-{}-fixed-sample", if dark { "dark" } else { "light" }, (scale * 100.0) as u32);
                    canvas.save_png(PathBuf::from(&out).join(format!("{stem}.png"))).unwrap();
                    fs::write(PathBuf::from(out).join(format!("{stem}.json")),serde_json::to_string_pretty(&json!({"data":"fixed samples; native widgets only, no live host/compositor","text_scale":scale,"rectangles":cards.iter().zip(&rects).map(|((spec,_,_,_),r)|json!({"widget":spec["use"],"rect":r})).collect::<Vec<_>>()})).unwrap()).unwrap();
                }
                for (i, a) in rects.iter().enumerate() {
                    for b in rects.iter().skip(i + 1) {
                        assert!(a.2 <= b.0 || b.2 <= a.0 || a.3 <= b.1 || b.3 <= a.1, "card overlap at {width}x{height},scale{scale}: {a:?}/{b:?}");
                    }
                }
            }
        }
    }
    decebal::theme::live::publish_theme(def.resolve("dux-lavender", true, &Selection::default()));
}

#[test]
fn dux_follow_theme_choices_resolve_to_reviewed_originals() {
    use decebal::settings::wallhaven::curated;
    let choices = curated::choices("dux-lavender").unwrap();
    assert_eq!(choices.len(), 2);
    for (mode, id, sha) in
        [("dark", "l3edjp", "931e8e461a521afe0eb53438358a487b7dd6b40f62adc2ad83df32d6571e7237"), ("light", "39zjq6", "b6a4fbaaaf5719f87b69d9911b0192ad8b6b493e9f1de77475c00815fba77b0c")]
    {
        assert!(choices.iter().any(|c| c.mode == mode && c.wall.id == id));
        assert_eq!(curated::reviewed_digest(id), Some(sha));
    }
}
