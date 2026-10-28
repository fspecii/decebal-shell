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
    for b in ["macsession", "macutil"] {
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
    for f in ["macshell-hot.desktop", "macshell-hot.service"] {
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("contrib").join(f).exists(), "contrib/{f} missing");
        assert!(make.contains(f), "Makefile does not install {f}");
        assert!(pkg.contains(f), "PKGBUILD does not install {f}");
    }
    assert!(read("contrib/macshell-hot.desktop").contains("Exec=machot daemon"));
    assert!(cargo_bins().contains("machot"));
}

#[test]
fn macexpose_is_registered_everywhere() {
    let doc: toml_edit::DocumentMut = read("Cargo.toml").parse().expect("Cargo.toml parses");
    let bin = doc["bin"].as_array_of_tables().unwrap().iter().find(|t| t.get("name").and_then(|n| n.as_str()) == Some("macexpose")).expect("macexpose [[bin]]");
    assert_eq!(bin["path"].as_str(), Some("src/bin/macexpose.rs"));
    let features: Vec<&str> = bin["required-features"].as_array().expect("required-features").iter().filter_map(|f| f.as_str()).collect();
    assert_eq!(features, ["x11"], "it is an X11 popup");
    assert!(listed(&read("Makefile"), "BINS =").contains("macexpose"));
    assert!(listed(&read("PKGBUILD"), "for bin in").contains("macexpose"));
    let readme = read("README.md");
    assert!(readme.contains("## Window overview (`macexpose`)") && readme.contains("macexpose --all"), "README section");
    assert!(readme.contains("tests/e2e/expose.sh"));
    assert!(std::path::Path::new(&format!("{}/tests/e2e/expose.sh", env!("CARGO_MANIFEST_DIR"))).exists(), "e2e script");
}

#[test]
fn the_caelestia_style_is_documented_and_has_its_e2e() {
    let readme = read("README.md");
    assert!(readme.contains("## Caelestia style (pill bar)") && readme.contains("docs/caelestia.md"), "README section");
    assert!(readme.contains("tests/e2e/vbar.sh") && readme.contains("tests/e2e/layouts.sh"));
    let doc = read("docs/caelestia.md");
    for needle in ["bar_style = \"vertical\"", "dock.enabled", "macspot", "macdash", "macsession", "macutil", "machud", "macnotify", "_NET_WM_STRUT_PARTIAL", "[[bind]]"] {
        assert!(doc.contains(needle), "docs/caelestia.md does not mention {needle}");
    }
    assert!(read("Makefile").contains("e2e-vbar:") && read("Makefile").contains("e2e-layouts:"), "Makefile targets");
    assert!(std::path::Path::new(&format!("{}/tests/e2e/vbar.sh", env!("CARGO_MANIFEST_DIR"))).exists(), "e2e script");
    // the theme that uses the layout
    assert!(read("src/theme/builtin/caelestia.toml").contains("bar_style = \"vertical\""));
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
    for needle in ["bar.position", "macthemectl layout", "caelestia-top", "--role bottom", "macbar-bottom", "Layout"] {
        assert!(doc.contains(needle), "docs/caelestia.md does not mention {needle}");
    }
    assert!(readme.contains("macthemectl layout") && readme.contains("\"position\""), "README: command and setting");
    // the --help texts name the commands, the setting values and the role
    let ctl = read("src/bin/macthemectl.rs");
    assert!(ctl.contains("macthemectl layout [left|right|top|bottom|split]") && ctl.contains("caelestia[-left|-right|-top|-bottom|-split]"), "macthemectl --help");
    let bar = read("src/bin/macbar.rs");
    assert!(bar.contains("--role main|bottom") && bar.contains("--position POS") && bar.contains("split"), "macbar --help");
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
    assert!(read("src/bin/macthemectl.rs").contains("macthemectl icons"));
}
