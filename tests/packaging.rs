//! Every binary Cargo builds is installed by the Makefile and the PKGBUILD and described in the README.

use std::collections::BTreeSet;

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{}/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn cargo_bins() -> BTreeSet<String> {
    let doc: toml_edit::DocumentMut = read("Cargo.toml").parse().expect("Cargo.toml parses");
    doc["bin"].as_array_of_tables().expect("[[bin]] tables").iter().filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from)).collect()
}

/// Words of the first `marker` line's list: `BINS = a b c` or `for bin in a b c; do`.
fn listed(text: &str, marker: &str) -> BTreeSet<String> {
    let line = text.lines().find(|l| l.contains(marker)).unwrap_or_else(|| panic!("no line with {marker:?}"));
    let rest = line.split_once(marker).unwrap().1;
    rest.split(';').next().unwrap().split_whitespace().map(String::from).collect()
}

#[test]
fn session_and_util_binaries_are_registered() {
    let bins = cargo_bins();
    for b in ["decsession", "decutil"] {
        assert!(bins.contains(b), "{b} missing from Cargo.toml");
    }
}

#[test]
fn makefile_installs_every_cargo_bin() {
    let make = listed(&read("Makefile"), "BINS =");
    assert_eq!(cargo_bins(), make, "Cargo.toml [[bin]] vs Makefile BINS");
}

#[test]
fn pkgbuild_packages_every_cargo_bin() {
    let pkg = listed(&read("PKGBUILD"), "for bin in");
    assert_eq!(cargo_bins(), pkg, "Cargo.toml [[bin]] vs PKGBUILD package()");
}

#[test]
fn readme_mentions_every_cargo_bin() {
    let readme = read("README.md");
    for b in cargo_bins() {
        assert!(readme.contains(&format!("`{b}`")) || readme.contains(&b), "README does not mention {b}");
    }
}

#[test]
fn hot_corners_autostart_is_installed() {
    let make = read("Makefile");
    let pkg = read("PKGBUILD");
    for f in ["decebal-hot.desktop", "decebal-hot.service"] {
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("contrib").join(f).exists(), "contrib/{f} missing");
        assert!(make.contains(f), "Makefile does not install {f}");
        assert!(pkg.contains(f), "PKGBUILD does not install {f}");
    }
    assert!(read("contrib/decebal-hot.desktop").contains("Exec=dechot daemon"));
    assert!(cargo_bins().contains("dechot"));
}

#[test]
fn decexpose_is_registered_everywhere() {
    let doc: toml_edit::DocumentMut = read("Cargo.toml").parse().expect("Cargo.toml parses");
    let bin = doc["bin"].as_array_of_tables().unwrap().iter().find(|t| t.get("name").and_then(|n| n.as_str()) == Some("decexpose")).expect("decexpose [[bin]]");
    assert_eq!(bin["path"].as_str(), Some("src/bin/decexpose.rs"));
    let features: Vec<&str> = bin["required-features"].as_array().expect("required-features").iter().filter_map(|f| f.as_str()).collect();
    assert_eq!(features, ["x11"], "it is an X11 popup");
    assert!(listed(&read("Makefile"), "BINS =").contains("decexpose"));
    assert!(listed(&read("PKGBUILD"), "for bin in").contains("decexpose"));
    let readme = read("README.md");
    assert!(readme.contains("## Window overview (`decexpose`)") && readme.contains("decexpose --all"), "README section");
    assert!(readme.contains("tests/e2e/expose.sh"));
    assert!(std::path::Path::new(&format!("{}/tests/e2e/expose.sh", env!("CARGO_MANIFEST_DIR"))).exists(), "e2e script");
}

