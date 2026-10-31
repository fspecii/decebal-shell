//! `decwidgets data --json`: one explicit, one-shot snapshot of the resolved `notes.*` and
//! `timer.*` widget fields, for automation and acceptance tests.
//!
//! The values come from the same `data::Live::update` path the widget painter uses, built
//! for only the requested groups. For `notes` and `timer` that path starts no worker,
//! watcher, sampler or command and only reads storage: the notes folder is listed and
//! read, the timer state file is read, nothing is created, locked, claimed or delivered.
//! The output is a snapshot at the time of the call, not a live stream, and nothing is
//! logged; it contains private note titles/excerpts only because the caller asked for them.
use super::data::{self, Data, Live, Val};
use std::collections::BTreeSet;

/// Versioned output schema. Bump on any incompatible field change.
pub const SCHEMA: &str = "decebal-widget-data/1";
/// Groups this command supports: their resolution is a pure read with no background work.
pub const GROUPS: [&str; 2] = ["notes", "timer"];
/// Defensive per-value bound (characters); the painter's own bounds are smaller.
pub const MAX_TEXT: usize = 256;

/// Validate requested groups (empty means all supported groups), keeping a stable order.
pub fn parse_groups(requested: &[String]) -> Result<Vec<&'static str>, String> {
    if requested.is_empty() {
        return Ok(GROUPS.to_vec());
    }
    let mut groups = Vec::new();
    for name in requested {
        let Some(group) = GROUPS.iter().find(|group| **group == name) else {
            return Err(format!("unsupported data group {name:?}; --json supports: {}", GROUPS.join(", ")));
        };
        if !groups.contains(group) {
            groups.push(*group);
        }
    }
    groups.sort_by_key(|group| GROUPS.iter().position(|known| known == group));
    Ok(groups)
}

fn value(v: Option<&Val>) -> serde_json::Value {
    match v {
        Some(Val::Num(n)) if n.is_finite() => serde_json::json!(n),
        Some(Val::Bool(b)) => serde_json::json!(b),
        Some(Val::Text(t)) => {
            let mut text: String = t.chars().take(MAX_TEXT).collect();
            if t.chars().count() > MAX_TEXT {
                text.push('…');
            }
            serde_json::json!(text)
        }
        _ => serde_json::Value::Null,
    }
}

/// The fields of `groups` as resolved values from an already-updated `Data`.
pub fn fields(d: &Data, groups: &[&str]) -> serde_json::Map<String, serde_json::Value> {
    data::SOURCES.iter().filter(|source| groups.iter().any(|group| source.path.split('.').next() == Some(*group))).map(|source| (source.path.to_string(), value(d.get(source.path)))).collect()
}

/// Resolve once through the painter's `Live` source and build the documented object.
pub fn snapshot(groups: &[&str]) -> serde_json::Value {
    let paths: BTreeSet<String> = data::SOURCES.iter().filter(|source| groups.iter().any(|group| source.path.split('.').next() == Some(*group))).map(|source| source.path.to_string()).collect();
    let mut d = Data::empty();
    Live::new(&paths, &[], data::null_waker()).update(&mut d);
    let generated = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_secs());
    serde_json::json!({
        "schema": SCHEMA,
        "snapshot": true,
        "live": false,
        "generated_unix": generated,
        "groups": groups,
        "fields": fields(&d, groups),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_validated_deduplicated_and_ordered() {
        assert_eq!(parse_groups(&[]).unwrap(), ["notes", "timer"]);
        assert_eq!(parse_groups(&["timer".into(), "notes".into(), "timer".into()]).unwrap(), ["notes", "timer"]);
        assert!(parse_groups(&["cpu".into()]).unwrap_err().contains("unsupported data group"));
    }

    #[test]
    fn values_are_typed_bounded_and_literal() {
        let mut d = Data::empty();
        d.set("timer.title", Val::Text("x".repeat(MAX_TEXT + 50)));
        d.set("timer.progress", Val::Num(0.25));
        d.set("notes.title", Val::Text("{cpu.usage} \"quoted\" <b>".into()));
        d.set("notes.count", Val::Num(f64::NAN));
        let out = fields(&d, &["notes", "timer"]);
        assert_eq!(out["timer.title"].as_str().unwrap().chars().count(), MAX_TEXT + 1);
        assert_eq!(out["timer.progress"], serde_json::json!(0.25));
        assert_eq!(out["notes.title"], serde_json::json!("{cpu.usage} \"quoted\" <b>"));
        assert_eq!(out["notes.count"], serde_json::Value::Null);
        assert!(out.keys().all(|key| key.starts_with("notes.") || key.starts_with("timer.")));
        assert_eq!(fields(&d, &["timer"]).len(), 4);
    }
}
