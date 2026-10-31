//! `decthemectl assets` end to end, through the real binary, in a private environment: no display, no bus, no user
//! config, no network. Sources are folders built here (archives are covered by the unit tests in src/desktop_assets).

use serde_json::Value;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(tag: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("decebal-assets-it-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["home", "config", "data/icons", "datadirs", "run", "catalogs"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        Env { dir }
    }

    fn icons(&self) -> PathBuf {
        self.dir.join("data/icons")
    }

    fn run(&self, args: &[&str]) -> (bool, String) {
        let d = &self.dir;
        let out = Command::new(env!("CARGO_BIN_EXE_decthemectl"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", d.join("home"))
            .env("LANG", "C.UTF-8")
            .env("XDG_CONFIG_HOME", d.join("home/.config"))
            .env("XDG_DATA_HOME", d.join("data"))
            .env("XDG_DATA_DIRS", d.join("datadirs"))
            .env("XDG_RUNTIME_DIR", d.join("run"))
            .env("DECEBAL_CONFIG", d.join("config"))
            .env("DECEBAL_ASSET_CATALOGS", d.join("catalogs"))
            .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", d.join("run/no-bus").display()))
            .env("DBUS_SYSTEM_BUS_ADDRESS", format!("unix:path={}", d.join("run/no-bus").display()))
            .env("GSETTINGS_BACKEND", "memory")
            .arg("assets")
            .args(args)
            .output()
            .unwrap();
        (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }

    fn ok(&self, args: &[&str]) -> String {
        let (ok, out) = self.run(args);
        assert!(ok, "decthemectl assets {args:?} failed:\n{out}");
        out
    }

    fn fails(&self, args: &[&str]) -> String {
        let (ok, out) = self.run(args);
        assert!(!ok, "decthemectl assets {args:?} should have failed:\n{out}");
        out
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A one-image Xcursor file (2x2, hotspot 1,1).
fn xcursor() -> Vec<u8> {
    let mut v = Vec::new();
    for x in [0x7275_6358u32, 16, 0x1_0000, 1, 0xfffd_0002, 24, 28, 36, 0xfffd_0002, 24, 1, 2, 2, 1, 1, 0] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    for _ in 0..4 {
        v.extend_from_slice(&0xff00_0000u32.to_le_bytes());
    }
    v
}

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"48\" height=\"48\"><rect width=\"48\" height=\"48\" fill=\"#36a\"/></svg>";

/// A clone-like source: `COPYING`, a build script, and the theme `wave/` with icons, cursors and alias links.
fn source(root: &Path) {
    let w = |rel: &str, data: &[u8]| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    };
    w("COPYING", b"GNU GENERAL PUBLIC LICENSE Version 3\n");
    w("install.sh", b"#!/bin/sh\ntouch \"$HOME/PWNED\"\n");
    std::fs::set_permissions(root.join("install.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    w("wave/index.theme", b"[Icon Theme]\nName=Wave\nInherits=hicolor\nDirectories=apps/48\n\n[apps/48]\nSize=48\nType=Fixed\n");
    w("wave/apps/48/utilities-terminal.svg", SVG.as_bytes());
    w("wave/apps/48/folder.svg", SVG.as_bytes());
    w("wave/cursors/left_ptr", &xcursor());
    symlink("utilities-terminal.svg", root.join("wave/apps/48/terminal.svg")).unwrap();
    symlink("left_ptr", root.join("wave/cursors/default")).unwrap();
}

const IMPORT: &[&str] = &["--provides", "icons,cursors", "--license", "GPL-3.0-or-later", "--license-file", "COPYING", "--path", "wave"];

#[test]
fn a_folder_pack_installs_resolves_is_selected_and_removed() {
    let e = Env::new("folder");
    let src = e.dir.join("src");
    source(&src);
    let s = src.to_str().unwrap();
    let out = e.ok(&[&["import", s][..], IMPORT].concat());
    assert!(out.starts_with("installed: local-wave"), "{out}");
    let t = e.icons().join("wave");
    assert!(t.join("LICENSES/COPYING").is_file() && t.join(".decebal-asset.json").is_file());
    assert!(!t.join("install.sh").exists() && !e.dir.join("home/PWNED").exists(), "nothing from the source is run or installed outside the theme");
    assert_eq!(std::fs::read_link(t.join("cursors/default")).unwrap().to_str(), Some("left_ptr"));
    assert_eq!(e.ok(&["verify"]).trim(), "wave: ok");

    let p: Value = serde_json::from_str(&e.ok(&["preview", "wave", "--json"])).unwrap();
    let icon = p["icons"].as_array().unwrap().iter().find(|i| i["name"] == "utilities-terminal").unwrap();
    assert!(icon["own"].as_bool().unwrap() && icon["path"].as_str().unwrap().ends_with("wave/apps/48/utilities-terminal.svg"));
    assert_eq!(p["cursors"][0]["theme"], "wave", "the pointer resolves in the pack");

    e.ok(&["use", "icons", "wave"]);
    e.ok(&["use", "cursors", "wave"]);
    let st: Value = serde_json::from_str(&e.ok(&["status", "--json"])).unwrap();
    assert_eq!((st["icons"]["theme"].as_str(), st["icons"]["owner"].as_str()), (Some("wave"), Some("your choice")));
    assert_eq!(st["cursors"]["theme"], "wave");
    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(e.dir.join("config/decebal.json")).unwrap()).unwrap();
    assert_eq!((cfg["icons"]["theme"].as_str(), cfg["system"]["cursorTheme"].as_str()), (Some("wave"), Some("wave")));

    assert!(e.fails(&["remove", "wave"]).contains("in use"));
    e.ok(&["use", "icons", "auto"]);
    e.ok(&["use", "cursors", "auto"]);
    std::fs::write(t.join("apps/48/folder.svg"), "<svg>changed</svg>").unwrap();
    assert!(e.fails(&["verify"]).contains("MODIFIED"));
    assert!(e.fails(&[&["import", s][..], IMPORT].concat()).contains("--force restores it"));
    e.ok(&[&["import", s, "--force"][..], IMPORT].concat());
    assert_eq!(e.ok(&["verify"]).trim(), "wave: ok");
    assert!(e.ok(&["remove", "wave"]).starts_with("removed"));
    assert!(!t.exists());
}

#[test]
fn hostile_links_and_foreign_folders_are_refused() {
    let e = Env::new("hostile");
    let src = e.dir.join("src");
    source(&src);
    symlink("/etc/passwd", src.join("wave/apps/48/passwd.svg")).unwrap();
    let s = src.to_str().unwrap();
    assert!(e.fails(&[&["import", s][..], IMPORT].concat()).contains("absolute"));
    std::fs::remove_file(src.join("wave/apps/48/passwd.svg")).unwrap();
    symlink("../../../../../../etc", src.join("wave/apps/48/up")).unwrap();
    assert!(e.fails(&[&["import", s][..], IMPORT].concat()).contains("outside the source"));
    std::fs::remove_file(src.join("wave/apps/48/up")).unwrap();
    assert!(e.fails(&["import", s, "--provides", "icons", "--license", "MIT", "--license-file", "NOPE", "--path", "wave"]).contains("missing"));
    assert_eq!(std::fs::read_dir(e.icons()).unwrap().flatten().filter(|d| !d.file_name().to_string_lossy().starts_with('.')).count(), 0, "nothing installed");
    std::fs::create_dir_all(e.icons().join("wave")).unwrap();
    std::fs::write(e.icons().join("wave/index.theme"), "[Icon Theme]\nName=Mine\n").unwrap();
    assert!(e.fails(&[&["import", s, "--force"][..], IMPORT].concat()).contains("not installed by decebal"));
    assert!(e.fails(&["remove", "wave"]).contains("left alone"));
    assert_eq!(std::fs::read_to_string(e.icons().join("wave/index.theme")).unwrap(), "[Icon Theme]\nName=Mine\n");
}

#[test]
fn a_catalogue_pack_is_checked_against_its_tree_pin() {
    let e = Env::new("catalog");
    let src = e.dir.join("src");
    source(&src);
    let s = src.to_str().unwrap();
    // the tree id of `wave` as decebal reads it (what `git rev-parse HEAD:wave` gives for a clone of this folder)
    let dry: Value = serde_json::from_str(&e.ok(&[&["import", s, "--dry-run", "--json"][..], IMPORT].concat())).unwrap();
    let tree = dry["provenance"]["source"]["tree"].as_str().unwrap().to_string();
    assert_eq!(tree.len(), 40);
    let catalogue = |tree: &str| {
        format!(
            "format = 1\nfamily = \"it\"\n[[source]]\nid = \"it-src\"\nrepo = \"https://example.org/it\"\nrevision = \"main\"\ncommit = \"{}\"\nlicense = \"GPL-3.0-or-later\"\nlicense_files = [\"COPYING\"]\n\
             [[pack]]\nid = \"it-wave\"\nname = \"Wave\"\nprovides = [\"icons\", \"cursors\"]\nsource = \"it-src\"\npath = \"wave\"\ntree = \"{tree}\"\n\
             [[recommend]]\ntheme = \"it-theme\"\nicons = \"it-wave\"\n",
            "0".repeat(40)
        )
    };
    std::fs::write(e.dir.join("catalogs/it.toml"), catalogue(&"1".repeat(40))).unwrap();
    assert!(e.fails(&["install", "it-wave", "--source-dir", s]).contains("tree mismatch"));
    std::fs::write(e.dir.join("catalogs/it.toml"), catalogue(&tree)).unwrap();
    assert!(e.ok(&["catalog", "check", e.dir.join("catalogs/it.toml").to_str().unwrap()]).contains("ok (1 sources, 1 packs, 1 recommendations)"));
    let out = e.ok(&["install", "it-wave", "--source-dir", s]);
    assert!(out.contains("verified: tree yes"), "{out}");
    let info: Value = serde_json::from_str(&e.ok(&["info", "it-wave", "--json"])).unwrap();
    assert_eq!((info["installed"]["origin"].as_str(), info["recommended_by"][0].as_str()), (Some("catalog"), Some("it-theme")));
    assert!(e.ok(&["list"]).lines().any(|l| l.starts_with("it-wave") && l.trim_end().ends_with("installed")));
}
