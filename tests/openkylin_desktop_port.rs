//! Offline contract of the openKylin packages' mode-aware window style (`themes/openkylin*`). Needs an engine with the generic mode-specific window-style
//! contract (`decoration_compat`, Lingmo lane commit ca418f2); where the schema lacks `decoration.style_light` the assertions are skipped with a message,
//! the way the Plasma port gates its engine-dependent checks. No display, network, live config or process.
use decebal::theme::engine::{decoration_compat, schema};
use std::path::Path;

const IDS: [&str; 5] = ["openkylin", "openkylin-islands", "openkylin-light-seeking", "openkylin-heyin", "openkylin-classic"];

fn manifest(id: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("themes").join(id).join("theme.toml")).unwrap()
}

/// The `[decoration]` table as the engine sees it (strings and integers only).
fn decoration(id: &str) -> serde_json::Map<String, serde_json::Value> {
    let doc: toml_edit::DocumentMut = manifest(id).parse().unwrap();
    let mut t = serde_json::Map::new();
    for (k, v) in doc["decoration"].as_table().unwrap().iter() {
        let v = v.as_value().unwrap();
        t.insert(k.into(), v.as_str().map(serde_json::Value::from).or_else(|| v.as_integer().map(serde_json::Value::from)).unwrap());
    }
    t
}

fn extra(id: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert("decoration".into(), decoration(id).into());
    m
}

#[test]
fn every_openkylin_package_wants_the_native_pair_by_mode_with_same_mode_fallbacks() {
    if schema::field("decoration.style_light").is_none() {
        eprintln!("skipped: this engine has no mode-specific window-style contract yet");
        return;
    }
    for id in IDS {
        let m = extra(id);
        assert_eq!(decoration_compat::wanted(&m, false), Some(decoration_compat::Wanted { desired: "openkylin".into(), fallback: Some("adwaita-light".into()) }), "{id} light");
        assert_eq!(decoration_compat::wanted(&m, true), Some(decoration_compat::Wanted { desired: "openkylin-dark".into(), fallback: Some("adwaita".into()) }), "{id} dark");
        let text = manifest(id);
        assert!(text.contains("min_shell_version = \"0.3.0\""), "{id}: the mode keys need the framework floor");
        // a theme height/radius would override the native geometry (38 px / 12) of whichever style is in use: only Classic (window-radius 0) sets a radius
        let d = decoration(id);
        assert!(d.get("height").is_none(), "{id}: native header height must come from the style");
        assert_eq!(d.get("radius").and_then(|r| r.as_i64()), (id == "openkylin-classic").then_some(0), "{id}");
    }
}
