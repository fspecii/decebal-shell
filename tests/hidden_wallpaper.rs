//! Hidden compatibility package policy; no legacy canonical-output snapshots or runtime changes.
use decebal::{
    settings::{
        io::Paths,
        tomlcfg::{Cfg, Val},
    },
    theme::{
        engine::{apply, archive::Limits, package},
        live::Source,
        model::{Mode, Selection},
        palette,
    },
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
const ALIASES: &[&str] = &["caelestia-mocha", "everforest", "kanagawa", "material-you-dynamic", "nothing-light", "rose-pine", "terminal-amber", "tokyo-night"];

#[test]
fn hidden_packages_validate_and_pack_reproducibly_without_art() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes");
    for id in ALIASES {
        assert!(palette::legacy_theme(id));
        let dir = root.join(id);
        let entries = package::read_dir(&dir, &Limits::default()).unwrap();
        assert!(!entries.iter().any(|e| e.path.starts_with("wallpapers/") || e.path.starts_with("screenshots/")), "{id}");
        let (packed, report) = package::pack_dir(&dir, &Limits::default()).unwrap();
        assert!(report.ok(), "{id}: {:?}", report.errors());
        assert_eq!(packed, package::pack_dir(&dir, &Limits::default()).unwrap().0);
    }
}

#[test]
fn explicit_alias_apply_plans_preserve_independent_wallpaper_and_widgets() {
    let dir = std::env::temp_dir().join(format!("hidden-art-contract-{}", std::process::id()));
    fs::create_dir_all(dir.join("shell")).unwrap();
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("none.sock") };
    let src = Source { shell_json: paths.shell_json(), themes_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes") };
    let before_cfg = "[general]\n[wallpaper]\nimage = \"/private/user-photo.jpg\"\nmode = \"fit\"\ncolor = \"#123456\"\n";
    fs::write(&paths.rdwm_config, before_cfg).unwrap();
    let custom = json!({"enabled":true,"items":[{"use":"builtin:clock-card","x":234}]});
    let before = json!({"theme":{"name":"macos"},"widgets":custom,"wallhaven":{"selection":"retain"}}).to_string();
    fs::write(paths.shell_json(), &before).unwrap();
    for id in ALIASES {
        assert!(!src.load_def(id).unwrap().extra.contains_key("wallpaper"));
        for mode in [Mode::Dark, Mode::Light] {
            let p = apply::plan_with_widgets(&src, &paths, id, &Selection { mode, ..Default::default() }, apply::WidgetChoice::Keep).unwrap();
            let cfg = Cfg::parse(&p.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new).unwrap();
            for (key, value) in [("image", "/private/user-photo.jpg"), ("mode", "fit"), ("color", "#123456")] {
                assert_eq!(cfg.get("wallpaper", key), Some(Val::Str(value.into())), "{id}: {key}");
            }
            let shell: Value = serde_json::from_str(&p.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
            assert_eq!(shell["widgets"]["items"], custom["items"], "{id}");
            assert_eq!(shell["wallhaven"]["selection"], "retain");
            assert!(shell["theme"]["dynamic"].is_null() || shell["theme"]["dynamic"]["enabled"] == false, "{id}: must not opt into extraction from a retired default");
        }
    }
    assert_eq!(fs::read_to_string(&paths.rdwm_config).unwrap(), before_cfg);
    assert_eq!(fs::read_to_string(paths.shell_json()).unwrap(), before);
    fs::remove_dir_all(dir).unwrap();
}
