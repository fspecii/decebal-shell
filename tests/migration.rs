//! The rename from macshell: a real `decthemectl` run in a private HOME copies the old settings (never touching them) and
//! honours the old `MACSHELL_*` environment variables.

use std::path::{Path, PathBuf};
use std::process::Command;

fn sandbox(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("decebal-migr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("home")).unwrap();
    d
}

/// `decthemectl` with only a private HOME (no XDG overrides) and `extra` environment on top.
fn ctl(d: &Path, args: &[&str], extra: &[(&str, &Path)]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_decthemectl"));
    c.args(args).env_clear().env("DECEBAL_SYSTEM_AUTOMATIC", "0").env("PATH", "/usr/bin:/bin").env("HOME", d.join("home")).env("RDWMCTL", "/nonexistent/rdwmctl");
    for (k, v) in extra {
        c.env(k, v);
    }
    let out = c.output().expect("decthemectl runs");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string(), String::from_utf8_lossy(&out.stderr).trim().to_string())
}

fn write(p: PathBuf, text: &str) -> PathBuf {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
    p
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut es: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        es.sort_by_key(|e| e.path());
        for e in es {
            if e.file_type().unwrap().is_dir() {
                walk(&e.path(), out);
            } else {
                out.push((e.path(), std::fs::read(e.path()).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

#[test]
fn first_run_copies_the_old_settings_and_leaves_them_alone() {
    let d = sandbox("copy");
    let home = d.join("home");
    write(home.join(".config/macshell/presets/mine/manifest.toml"), "name = \"mine\"\n");
    write(home.join(".config/macshell/themes/t/theme.toml"), "format = 1\n");
    write(home.join(".local/state/macshell/spotlight-history"), "firefox 3\n");
    write(home.join(".config/omarchy/macshell.json"), r#"{"theme": "nord"}"#);
    write(home.join(".config/omarchy/dock-settings.json"), r#"{"style": "windows"}"#);
    let old_before = (snapshot(&home.join(".config/macshell")), snapshot(&home.join(".local/state/macshell")), std::fs::read(home.join(".config/omarchy/macshell.json")).unwrap());

    let (ok, _, err) = ctl(&d, &["layout"], &[]);
    assert!(ok, "{err}");
    assert!(err.contains("copied") && err.contains("left untouched"), "the copy is announced: {err}");

    let read = |p: &str| std::fs::read_to_string(home.join(p)).unwrap();
    assert_eq!(read(".config/decebal/presets/mine/manifest.toml"), "name = \"mine\"\n");
    assert_eq!(read(".config/decebal/themes/t/theme.toml"), "format = 1\n");
    assert_eq!(read(".local/state/decebal/spotlight-history"), "firefox 3\n");
    assert_eq!(read(".config/omarchy/decebal.json"), r#"{"theme": "nord"}"#);
    assert_eq!(read(".config/omarchy/dock-settings.json"), r#"{"style": "windows"}"#, "unchanged names stay");
    assert!(home.join(".config/decebal/MIGRATED-FROM-MACSHELL.txt").is_file());

    let old_after = (snapshot(&home.join(".config/macshell")), snapshot(&home.join(".local/state/macshell")), std::fs::read(home.join(".config/omarchy/macshell.json")).unwrap());
    assert_eq!(old_before, old_after, "the old folders and macshell.json are byte for byte what they were");

    // a second run is silent and changes nothing, edits to the new copy survive
    write(home.join(".config/omarchy/decebal.json"), r#"{"theme": "dracula"}"#);
    let (ok, _, err) = ctl(&d, &["layout"], &[]);
    assert!(ok && !err.contains("copied"), "{err}");
    assert_eq!(read(".config/omarchy/decebal.json"), r#"{"theme": "dracula"}"#);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_existing_decebal_folder_is_never_overwritten() {
    let d = sandbox("exists");
    let home = d.join("home");
    write(home.join(".config/macshell/presets/mine/manifest.toml"), "old");
    write(home.join(".config/decebal/keep.txt"), "mine");
    let (ok, _, err) = ctl(&d, &["layout"], &[]);
    assert!(ok, "{err}");
    assert!(!home.join(".config/decebal/presets").exists());
    assert_eq!(std::fs::read_to_string(home.join(".config/decebal/keep.txt")).unwrap(), "mine");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn old_environment_variables_are_honoured_and_the_new_ones_win() {
    let d = sandbox("env");
    let (old_cfg, new_cfg) = (d.join("old-cfg"), d.join("new-cfg"));
    std::fs::create_dir_all(&old_cfg).unwrap();
    std::fs::create_dir_all(&new_cfg).unwrap();
    // only the old names: the setting lands where MACSHELL_CONFIG points
    let (ok, out, err) = ctl(&d, &["layout", "bottom"], &[("MACSHELL_CONFIG", &old_cfg), ("MACSETTINGS_CONFIG", &d.join("rdwm.toml"))]);
    assert!(ok, "{err}");
    assert!(out.starts_with("layout bottom:"), "{out}");
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(old_cfg.join("decebal.json")).unwrap()).unwrap();
    assert_eq!(j["bar"]["position"], "bottom");
    // both: DECEBAL_CONFIG wins
    let (ok, _, err) = ctl(&d, &["layout", "top"], &[("MACSHELL_CONFIG", &old_cfg), ("DECEBAL_CONFIG", &new_cfg)]);
    assert!(ok, "{err}");
    assert!(new_cfg.join("decebal.json").is_file());
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(old_cfg.join("decebal.json")).unwrap()).unwrap();
    assert_eq!(j["bar"]["position"], "bottom", "the old folder was not written by the second run");
    let _ = std::fs::remove_dir_all(&d);
}