#[test]
fn the_caelestia_style_is_documented_and_has_its_e2e() {
    let readme = read("README.md");
    assert!(readme.contains("## Caelestia style (pill bar)") && readme.contains("docs/caelestia.md"), "README section");
    assert!(readme.contains("tests/e2e/vbar.sh") && readme.contains("tests/e2e/layouts.sh"));
    let doc = read("docs/caelestia.md");
    for needle in ["bar_style = \"vertical\"", "dock.enabled", "decspot", "decdash", "decsession", "decutil", "dechud", "decnotify", "_NET_WM_STRUT_PARTIAL", "[[bind]]"] {
        assert!(doc.contains(needle), "docs/caelestia.md does not mention {needle}");
    }
    assert!(read("Makefile").contains("e2e-vbar:") && read("Makefile").contains("e2e-layouts:"), "Makefile targets");
    assert!(std::path::Path::new(&format!("{}/tests/e2e/vbar.sh", env!("CARGO_MANIFEST_DIR"))).exists(), "e2e script");
    // the theme that uses the layout
    assert!(read("themes/caelestia/theme.toml").contains("style = \"vertical\""));
}

#[test]
fn the_caelestia_layouts_are_documented_everywhere_they_are_user_visible() {
    let positions = ["left", "right", "top", "bottom", "split"];
    let readme = read("README.md");
    let doc = read("docs/caelestia.md");
    for p in positions {
        assert!(readme.contains(&format!("`{p}`")), "README does not describe the {p} layout");
        assert!(doc.contains(&format!("`{p}`")), "docs/caelestia.md does not describe the {p} layout");
    }
    for needle in ["bar.position", "decthemectl layout", "caelestia-top", "--role bottom", "decbar-bottom", "Layout"] {
        assert!(doc.contains(needle), "docs/caelestia.md does not mention {needle}");
    }
    assert!(readme.contains("decthemectl layout") && readme.contains("\"position\""), "README: command and setting");
    // the --help texts name the commands, the setting values and the role
    let ctl = read("src/bin/decthemectl.rs");
    assert!(ctl.contains("decthemectl layout [left|right|top|bottom|split]") && ctl.contains("caelestia[-left|-right|-top|-bottom|-split]"), "decthemectl --help");
    let bar = read("src/bin/decbar.rs");
    assert!(bar.contains("--role main|bottom") && bar.contains("--position POS") && bar.contains("split"), "decbar --help");
    // the end-to-end check and its Makefile target
    let e2e = read("tests/e2e/layouts.sh");
    for p in positions {
        assert!(e2e.contains(&format!("check_layout {p} ")), "layouts.sh does not check {p}");
    }
    assert!(e2e.contains("_NET_WM_STRUT_PARTIAL") && e2e.contains("Xvfb"), "layouts.sh checks struts on a private Xvfb");
}

#[test]
fn icon_themes_are_documented_and_have_their_e2e() {
    let readme = read("README.md");
    assert!(readme.contains("## Icon themes") && readme.contains("docs/icons.md") && readme.contains("tests/e2e/icons.sh"));
    for f in ["docs/icons.md", "contrib/icon-themes.md", "tests/e2e/icons.sh"] {
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(f).exists(), "{f} missing");
    }
    let doc = read("docs/icons.md");
    for key in ["theme", "size_scale", "monochrome_tint", "fallback"] {
        assert!(doc.contains(&format!("`{key}`")), "docs/icons.md does not describe `{key}`");
    }
    assert!(read("contrib/icon-themes.md").contains("papirus-icon-theme"));
    assert!(read("src/bin/decthemectl.rs").contains("decthemectl icons"));
}

