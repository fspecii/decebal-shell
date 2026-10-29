//! Theme port safety, native rendering and honest unavailable-state evidence.
//! No live providers, display, writes to user config or authentication calls.
use decebal::{
    settings::io::Paths,
    text::{Fonts, Weight},
    theme::{
        builtin,
        engine::{apply, archive::Limits, package},
        live::Source,
        model::{Mode, Selection},
    },
    widgets::{
        data, dsl,
        layout::{self, Item, Leaf, Metrics},
        paint, Trust,
    },
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const PORTS: &[&str] = &["mountain-neon-dot", "glyph-rice", "sweeteh"];

fn root(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("themes").join(id)
}

fn texts(it: &Item, out: &mut Vec<String>) {
    match &it.leaf {
        Leaf::Text { s, .. } | Leaf::DotText { s, .. } => out.push(s.clone()),
        _ => (),
    }
    for child in &it.children {
        texts(child, out);
    }
}

fn fits(it: &Item, fonts: &Fonts, bounds: (f32, f32)) {
    let r = it.rect;
    assert!(r.x >= -0.1 && r.y >= -0.1 && r.x + r.w <= bounds.0 + 0.1 && r.y + r.h <= bounds.1 + 0.1, "{} outside widget: {r:?} in {bounds:?}", it.kind);
    if let Leaf::Text { s, size, bold, .. } = &it.leaf {
        let w = fonts.measure(s, *size, if *bold { Weight::Bold } else { Weight::Regular });
        assert!(w <= r.w + 1.0, "text {s:?} clips: {w} > {}", r.w);
        assert!(paint::FontMetrics(fonts).line_height(*size) <= r.h + 1.0, "text {s:?} vertically clipped");
    }
    for child in &it.children {
        fits(child, fonts, bounds);
    }
}

#[test]
fn port_packages_pack_and_keep_custom_layout_choices() {
    for id in PORTS {
        let path = root(id);
        let (a, report) = package::pack_dir(&path, &Limits::default()).unwrap();
        assert!(report.ok(), "{:?}", report.errors());
        assert_eq!(a, package::pack_dir(&path, &Limits::default()).unwrap().0);
        let dir = std::env::temp_dir().join(format!("theme-port-{}-{id}", std::process::id()));
        fs::create_dir_all(dir.join("shell")).unwrap();
        let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("no.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: path.parent().unwrap().into() };
        let custom = json!({"enabled":true,"items":[{"use":"builtin:clock-card","x":321,"options":{"private":"retain"}}]});
        fs::write(paths.shell_json(), json!({"theme":{"name":"nord"},"widgets":custom}).to_string()).unwrap();
        let before = fs::read(paths.shell_json()).unwrap();
        for mode in [Mode::Dark, Mode::Light] {
            let sel = Selection { mode, palette: Some("nord".into()), ..Selection::default() };
            assert!(apply::plan(&src, &paths, id, &sel).unwrap_err().contains("custom layout"));
            for choice in [apply::WidgetChoice::Keep, apply::WidgetChoice::Replace] {
                let p = apply::plan_with_widgets(&src, &paths, id, &sel, choice).unwrap();
                assert!(p.selection.palette.is_none());
                let shell: Value = serde_json::from_str(&p.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
                if choice == apply::WidgetChoice::Keep {
                    assert_eq!(shell["widgets"]["items"], custom["items"]);
                } else {
                    assert_eq!(shell["widgets"]["items"].as_array().unwrap().len(), if *id == "sweeteh" { 8 } else { 6 });
                }
                assert_eq!(shell["dock"]["enabled"], false);
                let config = &p.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new;
                let artwork = if *id == "sweeteh" {
                    if mode == Mode::Dark {
                        "julia-ly-pD0yNIuaLho-unsplash.jpg"
                    } else {
                        "jack-b-JUvlf3OT6kc-unsplash.jpg"
                    }
                } else if mode == Mode::Dark {
                    "dark.png"
                } else {
                    "light.png"
                };
                assert!(config.contains(artwork));
                assert!(!config.contains("buttons = \"none\""));
            }
        }
        assert_eq!(fs::read(paths.shell_json()).unwrap(), before, "plans and cancelled review do not write");
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn native_port_widgets_fit_and_keep_absent_sources_honest() {
    let mut fonts = Fonts::load().expect("native render needs an installed fallback font");
    for id in PORTS {
        let package = root(id);
        let mut files: Vec<_> = fs::read_dir(package.join("widgets")).unwrap().map(|p| p.unwrap().path()).collect();
        files.sort();
        for mode in [Mode::Dark, Mode::Light] {
            let th = builtin::def(id).unwrap().resolve(id, mode == Mode::Dark, &Selection { mode, ..Selection::default() });
            for empty in [false, true] {
                let mut d = data::sample();
                if empty {
                    d.vals.retain(|key, _| key.starts_with("clock."));
                    d.tags.clear();
                }
                for file in &files {
                    let def = dsl::parse(&fs::read_to_string(file).unwrap(), Trust::Theme).unwrap();
                    let tree = layout::layout(&def, &layout::Ctx { data: &d, metrics: &paint::FontMetrics(&fonts), theme_radius: th.radius }, None);
                    fits(&tree, &fonts, (tree.rect.w, tree.rect.h));
                    let mut content = vec![];
                    texts(&tree, &mut content);
                    if def.id == "summit-lab" {
                        assert!(content.iter().any(|s| s.contains("MONITORING NOT CONFIGURED")));
                        assert_eq!(content.iter().filter(|s| s.contains("— NOT CONFIGURED")).count(), 6);
                    }
                    if empty && def.id == "summit-system" {
                        assert!(content.iter().any(|s| s.contains("Unavailable")));
                    }
                    if empty && def.id == "glyph-weather" {
                        assert!(content.iter().any(|s| s == "Unavailable"));
                    }
                    if empty && def.id == "glyph-player" {
                        assert!(content.iter().any(|s| s == "No active player"));
                        assert!(!layout::dump(&tree).contains("PlayPause"), "absent player must not offer transport controls");
                    }

                    let pm = paint::render(&def, &d, &th, &mut fonts, file.parent().unwrap(), 0).unwrap();
                    assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0));
                    if let Some(out) = std::env::var_os("DECEBAL_THEME_EVIDENCE") {
                        let dir = PathBuf::from(out).join(id);
                        fs::create_dir_all(&dir).unwrap();
                        let mode = if mode == Mode::Dark { "dark" } else { "light" };
                        let state = if empty { "empty" } else { "sample" };
                        pm.save_png(dir.join(format!("{}-{mode}-{state}.png", def.id))).unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn glyph_transport_tracks_mpris_state_without_fabricating_a_player() {
    let def = dsl::parse(&fs::read_to_string(root("glyph-rice").join("widgets/glyph-player.toml")).unwrap(), Trust::Theme).unwrap();
    for (present, playing, icon, controls) in [(true, true, "pause", true), (true, false, "play", true), (false, false, "", false)] {
        let mut d = data::sample();
        d.set("media.present", data::Val::Bool(present));
        d.set("media.playing", data::Val::Bool(playing));
        let tree = layout::layout(&def, &layout::Ctx { data: &d, metrics: &layout::Fixed, theme_radius: 20.0 }, None);
        fn has_control(it: &Item) -> bool {
            it.on_click == Some(decebal::widgets::actions::Action::Media(decebal::widgets::actions::Media::PlayPause)) || it.children.iter().any(has_control)
        }
        fn has_icon(it: &Item, icon: &str) -> bool {
            matches!(&it.leaf, Leaf::Icon {name, ..} if name == icon) || it.children.iter().any(|i| has_icon(i, icon))
        }
        assert_eq!(has_control(&tree), controls);
        if controls {
            assert!(has_icon(&tree, icon));
        }
    }
}
