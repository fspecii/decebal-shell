//! Glyph's package contract; all application checks inspect plans in a private folder.
use decebal::settings::io::Paths;
use decebal::theme::{
    engine::{apply, archive::Limits, package},
    live::Source,
    model::{Mode, Selection},
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes/glyph-rice")
}

#[test]
fn glyph_package_is_safe_and_packs_reproducibly() {
    let entries = package::read_dir(&root(), &Limits::default()).unwrap();
    let report = package::check(&entries, &Limits::default());
    assert!(report.ok(), "{:?}", report.errors());
    let (archive, packed) = package::pack_dir(&root(), &Limits::default()).unwrap();
    assert!(packed.ok(), "{:?}", packed.errors());
    assert_eq!(archive, package::pack_dir(&root(), &Limits::default()).unwrap().0);
}

#[test]
fn glyph_restores_colours_preserves_widgets_and_selects_variant_art() {
    let dir = std::env::temp_dir().join(format!("glyph-contract-{}", std::process::id()));
    fs::create_dir_all(dir.join("shell")).unwrap();
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: "/nonexistent/glyph-rdwmctl".into(), socket: dir.join("no.sock") };
    let src = Source { shell_json: paths.shell_json(), themes_dir: root().parent().unwrap().to_path_buf() };
    let widgets = json!({"enabled":false,"future":42,"items":[{"use":"builtin:clock-card","enabled":false,"x":123,"options":{"private":"retain"}}]});
    fs::write(paths.shell_json(), json!({"theme":{"name":"nord","palette":"nord"},"widgets":widgets,"unrelated":123}).to_string()).unwrap();
    for (mode, bg, fg, artwork) in [(Mode::Dark, "#000000", "#f2f2f2", "dark.png"), (Mode::Light, "#e9e9e9", "#0b0b0b", "light.png")] {
        let base = Selection { mode, palette: Some("nord".into()), ..Selection::default() };
        let p = apply::plan_with_widgets(&src, &paths, "glyph-rice", &base, apply::WidgetChoice::Keep).unwrap();
        assert!(p.selection.palette.is_none());
        let theme = src.build(&p.selection, true).0;
        assert_eq!(theme.background, decebal::theme::Rgb::parse(bg).unwrap());
        assert_eq!(theme.text, decebal::theme::Rgb::parse(fg).unwrap());
        assert!(theme.bar_vertical());
        let shell: Value = serde_json::from_str(&p.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
        assert_eq!(shell["widgets"]["items"], widgets["items"]);
        assert_eq!(shell["widgets"]["enabled"], widgets["enabled"]);
        assert_eq!(shell["widgets"]["future"], 42);
        assert_eq!(shell["unrelated"], 123);
        assert_eq!(shell["dock"]["enabled"], false);
        assert_eq!(shell["bar"]["position"], "left");
        assert!(p.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new.contains(artwork));
    }
    fs::remove_dir_all(dir).unwrap();
}
