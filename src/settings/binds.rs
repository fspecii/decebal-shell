//! Key bindings: merging rdwm's runtime list with the file, normalising key specs, spotting
//! conflicts and turning recorded keys into specs. Works with whatever binds exist: plain
//! `mods`+`key`, `keys = [...]` chord sequences, `[[mode]]` blocks, defaults and IPC binds.

use super::tomlcfg::{Cfg, Val};
use crate::window_app::{Key, KeyEvent};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Src {
    /// `[[bind]]` number n in the file.
    File(usize),
    /// Built-in default (only known while rdwm runs).
    Default,
    /// Registered at runtime over IPC; not editable.
    Ipc,
    /// Inside `[[mode]]` (read only here).
    Mode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BindRow {
    /// Shown to the user, e.g. `Super+Shift+Return` or `Super+x  r`.
    pub keys: String,
    pub norm: String,
    pub action: String,
    pub args: Vec<String>,
    pub mode: String,
    pub src: Src,
}

fn canon_mod(m: &str) -> Option<&'static str> {
    Some(match m.trim().to_ascii_lowercase().as_str() {
        "super" | "mod4" | "logo" | "win" | "mod" | "meta" => "super",
        "ctrl" | "control" => "ctrl",
        "alt" | "mod1" => "alt",
        "shift" => "shift",
        _ => return None,
    })
}

