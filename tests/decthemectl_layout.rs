//! `decthemectl layout` and the `caelestia-top` style words, run as the real binary in a private config folder.

use std::path::{Path, PathBuf};
use std::process::Command;

fn sandbox(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("decebal-ctl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cfg/omarchy")).unwrap();
    std::fs::create_dir_all(d.join("home")).unwrap();
    d
}

fn ctl(d: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_decthemectl"))
        .args(args)
        .env("DECEBAL_SYSTEM_AUTOMATIC", "0")
        .env("HOME", d.join("home"))
        .env("XDG_CONFIG_HOME", d.join("cfg"))
        .env("DECEBAL_CONFIG", d.join("cfg/omarchy"))
        .env("DECEBAL_THEMES", d.join("cfg/themes"))
        .env("DECSETTINGS_CONFIG", d.join("cfg/rdwm.toml"))
        .env("RDWMCTL", "/nonexistent/rdwmctl")
        .env("DISPLAY", "")
        .env("WAYLAND_DISPLAY", "")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/decebal-test-bus")
        .env("XDG_RUNTIME_DIR", d.join("run"))
        .env_remove("RDWM_SOCK")
        .output()
        .expect("decthemectl runs");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string(), String::from_utf8_lossy(&out.stderr).trim().to_string())
}

fn shell_json(d: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(d.join("cfg/omarchy/decebal.json")).unwrap_or_else(|_| "{}".into())).unwrap()
}

#[test]
fn layout_prints_and_sets_the_position_and_rejects_nonsense() {
    let d = sandbox("layout");
    assert_eq!(ctl(&d, &["layout"]), (true, "left".into(), String::new()), "left by default");
    for p in ["right", "top", "bottom", "split", "left"] {
        let (ok, out, err) = ctl(&d, &["layout", p]);
        assert!(ok, "{p}: {err}");
        assert!(out.starts_with(&format!("layout {p}:")), "{out}");
        assert_eq!(ctl(&d, &["layout"]).1, p);
        assert_eq!(shell_json(&d)["bar"]["position"], p);
    }
    let (ok, _, err) = ctl(&d, &["layout", "diagonal"]);
    assert!(!ok && err.contains("left, right, top, bottom or split"), "{err}");
    let (ok, _, err) = ctl(&d, &["layout", "top", "extra"]);
    assert!(!ok && err.contains("unexpected argument"), "{err}");
    assert_eq!(shell_json(&d)["bar"]["position"], "left", "a refused word changes nothing");
    // without the Caelestia style it says when the setting shows
    let (_, out, _) = ctl(&d, &["layout", "bottom"]);
    assert!(out.contains("Caelestia style"), "{out}");
}

#[test]
fn style_words_with_a_layout_set_theme_and_position_and_plain_caelestia_keeps_it() {
    let d = sandbox("style");
    let (ok, out, err) = ctl(&d, &["style", "caelestia-top", "--no-rdwm"]);
    assert!(ok, "{err}");
    assert!(out.contains("style caelestia") && out.contains("top edge"), "{out}");
    let j = shell_json(&d);
    assert_eq!((j["bar"]["position"].as_str(), j["theme"]["name"].as_str().or(j["theme"].as_str())), (Some("top"), Some("caelestia")));
    assert_eq!(j["dock"]["enabled"], "auto");
    assert_eq!(ctl(&d, &["style"]).1, "caelestia");
    // macOS and plain Caelestia keep the layout
    assert!(ctl(&d, &["style", "macos", "--no-rdwm"]).0);
    assert_eq!(ctl(&d, &["layout"]).1, "top");
    let (_, out, _) = ctl(&d, &["style", "caelestia", "--no-rdwm"]);
    assert!(out.contains("top edge"), "{out}");
    for (word, pos) in [("caelestia-left", "left"), ("caelestia-right", "right"), ("caelestia-bottom", "bottom"), ("caelestia-split", "split"), ("caelestia_top", "top")] {
        assert!(ctl(&d, &["style", word, "--no-rdwm"]).0, "{word}");
        assert_eq!(ctl(&d, &["layout"]).1, pos, "{word}");
    }
    let (ok, _, err) = ctl(&d, &["style", "macos-top"]);
    assert!(!ok && err.contains("unknown style"), "{err}");
    let (ok, _, _) = ctl(&d, &["style", "caelestia-diagonal"]);
    assert!(!ok);
    // --help names both
    let help = ctl(&d, &["--help"]).1;
    assert!(help.contains("decthemectl layout [left|right|top|bottom|split]") && help.contains("caelestia[-left|-right|-top|-bottom|-split]"), "{help}");
}
