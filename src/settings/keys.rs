//! Keyboard pane actions: rebinding, removing and adding shortcuts.

use super::binds::{self, Src};
use super::model::{CtlVal, Model};
use super::tomlcfg::{Cfg, Val};

/// Fields of a `[[bind]]` for `spec`: `mods`+`key` for a single key, `keys` for sequences.
fn spec_fields(spec: &str) -> Vec<(&'static str, Val)> {
    match binds::split_spec(spec) {
        Some((mods, key)) if mods.is_empty() => vec![("key", Val::Str(key))],
        Some((mods, key)) => vec![("mods", Val::List(mods)), ("key", Val::Str(key))],
        None => vec![("keys", Val::Str(spec.split_whitespace().collect::<Vec<_>>().join(" ")))],
    }
}

fn push_bind(c: &mut Cfg, spec: &str, action: &str, args: &[String]) -> Result<(), String> {
    let mut fields = spec_fields(spec);
    fields.push(("action", Val::Str(action.to_string())));
    if !args.is_empty() {
        fields.push(("args", Val::List(args.to_vec())));
    }
    c.array_push("bind", &fields).map(|_| ())
}

/// Disables a built-in default (`action = "none"` on the same keys).
fn push_none(c: &mut Cfg, spec: &str) -> Result<(), String> {
    push_bind(c, spec, "none", &[])
}

/// Re-points file bind `idx` at `spec`, keeping the style (`mods`/`key` vs `keys`) when possible.
fn retarget(c: &mut Cfg, idx: usize, spec: &str) -> Result<(), String> {
    let had_keys = c.array_get("bind", idx, "keys").is_some();
    match binds::split_spec(spec) {
        Some((mods, key)) if !had_keys => {
            if mods.is_empty() {
                c.array_unset("bind", idx, "mods")?;
            } else {
                c.array_set("bind", idx, "mods", &Val::List(mods))?;
            }
            c.array_set("bind", idx, "key", &Val::Str(key))?;
        }
        _ => {
            c.array_set("bind", idx, "keys", &Val::Str(spec.split_whitespace().collect::<Vec<_>>().join(" ")))?;
            c.array_unset("bind", idx, "mods")?;
            c.array_unset("bind", idx, "key")?;
        }
    }
    Ok(())
}

pub fn rebind(m: &mut Model, n: usize, spec: &str) {
    let rows = m.bind_rows();
    let Some(row) = rows.get(n).cloned() else { return };
    if binds::norm_keys(spec) == row.norm {
        return;
    }
    let spec = spec.to_string();
    match row.src {
        Src::File(idx) => {
            m.edit(|c| retarget(c, idx, &spec));
        }
        Src::Default => {
            let old = row.keys.replace("  ", " ");
            m.edit(|c| {
                push_none(c, &old)?;
                push_bind(c, &spec, &row.action, &row.args)
            });
        }
        _ => m.say("This shortcut cannot be changed here", true),
    }
}

pub fn remove(m: &mut Model, n: usize) {
    let rows = m.bind_rows();
    let Some(row) = rows.get(n).cloned() else { return };
    match row.src {
        Src::File(idx) => {
            m.edit(|c| c.array_remove("bind", idx));
        }
        Src::Default => {
            m.edit(|c| push_none(c, &row.keys.replace("  ", " ")));
        }
        _ => m.say("This shortcut cannot be removed here", true),
    }
}