/// One chord (`super+shift+Return`) -> canonical lowercase form with sorted modifiers.
pub fn norm_chord(chord: &str) -> String {
    let mut mods: Vec<&str> = Vec::new();
    let mut key = String::new();
    let parts: Vec<&str> = chord.split('+').collect();
    for (i, p) in parts.iter().enumerate() {
        let last = i == parts.len() - 1;
        match canon_mod(p) {
            Some(m) if !last || p.is_empty() => mods.push(m),
            Some(m) if parts.len() > 1 => mods.push(m),
            _ => key = p.trim().to_ascii_lowercase(),
        }
    }
    if parts.last().is_some_and(|l| l.is_empty()) {
        key = "plus".into(); // "super++"
    }
    mods.sort_by_key(|m| ["super", "ctrl", "alt", "shift"].iter().position(|x| x == m));
    mods.dedup();
    mods.iter().map(|m| m.to_string()).chain(std::iter::once(key)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("+")
}

/// A full sequence (`"super+x r"` or several chords) -> canonical.
pub fn norm_keys(spec: &str) -> String {
    spec.split_whitespace().map(norm_chord).collect::<Vec<_>>().join(" ")
}

fn nice_chord(chord: &str) -> String {
    chord
        .split('+')
        .map(|p| match canon_mod(p) {
            Some("super") => "Super".to_string(),
            Some("ctrl") => "Ctrl".into(),
            Some("alt") => "Alt".into(),
            Some("shift") => "Shift".into(),
            _ => {
                let mut c = p.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

pub fn nice(spec: &str) -> String {
    spec.split_whitespace().map(nice_chord).collect::<Vec<_>>().join("  ")
}

/// Spec of a `[[bind]]`-like table: `keys` (string or list) or `mods`+`key`.
fn spec_of(get: &dyn Fn(&str) -> Option<Val>) -> Option<String> {
    match get("keys") {
        Some(Val::Str(s)) => return Some(s),
        Some(Val::List(l)) => return Some(l.join(" ")),
        _ => {}
    }
    let key = get("key")?.as_str()?.to_string();
    let mods = match get("mods") {
        Some(Val::List(l)) => l.join("+"),
        Some(Val::Str(s)) => s,
        _ => String::new(),
    };
    Some(if mods.is_empty() { key } else { format!("{mods}+{key}") })
}

fn args_of(v: Option<Val>) -> Vec<String> {
    match v {
        Some(Val::List(l)) => l,
        _ => Vec::new(),
    }
}

/// All bindings to show. `runtime` is rdwm's `bind list` array (may be null while rdwm is down).
pub fn collect(cfg: &Cfg, runtime: &Value) -> Vec<BindRow> {
    let mut rows: Vec<BindRow> = Vec::new();
    let file: Vec<(usize, String, String, Vec<String>)> = (0..cfg.array_len("bind"))
        .filter_map(|i| {
            let spec = spec_of(&|k| cfg.array_get("bind", i, k))?;
            let action = cfg.array_get("bind", i, "action")?.as_str()?.to_string();
            Some((i, spec, action, args_of(cfg.array_get("bind", i, "args"))))
        })
        .collect();
    let mut seen_file: Vec<usize> = Vec::new();
    if let Some(list) = runtime.as_array() {
        for b in list {
            let spec = b["keys"].as_str().unwrap_or("").to_string();
            let norm = norm_keys(&spec);
            let action = b["action"].as_str().unwrap_or("").to_string();
            let args: Vec<String> = b["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            let mode = b["mode"].as_str().filter(|m| !m.is_empty()).unwrap_or("default").to_string();
            let src = if b["source"] == "ipc" {
                Src::Ipc
            } else if mode != "default" {
                Src::Mode
            } else if let Some(f) = file.iter().find(|f| norm_keys(&f.1) == norm && !seen_file.contains(&f.0)) {
                seen_file.push(f.0);
                Src::File(f.0)
            } else {
                Src::Default
            };
            rows.push(BindRow { keys: nice(&spec), norm, action, args, mode, src });
        }
    }
    // file binds rdwm has not applied (or rdwm is not running)
    for (i, spec, action, args) in &file {
        if !seen_file.contains(i) && !(runtime.is_array() && action == "none") {
            rows.push(BindRow { keys: nice(spec), norm: norm_keys(spec), action: action.clone(), args: args.clone(), mode: "default".into(), src: Src::File(*i) });
        }
    }
    if !runtime.is_array() {
        for m in 0..cfg.array_len("mode") {
            let name = cfg.array_get("mode", m, "name").and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            for j in 0..cfg.nested_len("mode", m, "bind") {
                let g = |k: &str| cfg.nested_get("mode", m, "bind", j, k);
                if let (Some(spec), Some(action)) = (spec_of(&g), g("action").and_then(|v| v.as_str().map(String::from))) {
                    rows.push(BindRow { keys: nice(&spec), norm: norm_keys(&spec), action, args: args_of(g("args")), mode: name.clone(), src: Src::Mode });
                }
            }
        }
    }
    rows.retain(|r| r.action != "none");
    rows
}

/// Human warnings: identical sequences in one mode, and a sequence that is the start of another.
pub fn conflicts(rows: &[BindRow]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in rows.iter().enumerate() {
        for b in &rows[i + 1..] {
            if a.mode != b.mode {
                continue;
            }
            if a.norm == b.norm {
                out.push(format!("{} is bound twice: {} and {}", a.keys, a.action, b.action));
            } else if b.norm.starts_with(&format!("{} ", a.norm)) || a.norm.starts_with(&format!("{} ", b.norm)) {
                out.push(format!("{} starts the sequence {}: one of them cannot work", if a.norm.len() < b.norm.len() { &a.keys } else { &b.keys }, if a.norm.len() < b.norm.len() { &b.keys } else { &a.keys }));
            }
        }
    }
    out
}

/// X keysym name for a pressed key (`None` for keys that cannot be bound, like Escape).
pub fn key_name(ev: &KeyEvent) -> Option<String> {
    Some(match ev.key {
        Key::Escape => return None,
        Key::Enter => "Return".into(),
        Key::Tab => "Tab".into(),
        Key::Backspace => "BackSpace".into(),
        Key::Delete => "Delete".into(),
        Key::Left => "Left".into(),
        Key::Right => "Right".into(),
        Key::Up => "Up".into(),
        Key::Down => "Down".into(),
        Key::PageUp => "Page_Up".into(),
        Key::PageDown => "Page_Down".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::Char => match ev.ch? {
            ' ' => "space".into(),
            ',' => "comma".into(),
            '.' => "period".into(),
            '-' => "minus".into(),
            '=' => "equal".into(),
            '+' => "plus".into(),
            '/' => "slash".into(),
            '\\' => "backslash".into(),
            ';' => "semicolon".into(),
            '\'' => "apostrophe".into(),
            '`' => "grave".into(),
            '[' => "bracketleft".into(),
            ']' => "bracketright".into(),
            c if c.is_ascii_alphanumeric() => c.to_string(),
            _ => return None,
        },
    })
}

/// `Super+Shift+Return`-style spec from toggled modifier chips plus the pressed key.
pub fn build_spec(mods: &[&str], key: &str) -> String {
    let mut m: Vec<&str> = ["super", "ctrl", "alt", "shift"].into_iter().filter(|x| mods.contains(x)).collect();
    m.push(key);
    m.join("+")
}

/// Splits `super+shift+Return` into `(["super","shift"], "Return")` for writing `mods`/`key`.
pub fn split_spec(spec: &str) -> Option<(Vec<String>, String)> {
    if spec.split_whitespace().count() != 1 {
        return None;
    }
    let mut parts: Vec<&str> = spec.split('+').collect();
    let key = parts.pop()?.to_string();
    if key.is_empty() {
        return None;
    }
    Some((parts.iter().map(|s| s.to_ascii_lowercase()).collect(), key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalisation_ignores_case_order_and_aliases() {
        assert_eq!(norm_keys("Shift+Super+Return"), "super+shift+return");
        assert_eq!(norm_keys("mod4+SHIFT+Return"), "super+shift+return");
        assert_eq!(norm_keys("Super+x  r"), "super+x r");
        assert_eq!(norm_keys("super+plus"), "super+plus");
        assert_eq!(norm_keys("Control+alt+Delete"), "ctrl+alt+delete");
        assert_eq!(norm_keys("F5"), "f5");
        assert_eq!(nice("super+shift+return"), "Super+Shift+Return");
    }

    #[test]
    fn file_binds_chords_and_modes_without_rdwm() {
        let cfg = Cfg::parse(
            r#"
[[bind]]
mods = ["super", "shift"]
key = "Return"
action = "spawn"
args = ["foot"]
[[bind]]
keys = ["super+x", "r"]
action = "reload"
[[bind]]
keys = "super+w"
action = "close"
[[bind]]
mods = "super"
key = "q"
action = "none"
[[mode]]
name = "resize"
[[mode.bind]]
keys = "h"
action = "setmfact"
args = ["-0.05"]
"#,
        )
        .unwrap();
        let rows = collect(&cfg, &Value::Null);
        assert_eq!(rows.len(), 4, "{rows:#?}");
        assert_eq!(rows[0].norm, "super+shift+return");
        assert_eq!(rows[0].args, ["foot"]);
        assert_eq!(rows[1].norm, "super+x r");
        assert_eq!(rows[2].src, Src::File(2));
        assert_eq!((rows[3].mode.as_str(), rows[3].src.clone()), ("resize", Src::Mode));
    }

    #[test]
    fn runtime_list_marks_defaults_files_ipc_and_modes() {
        let cfg = Cfg::parse("[[bind]]\nkeys = \"super+w\"\naction = \"close\"\n").unwrap();
        let rt = json!([
            {"keys":"Super+w","action":"close","source":"config"},
            {"keys":"Super+Return","action":"spawn","source":"config","args":["foot"]},
            {"keys":"super+F1","action":"spawn","source":"ipc","args":["x"]},
            {"keys":"h","action":"setmfact","source":"config","mode":"resize"},
        ]);
        let rows = collect(&cfg, &rt);
        assert_eq!(rows.iter().map(|r| r.src.clone()).collect::<Vec<_>>(), [Src::File(0), Src::Default, Src::Ipc, Src::Mode]);
    }

    #[test]
    fn conflicts_are_reported() {
        let mk = |k: &str, a: &str| BindRow { keys: nice(k), norm: norm_keys(k), action: a.into(), args: vec![], mode: "default".into(), src: Src::Default };
        let rows = vec![mk("super+q", "close"), mk("Super+Q", "quit"), mk("super+x", "reload"), mk("super+x r", "reload"), mk("super+y", "zoom")];
        let c = conflicts(&rows);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c[0].contains("bound twice") && c[1].contains("starts the sequence"));
        assert!(conflicts(&[mk("super+a", "x"), mk("super+b", "y")]).is_empty());
    }

    #[test]
    fn recorded_keys_become_specs() {
        let ev = |key, ch: Option<char>| KeyEvent { key, text: String::new(), ch, ctrl: false, shift: false, alt: false };
        assert_eq!(key_name(&ev(Key::Enter, None)).as_deref(), Some("Return"));
        assert_eq!(key_name(&ev(Key::Char, Some(' '))).as_deref(), Some("space"));
        assert_eq!(key_name(&ev(Key::Char, Some('q'))).as_deref(), Some("q"));
        assert_eq!(key_name(&ev(Key::Char, Some(','))).as_deref(), Some("comma"));
        assert_eq!(key_name(&ev(Key::Escape, None)), None);
        assert_eq!(build_spec(&["shift", "super"], "Return"), "super+shift+Return");
        assert_eq!(build_spec(&[], "F5"), "F5");
        assert_eq!(split_spec("super+shift+Return"), Some((vec!["super".into(), "shift".into()], "Return".into())));
        assert_eq!(split_spec("super+x r"), None);
    }
}
