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
