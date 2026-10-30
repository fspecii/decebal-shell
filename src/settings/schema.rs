//! The key table of rdwm's config (types, ranges, defaults, descriptions).
//!
//! Source of truth is rdwm itself: `rdwmctl schema --offline` prints the table generated next to the
//! parser. A frozen copy (`rdwm-schema.json`) is the fallback when rdwmctl is not installed; a test
//! compares the snapshot with the installed rdwmctl when one is found, and another one checks that
//! every key the panes use exists in the table.

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

    #[test]
    fn snapshot_parses_and_has_the_basics() {
        let s = Schema::snapshot();
        assert!(s.keys.len() > 60 && s.layouts.len() >= 20 && s.actions.contains(&"spawn".to_string()));
        let m = s.get("general", "mfact").unwrap();
        assert_eq!((m.min, m.max), (Some(0.05), Some(0.95)));
        assert_eq!(s.get("decoration", "style").unwrap().choices.len(), 18);
        for id in ["adwaita", "xfce-default", "xfce-daloa", "xfce-kokodi"] {
            assert!(s.get("decoration", "style").unwrap().choices.contains(&id.to_string()), "{id}");
        }
    }

    /// When an rdwmctl is reachable (RDWMCTL env, PATH or the sibling rdwm worktree) the bundled
    /// snapshot must equal what the parser's own table says; regenerate it otherwise:
    /// `rdwmctl schema --offline > src/settings/rdwm-schema.json`.
    #[test]
    fn snapshot_matches_installed_rdwmctl_if_any() {
        let candidates = [
            std::env::var_os("RDWMCTL").map(Into::into),
            Some(std::path::PathBuf::from("rdwmctl")),
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Projects/rdwm-settings/target/debug/rdwmctl")),
        ];
        for c in candidates.into_iter().flatten() {
            let Some(out) = Command::new(&c).args(["schema", "--offline"]).output().ok().filter(|o| o.status.success()) else { continue };
            let Ok(live) = serde_json::from_slice::<Value>(&out.stdout) else { continue };
            if live["keys"].as_array().is_some_and(|k| k.iter().any(|x| x["section"] == "decoration")) {
                let snap: Value = serde_json::from_str(SNAPSHOT).unwrap();
                let keyset = |v: &Value| v["keys"].as_array().unwrap().iter().map(|k| format!("{}.{}", k["section"], k["key"])).collect::<std::collections::BTreeSet<_>>();
                let (a, b) = (keyset(&live), keyset(&snap));
                // Newer rdwm may add keys (the app then shows them only after the snapshot is refreshed);
                // keys that vanished would break the panes.
                assert!(b.is_subset(&a), "snapshot has keys rdwm no longer knows: {:?}", b.difference(&a).collect::<Vec<_>>());
                return;
            }
        }
        eprintln!("(no rdwmctl found: snapshot not cross-checked)");
    }
}
