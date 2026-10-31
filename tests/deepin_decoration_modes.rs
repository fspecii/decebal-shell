//! The engine's own `decoration_compat::wanted` over the real Deepin manifest. Needs the mode-specific window-style contract
//! (Lingmo branch commit ca418f2); a tree without `src/theme/engine/decoration_compat.rs` cannot compile this file, so it is
//! kept apart from `tests/deepin_port.rs` (whose mirror of the rules runs on every engine).
use decebal::theme::engine::decoration_compat::{self, Wanted};
use serde_json::{Map, Value};

fn extra() -> Map<String, Value> {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes/deepin/theme.toml")).unwrap();
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    let table = doc["decoration"].as_table().expect("[decoration]");
    let json: Map<String, Value> = table.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), Value::String(s.into())))).collect();
    let mut m = Map::new();
    m.insert("decoration".into(), Value::Object(json));
    m
}

#[test]
fn the_engine_wants_the_pair_for_both_modes_with_the_older_fallbacks() {
    let m = extra();
    assert_eq!(decoration_compat::wanted(&m, false), Some(Wanted { desired: "deepin".into(), fallback: Some("adwaita-light".into()) }));
    assert_eq!(decoration_compat::wanted(&m, true), Some(Wanted { desired: "deepin-dark".into(), fallback: Some("adwaita".into()) }));
}
