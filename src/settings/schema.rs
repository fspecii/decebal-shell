//! The key table of rdwm's config (types, ranges, defaults, descriptions).
//!
//! Source of truth is rdwm itself: `rdwmctl schema --offline` prints the table generated next to the
//! parser. A frozen copy (`rdwm-schema.json`) is the fallback when rdwmctl is not installed; a test
//! explicitly compares the snapshot with a paired rdwmctl when requested, and another checks that
//! every key the panes use exists in the table. Unit tests do not depend on the host's installed version.

use serde_json::Value;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct KeyMeta {
    pub section: String,
    pub key: String,
    pub ty: String,
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub choices: Vec<String>,
    pub desc: String,
}

#[derive(Debug, Clone, Default)]
pub struct Schema {
    pub keys: Vec<KeyMeta>,
    /// (name, bar symbol)
    pub layouts: Vec<(String, String)>,
    pub actions: Vec<String>,
    pub live: bool,
}

const SNAPSHOT: &str = include_str!("rdwm-schema.json");

impl Schema {
    pub fn from_json(v: &Value) -> Option<Schema> {
        let keys = v["keys"]
            .as_array()?
            .iter()
            .filter_map(|k| {
                Some(KeyMeta {
                    section: k["section"].as_str()?.to_string(),
                    key: k["key"].as_str()?.to_string(),
                    ty: k["type"].as_str()?.to_string(),
                    default: k["default"].clone(),
                    min: k["min"].as_f64(),
                    max: k["max"].as_f64(),
                    choices: k["choices"].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default(),
                    desc: k["desc"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();
        let strs = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default();
        let layouts = v["layouts"].as_array().map(|a| a.iter().filter_map(|l| Some((l["name"].as_str()?.to_string(), l["symbol"].as_str()?.to_string()))).collect()).unwrap_or_default();
        Some(Schema { keys, layouts, actions: strs("actions"), live: false })
    }

    pub fn snapshot() -> Schema {
        serde_json::from_str(SNAPSHOT).ok().and_then(|v: Value| Schema::from_json(&v)).expect("bundled schema is valid")
    }

    /// Asks the installed rdwmctl; falls back to the bundled snapshot.
    pub fn load(rdwmctl: &Path) -> Schema {
        let live = Command::new(rdwmctl)
            .args(["schema", "--offline"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
            .and_then(|v| Schema::from_json(&v));
        match live {
            Some(mut s) if !s.keys.is_empty() => {
                s.live = true;
                s
            }
            _ => Schema::snapshot(),
        }
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&KeyMeta> {
        self.keys.iter().find(|k| k.section == section && k.key == key)
    }

    pub fn has(&self, section: &str, key: &str) -> bool {
        self.get(section, key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SchemaFixture(std::path::PathBuf);

    impl SchemaFixture {
        fn new(label: &str) -> Self {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("decebal-schema-{label}-{}-{stamp}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for SchemaFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn snapshot_parses_and_has_the_basics() {
        let s = Schema::snapshot();
        assert!(s.keys.len() > 60 && s.layouts.len() >= 20 && s.actions.contains(&"spawn".to_string()));
        let m = s.get("general", "mfact").unwrap();
        assert_eq!((m.min, m.max), (Some(0.05), Some(0.95)));
        assert_eq!(s.get("decoration", "style").unwrap().choices.len(), 24);
        for id in ["adwaita", "xfce-default", "xfce-daloa", "xfce-kokodi", "openkylin", "openkylin-dark", "deepin", "deepin-dark", "lingmo", "lingmo-dark"] {
            assert!(s.get("decoration", "style").unwrap().choices.contains(&id.to_string()), "{id}");
        }
    }

    #[test]
    fn an_older_live_schema_is_authoritative_instead_of_gaining_snapshot_capabilities() {
        use std::os::unix::fs::PermissionsExt;
        let dir = SchemaFixture::new("old");
        let tool = dir.0.join("rdwmctl");
        let old = serde_json::json!({"keys":[{"section":"general","key":"mfact","type":"float","default":0.6,"min":0.05,"max":0.95}],"layouts":[],"actions":[]});
        std::fs::write(&tool, format!("#!/bin/sh\n[ \"$1\" = schema ] && [ \"$2\" = --offline ] || exit 9\nprintf '%s' '{}'\n", old)).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
        let schema = Schema::load(&tool);
        assert!(schema.live);
        assert_eq!(schema.get("general", "mfact").unwrap().default, serde_json::json!(0.6));
        assert!(!schema.has("input", "natural_scroll"), "an old binary must not gain newer snapshot controls");
        assert!(Schema::snapshot().has("input", "natural_scroll"), "fixture must exercise the version difference");
    }

    #[test]
    fn an_unavailable_tool_uses_the_bundled_fallback() {
        let dir = SchemaFixture::new("missing");
        // The fresh directory contains no executable; never search the host PATH.
        let schema = Schema::load(&dir.0.join("not-installed"));
        assert!(!schema.live);
        assert_eq!(schema.keys, Schema::snapshot().keys);
    }

    /// Opt in with an explicit binary paired with this snapshot. An arbitrary installed version is not a reference.
    /// Run: RDWMCTL_SCHEMA_REFERENCE=/absolute/rdwmctl cargo test --lib snapshot_matches_paired_rdwmctl -- --ignored
    #[test]
    #[ignore = "integration: requires RDWMCTL_SCHEMA_REFERENCE pointing at a paired rdwmctl"]
    fn snapshot_matches_paired_rdwmctl() {
        let reference = std::env::var_os("RDWMCTL_SCHEMA_REFERENCE").expect("set RDWMCTL_SCHEMA_REFERENCE to the paired binary");
        assert!(Path::new(&reference).is_absolute(), "reference must be an explicit absolute path");
        let out = Command::new(reference).args(["schema", "--offline"]).output().expect("run paired rdwmctl");
        assert!(out.status.success(), "paired rdwmctl failed");
        let live: Value = serde_json::from_slice(&out.stdout).expect("paired rdwmctl returned schema JSON");
        let snap: Value = serde_json::from_str(SNAPSHOT).unwrap();
        let keyset = |v: &Value| v["keys"].as_array().expect("schema keys").iter().map(|k| format!("{}.{}", k["section"], k["key"])).collect::<std::collections::BTreeSet<_>>();
        let (a, b) = (keyset(&live), keyset(&snap));
        assert!(b.is_subset(&a), "paired rdwmctl is missing snapshot keys: {:?}", b.difference(&a).collect::<Vec<_>>());
    }
}