pub fn apply(m: &mut Model, id: &str, v: CtlVal) {
    let parts: Vec<&str> = id.split(':').collect();
    match (parts.as_slice(), v) {
        (["k", "filter"], CtlVal::Str(s)) => {
            m.sel.bind_filter = s;
            m.dirty = true;
        }
        (["k", "new", "keys"], CtlVal::Click) => {
            m.sel.recording = Some("new".into());
            m.sel.rec_mods.clear();
            m.dirty = true;
        }
        (["k", "new", "keys"], CtlVal::Str(s)) => {
            m.sel.new_bind.keys = s;
            m.sel.recording = None;
            m.dirty = true;
        }
        (["k", "new", "action"], CtlVal::Str(s)) => {
            m.sel.new_bind.action = s;
            m.dirty = true;
        }
        (["k", "new", "args"], CtlVal::Str(s)) => {
            m.sel.new_bind.args = s;
            m.dirty = true;
        }
        (["k", "new", "add"], _) => {
            let nb = m.sel.new_bind.clone();
            if nb.keys.is_empty() || nb.action.is_empty() {
                m.say("Record the keys and choose an action first", true);
                return;
            }
            let args: Vec<String> = nb.args.split_whitespace().map(String::from).collect();
            if m.edit(|c| push_bind(c, &nb.keys, &nb.action, &args)) {
                m.sel.new_bind = Default::default();
            }
        }
        (["k", "del", n], _) => {
            if let Ok(n) = n.parse() {
                remove(m, n);
            }
        }
        (["k", n], CtlVal::Click) => {
            if n.parse::<usize>().is_ok() {
                m.sel.recording = Some(id.to_string());
                m.sel.rec_mods.clear();
                m.dirty = true;
            }
        }
        (["k", n], CtlVal::Str(spec)) => {
            m.sel.recording = None;
            if let Ok(n) = n.parse() {
                rebind(m, n, &spec);
            }
            m.dirty = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::tests_support::model_with;
    use super::*;
    use serde_json::json;

    fn text(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn rebinding_a_file_bind_keeps_its_style_and_comments() {
        let (mut m, cfg) = model_with("[[bind]]\nmods = [\"super\"]   # term\nkey = \"Return\"\naction = \"spawn\"\nargs = [\"foot\"]\n\n[[bind]]\nkeys = [\"super+x\", \"r\"]\naction = \"reload\"\n");
        apply(&mut m, "k:0", CtlVal::Str("super+shift+t".into()));
        let t = text(&cfg);
        assert!(t.contains("mods = [\"super\", \"shift\"]") && t.contains("key = \"t\"") && t.contains("args = [\"foot\"]"), "{t}");
        // a chord sequence stays a `keys` entry
        apply(&mut m, "k:1", CtlVal::Str("super+y z".into()));
        assert!(text(&cfg).contains("keys = \"super+y z\""), "{}", text(&cfg));
        // turning a plain bind into a sequence switches fields
        apply(&mut m, "k:0", CtlVal::Str("super+a b".into()));
        let t = text(&cfg);
        assert!(t.contains("keys = \"super+a b\"") && !t.contains("key = \"t\""), "{t}");
    }

    #[test]
    fn rebinding_or_removing_a_default_overrides_it() {
        let (mut m, cfg) = model_with("");
        m.rt.running = true;
        m.rt.binds = json!([{"keys":"Super+Return","action":"spawn","source":"config","args":["foot"]}, {"keys":"Super+q","action":"close","source":"config"}]);
        apply(&mut m, "k:0", CtlVal::Str("super+t".into()));
        let t = text(&cfg);
        assert!(t.contains("action = \"none\"") && t.contains("key = \"Return\""), "{t}");
        assert!(t.contains("key = \"t\"") && t.contains("args = [\"foot\"]"), "{t}");
        apply(&mut m, "k:del:1", CtlVal::Click);
        assert_eq!(t.matches("[[bind]]").count() + 1, text(&cfg).matches("[[bind]]").count());
        assert!(super::super::tomlcfg::Cfg::parse(&text(&cfg)).is_ok());
    }

    #[test]
    fn adding_requires_keys_and_action() {
        let (mut m, cfg) = model_with("");
        apply(&mut m, "k:new:add", CtlVal::Click);
        assert!(m.notice.as_ref().unwrap().error);
        assert_eq!(text(&cfg), "");
        apply(&mut m, "k:new:keys", CtlVal::Str("super+F1".into()));
        apply(&mut m, "k:new:action", CtlVal::Str("spawn".into()));
        apply(&mut m, "k:new:args", CtlVal::Str("foot -e htop".into()));
        apply(&mut m, "k:new:add", CtlVal::Click);
        let t = text(&cfg);
        assert!(t.contains("mods = [\"super\"]") && t.contains("key = \"F1\"") && t.contains("args = [\"foot\", \"-e\", \"htop\"]"), "{t}");
        assert!(m.sel.new_bind.keys.is_empty());
    }

    #[test]
    fn recording_state_machine() {
        let (mut m, _) = model_with("[[bind]]\nkey = \"F9\"\naction = \"reload\"\n");
        apply(&mut m, "k:0", CtlVal::Click);
        assert_eq!(m.sel.recording.as_deref(), Some("k:0"));
        m.apply("mod:super", CtlVal::Click);
        m.apply("mod:shift", CtlVal::Click);
        m.apply("mod:super", CtlVal::Click);
        assert_eq!(m.sel.rec_mods, ["shift"]);
        apply(&mut m, "k:0", CtlVal::Str("shift+F9".into()));
        assert!(m.sel.recording.is_none());
    }
}
