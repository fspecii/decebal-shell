//! Read-only newcomer help from the Settings watcher's effective `bind list` snapshot.
//! No file/IPC access or binding command execution belongs in this module.
use super::{binds, io::Runtime};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct Guide {
    pub status: &'static str,
    pub rows: Vec<(&'static str, String)>,
}

const ACTIONS: &[&str] = &["App search", "Applications", "Terminal", "Settings", "Fullscreen", "Float a window", "Screenshot", "Close a window"];

/// These are effective running bindings, not a merge with possibly unapplied file edits.
/// Missing or opaque commands stay unknown rather than inheriting sample defaults.
pub fn guide(rt: &Runtime) -> Guide {
    if !rt.running {
        return Guide { status: "Live shortcuts are unavailable because rdwm is not connected. Shortcuts shows your saved bindings; this guide does not assume sample defaults.", rows: vec![] };
    }
    let Some(list) = rt.binds.as_array() else {
        return Guide { status: "rdwm is connected, but its effective shortcut list is unavailable. Open Shortcuts to inspect bindings; no default keys are assumed.", rows: vec![] };
    };
    let mut found: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for bind in list {
        let Some(keys) = keys(bind) else { continue };
        let action = bind["action"].as_str().unwrap_or("");
        let label = match action {
            "togglefullscreen" | "fullscreen_center" | "fullscreen" => Some("Fullscreen"),
            "togglefloating" | "togglefloat" => Some("Float a window"),
            "close" | "killclient" => Some("Close a window"),
            "spawn" => {
                let args: Option<Vec<&str>> = bind["args"].as_array().and_then(|a| a.iter().map(Value::as_str).collect());
                Some(args.as_deref().and_then(command_label).unwrap_or("Custom launch commands"))
            }
            _ => None,
        };
        if let Some(label) = label {
            let hits = found.entry(label).or_default();
            if !hits.contains(&keys) {
                hits.push(keys);
            }
        }
    }
    let mut rows: Vec<_> = ACTIONS.iter().map(|label| (*label, formatted(found.remove(label).unwrap_or_default()))).collect();
    if let Some(custom) = found.remove("Custom launch commands") {
        rows.push(("Custom launch commands", formatted(custom)));
    }
    Guide {
        status: "Reported by the running rdwm session and refreshed automatically. ‘Not identified’ means no recognized binding, not necessarily unbound: custom scripts are listed separately. Open Shortcuts for all bindings and modes.",
        rows,
    }
}

fn formatted(mut keys: Vec<String>) -> String {
    keys.sort();
    if keys.is_empty() {
        return "Not identified".into();
    }
    let extra = keys.len().saturating_sub(3);
    keys.truncate(3);
    let text = keys.join("  ·  ");
    if extra > 0 {
        format!("{text}  (+{extra} more in Shortcuts)")
    } else {
        text
    }
}

fn keys(bind: &Value) -> Option<String> {
    let spec = match &bind["keys"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(Value::as_str).collect::<Option<Vec<_>>>()?.join(" "),
        _ => return None,
    };
    if spec.trim().is_empty() || spec.len() > 512 || spec.chars().any(char::is_control) {
        return None;
    }
    let normalized = binds::norm_keys(&spec);
    let nice = normalized.split_whitespace().map(binds::nice).collect::<Vec<_>>().join(" → ");
    let mode = bind["mode"].as_str().unwrap_or("default");
    if mode != "default" && !mode.is_empty() {
        if mode.len() > 64 || mode.chars().any(char::is_control) {
            return None;
        }
        Some(format!("{nice} ({mode} mode)"))
    } else {
        Some(nice)
    }
}

fn basename(command: &str) -> Option<&str> {
    Path::new(command).file_name()?.to_str()
}

/// Only argv identities/simple shell execs are recognized. Arbitrary shell programs
/// are opaque; a mention of `decspot` in a comment/string never implies a shortcut.
fn command_label(args: &[&str]) -> Option<&'static str> {
    let (command, rest) = args.split_first()?;
    let program = basename(command)?;
    if matches!(program, "sh" | "bash" | "dash") && rest.len() == 2 && rest[0] == "-c" {
        let script = rest[1];
        if !script.chars().all(|c| c.is_ascii_alphanumeric() || " /._-".contains(c)) {
            return None;
        }
        let words: Vec<_> = script.split_whitespace().collect();
        let words = words.strip_prefix(&["exec"]).unwrap_or(&words);
        return command_label(words);
    }
    match program {
        "decspot" if rest.is_empty() => Some("App search"),
        "declaunch" if rest.is_empty() => Some("Applications"),
        "fuzzel" | "dmenu_run" if rest.is_empty() => Some("App search"),
        "rofi" if rest == ["-show", "drun"] => Some("App search"),
        "wofi" if rest == ["--show", "drun"] => Some("App search"),
        "decsettings" if rest.is_empty() || rest.len() == 1 && !rest[0].starts_with('-') => Some("Settings"),
        "rdwm-terminal" | "foot" | "kitty" | "alacritty" | "ghostty" | "xterm" | "konsole" | "gnome-terminal" if rest.is_empty() => Some("Terminal"),
        "decutil" if rest.first() == Some(&"screenshot") && !rest.contains(&"--help") => Some("Screenshot"),
        "gnome-screenshot" | "grim" | "scrot" | "maim" if !rest.contains(&"--help") => Some("Screenshot"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn runtime(binds: Value) -> Runtime {
        Runtime { running: true, binds, ..Runtime::default() }
    }
    #[test]
    fn effective_custom_bindings_replace_defaults_and_deduplicate() {
        let g = guide(&runtime(json!([
            {"keys":["alt+shift+space"],"action":"spawn","args":["/usr/bin/decspot"],"source":"ipc"},
            {"keys":"shift+alt+space","action":"spawn","args":["decspot"]},
            {"keys":["super+x","r"],"action":"fullscreen_center"},
            {"keys":["super+t"],"action":"spawn","args":["rdwm-terminal"]},
            {"keys":"super+s","action":"spawn","args":["decsettings","welcome"]},
            {"keys":"ctrl+print","action":"spawn","args":["decutil","screenshot","--fullscreen"]},
            {"keys":"f","action":"togglefloating","mode":"resize"}
        ])));
        let row = |label| &g.rows.iter().find(|(l, _)| *l == label).unwrap().1;
        assert_eq!(row("App search"), "Alt+Shift+Space");
        assert_eq!(row("Fullscreen"), "Super+X → R");
        assert_eq!(row("Terminal"), "Super+T");
        assert_eq!(row("Settings"), "Super+S");
        assert_eq!(row("Screenshot"), "Ctrl+Print");
        assert_eq!(row("Float a window"), "F (resize mode)");
        assert!(!row("App search").contains("Super+Space"));
    }
    #[test]
    fn disconnect_unavailable_and_an_empty_authoritative_list_are_distinct() {
        let stale = Runtime { running: false, binds: json!([{"keys":"super+space","action":"spawn","args":["decspot"]}]), ..Runtime::default() };
        assert!(guide(&stale).rows.is_empty());
        assert!(guide(&stale).status.contains("not connected"));
        assert!(guide(&runtime(Value::Null)).status.contains("list is unavailable"));
        let empty = guide(&runtime(json!([])));
        assert_eq!(empty.rows.len(), ACTIONS.len());
        assert!(empty.rows.iter().all(|(_, v)| v == "Not identified"));
    }
    #[test]
    fn scripts_stay_opaque_and_unapplied_file_values_are_never_merged() {
        let mut r = runtime(json!([
            {"keys":"super+p","action":"spawn","args":["sh","-c","exec /usr/bin/decspot"]},
            {"keys":"alt+p","action":"spawn","args":["sh","-c","echo decspot; arbitrary-command"]},
            {"keys":"super+space","action":"none"},
            {"keys":"bad\nkey","action":"close"}
        ]));
        r.config = json!({"binds":[{"keys":"super+space","action":"spawn","args":["decspot"]}]});
        let g = guide(&r);
        assert_eq!(g.rows.iter().find(|(l, _)| *l == "App search").unwrap().1, "Super+P");
        assert_eq!(g.rows.iter().find(|(l, _)| *l == "Custom launch commands").unwrap().1, "Alt+P");
        assert_eq!(g.rows.iter().find(|(l, _)| *l == "Close a window").unwrap().1, "Not identified");
    }
    #[test]
    fn long_lists_are_bounded_without_hiding_their_existence() {
        let list: Vec<_> = (1..8).map(|i| json!({"keys":format!("super+F{i}"),"action":"spawn","args":["decspot"]})).collect();
        let g = guide(&runtime(json!(list)));
        let row = &g.rows[0].1;
        assert!(row.contains("(+4 more in Shortcuts)"));
        assert!(row.len() < 100);
    }
}