#[test]
fn decwidgets_and_the_theme_engine_are_registered_everywhere() {
    let doc: toml_edit::DocumentMut = read("Cargo.toml").parse().expect("Cargo.toml parses");
    let bin = doc["bin"].as_array_of_tables().unwrap().iter().find(|t| t.get("name").and_then(|n| n.as_str()) == Some("decwidgets")).expect("decwidgets [[bin]]");
    assert_eq!(bin["path"].as_str(), Some("src/bin/decwidgets.rs"));
    assert!(listed(&read("Makefile"), "BINS =").contains("decwidgets"));
    assert!(listed(&read("PKGBUILD"), "for bin in").contains("decwidgets"));
    let readme = read("README.md");
    assert!(readme.contains("## Theme packages and widgets") && readme.contains("docs/theme-engine.md") && readme.contains("tests/e2e/theme-engine.sh"));
    for f in ["docs/theme-engine.md", "tests/e2e/theme-engine.sh", "widgets/clock-card.toml", "themes/nord/theme.toml"] {
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(f).exists(), "{f} missing");
    }
    // the asset themes are documented with a screenshot each and installed by make and the PKGBUILD
    let gallery = read("docs/themes.md");
    for id in ["nothing-dot", "nothing-light", "brutalist", "terminal-minimal", "terminal-amber"] {
        assert!(gallery.contains(&format!("decthemectl theme apply {id}")) && gallery.contains(&format!("themes/{id}.png")), "docs/themes.md: {id}");
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("docs/themes/{id}.png")).exists(), "{id} screenshot");
    }
    // every theme folder (manifest, wallpapers, widgets, screenshots) is installed by one generic loop
    assert!(read("Makefile").contains("cd themes && find . -type f") && read("PKGBUILD").contains("cd themes && find . -type f"), "themes are not installed by Makefile/PKGBUILD");
    // every built-in theme is a bundled package
    for id in ["macos", "graphite", "nord", "dracula", "solarized", "windows11", "caelestia", "high-contrast", "caelestia-mocha", "rose-pine", "tokyo-night", "kanagawa", "material-you-dynamic"] {
        assert!(read(&format!("themes/{id}/theme.toml")).starts_with("format = 1"), "{id}");
    }
}

#[test]
fn the_old_mac_names_are_installed_as_deprecated_wrappers() {
    // Programs added after the rename have no old name and get no wrapper.
    let mut bins = cargo_bins();
    for new in ["declock", "decmon"] {
        bins.remove(new);
    }
    assert!(read("Makefile").contains("contrib/compat/mac-compat"), "Makefile does not install the compatibility wrappers");
    assert_eq!(listed(&read("PKGBUILD"), "for b in"), bins, "PKGBUILD wrapper loop vs Cargo.toml [[bin]]");
    assert!(read("PKGBUILD").contains("contrib/compat/mac-compat") && read("PKGBUILD").contains("replaces=('macshell')"));
    let doc = read("docs/RENAME.md");
    for b in &bins {
        let old = format!("mac{}", b.strip_prefix("dec").expect("every binary is dec*"));
        assert!(doc.contains(&old) && doc.contains(b.as_str()), "docs/RENAME.md does not list {old} -> {b}");
    }
}

#[test]
fn a_wrapper_runs_the_new_program_with_the_same_arguments() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("decebal-compat-wrapper-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let put = |name: &str, text: &str| {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    put("decspot", "#!/bin/sh\necho \"decspot ran: $*\"\n");
    put("decebal-session", "#!/bin/sh\necho \"session ran: $*\"\n");
    let wrapper = read("contrib/compat/mac-compat");
    put("macspot", &wrapper);
    put("macshell-session", &wrapper);
    put("mac-compat", &wrapper);
    let run = |name: &str, args: &[&str]| {
        let out = std::process::Command::new(dir.join(name)).args(args).env("PATH", format!("{}:/usr/bin:/bin", dir.display())).output().unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stdout).trim().to_string(), String::from_utf8_lossy(&out.stderr).trim().to_string())
    };
    assert_eq!(run("macspot", &["--search", "a b"]), (Some(0), "decspot ran: --search a b".into(), String::new()), "silent when stderr is not a terminal");
    assert_eq!(run("macshell-session", &["x"]).1, "session ran: x");
    assert_eq!(run("mac-compat", &[]).0, Some(127), "not installed under an old name");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_theme_gallery_lists_every_shipped_package_with_its_screenshot() {
    let doc = read("docs/themes.md");
    for id in ["caelestia-mocha", "rose-pine", "tokyo-night", "kanagawa", "material-you-dynamic"] {
        assert!(doc.contains(&format!("decthemectl theme apply {id}")) && doc.contains(&format!("themes/{id}.png")), "docs/themes.md does not show {id}");
        let shot = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("docs/themes/{id}.png"));
        let len = std::fs::metadata(&shot).unwrap_or_else(|e| panic!("{}: {e}", shot.display())).len();
        assert!(len < 400 * 1024, "{id}.png is {len} bytes");
    }
}
