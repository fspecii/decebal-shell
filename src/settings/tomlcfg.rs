//! Comment-preserving editor for rdwm's `config.toml` (the source of truth).
//!
//! Every setter edits the parsed `toml_edit` document in place: comments, key order, formatting of
//! untouched keys and keys this app does not know all survive. The file is never regenerated from a
//! struct. A replaced value keeps its leading/trailing decoration (so `key = 1  # why` stays commented).

use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table, Value};

/// A scalar or string list as the UI sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<String>),
}

impl Val {
    pub fn as_bool(&self) -> Option<bool> {
        if let Val::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Val::Int(i) => Some(*i as f64),
            Val::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Val::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
    fn to_toml(&self) -> Value {
        match self {
            Val::Bool(b) => Value::from(*b),
            Val::Int(i) => Value::from(*i),
            // 0.1 + 0.2 style noise never reaches the file
            Val::Float(f) => Value::from((f * 1e6).round() / 1e6),
            Val::Str(s) => Value::from(s.as_str()),
            Val::List(l) => {
                let mut a = Array::new();
                for s in l {
                    a.push(s.as_str());
                }
                Value::Array(a)
            }
        }
    }
    fn from_toml(v: &Value) -> Option<Val> {
        Some(match v {
            Value::Boolean(b) => Val::Bool(*b.value()),
            Value::Integer(i) => Val::Int(*i.value()),
            Value::Float(f) => Val::Float(*f.value()),
            Value::String(s) => Val::Str(s.value().clone()),
            Value::Array(a) => Val::List(a.iter().filter_map(|x| x.as_str().map(String::from)).collect()),
            _ => return None,
        })
    }
}

/// The parsed config document.
#[derive(Clone)]
pub struct Cfg {
    doc: DocumentMut,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg { doc: DocumentMut::new() }
    }
}

fn table_at<'a>(root: &'a Table, section: &str) -> Option<&'a Table> {
    let mut t = root;
    if section.is_empty() {
        return Some(t);
    }
    for part in section.split('.') {
        t = t.get(part)?.as_table()?;
    }
    Some(t)
}

fn table_at_mut<'a>(root: &'a mut Table, section: &str, create: bool) -> Result<&'a mut Table, String> {
    let mut t = root;
    if section.is_empty() {
        return Ok(t);
    }
    for part in section.split('.') {
        if !t.contains_key(part) {
            if !create {
                return Err(format!("no [{section}]"));
            }
            let mut n = Table::new();
            // `[decoration]` is only written when it has keys; `[decoration.bar]` needs no parent header.
            n.set_implicit(section.contains('.'));
            t.insert(part, Item::Table(n));
        }
        t = t.get_mut(part).and_then(Item::as_table_mut).ok_or_else(|| format!("`{part}` in [{section}] is not a table"))?;
    }
    Ok(t)
}

fn forget_position(t: &mut Table) {
    t.set_position(None);
    for (_, item) in t.iter_mut() {
        match item {
            Item::Table(t) => forget_position(t),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(forget_position),
            _ => {}
        }
    }
}

/// Stores `v` at `slot`, keeping the decoration (comments, spacing) of the value it replaces.
fn put(slot: &mut Item, v: Value) {
    let mut v = v;
    if let Some(old) = slot.as_value() {
        let d = old.decor();
        if let Some(p) = d.prefix() {
            v.decor_mut().set_prefix(p.clone());
        }
        if let Some(s) = d.suffix() {
            v.decor_mut().set_suffix(s.clone());
        }
    }
    *slot = value(v);
}

impl Cfg {
    pub fn parse(text: &str) -> Result<Cfg, String> {
        text.parse::<DocumentMut>().map(|doc| Cfg { doc }).map_err(|e| e.to_string())
    }

    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    // ---------------------------------------------------------------- plain keys

    pub fn get(&self, section: &str, key: &str) -> Option<Val> {
        let t = table_at(self.doc.as_table(), section)?;
        Val::from_toml(t.get(key)?.as_value()?)
    }

    pub fn has(&self, section: &str, key: &str) -> bool {
        table_at(self.doc.as_table(), section).is_some_and(|t| t.contains_key(key))
    }

    pub fn set(&mut self, section: &str, key: &str, v: &Val) -> Result<(), String> {
        let t = table_at_mut(self.doc.as_table_mut(), section, true)?;
        if t.contains_key(key) && t.get(key).is_some_and(|i| !i.is_value()) {
            return Err(format!("`{key}` in [{section}] is a table, not a value"));
        }
        let slot = t.entry(key).or_insert(Item::None);
        put(slot, v.to_toml());
        Ok(())
    }

    /// Removes a key (back to the built-in default). Drops a table that became empty.
    pub fn remove(&mut self, section: &str, key: &str) -> bool {
        let Ok(t) = table_at_mut(self.doc.as_table_mut(), section, false) else { return false };
        t.remove(key).is_some()
    }

    // ---------------------------------------------------------------- [[arrays of tables]]

    fn aot(&self, name: &str) -> Option<&ArrayOfTables> {
        let (parent, last) = name.rsplit_once('.').unwrap_or(("", name));
        table_at(self.doc.as_table(), parent)?.get(last)?.as_array_of_tables()
    }

    /// `name` may be dotted (`wallpaper.output` for `[[wallpaper.output]]`).
    fn aot_mut(&mut self, name: &str, create: bool) -> Result<&mut ArrayOfTables, String> {
        let (parent, last) = name.rsplit_once('.').unwrap_or(("", name));
        let t = table_at_mut(self.doc.as_table_mut(), parent, create)?;
        if !t.contains_key(last) {
            if !create {
                return Err(format!("no [[{name}]]"));
            }
            t.insert(last, Item::ArrayOfTables(ArrayOfTables::new()));
        }
        t.get_mut(last).and_then(Item::as_array_of_tables_mut).ok_or_else(|| format!("`{name}` is not an array of tables (inline arrays are not editable here)"))
    }

    pub fn array_len(&self, name: &str) -> usize {
        self.aot(name).map_or(0, |a| a.len())
    }

    pub fn array_get(&self, name: &str, i: usize, key: &str) -> Option<Val> {
        Val::from_toml(self.aot(name)?.get(i)?.get(key)?.as_value()?)
    }

    pub fn array_keys(&self, name: &str, i: usize) -> Vec<String> {
        self.aot(name).and_then(|a| a.get(i)).map(|t| t.iter().filter(|(_, v)| v.is_value()).map(|(k, _)| k.to_string()).collect()).unwrap_or_default()
    }

    pub fn array_set(&mut self, name: &str, i: usize, key: &str, v: &Val) -> Result<(), String> {
        let t = self.aot_mut(name, false)?.get_mut(i).ok_or_else(|| format!("[[{name}]] #{i} does not exist"))?;
        put(t.entry(key).or_insert(Item::None), v.to_toml());
        Ok(())
    }

    pub fn array_unset(&mut self, name: &str, i: usize, key: &str) -> Result<(), String> {
        let t = self.aot_mut(name, false)?.get_mut(i).ok_or_else(|| format!("[[{name}]] #{i} does not exist"))?;
        t.remove(key);
        Ok(())
    }

    pub fn array_push(&mut self, name: &str, fields: &[(&str, Val)]) -> Result<usize, String> {
        let a = self.aot_mut(name, true)?;
        let mut t = Table::new();
        for (k, v) in fields {
            t.insert(k, value(v.to_toml()));
        }
        a.push(t);
        Ok(a.len() - 1)
    }

    pub fn array_remove(&mut self, name: &str, i: usize) -> Result<(), String> {
        self.aot_len_check(name, i)?;
        let arr = self.aot_mut(name, false)?;
        arr.remove(i);
        if arr.is_empty() {
            let (parent, last) = name.rsplit_once('.').unwrap_or(("", name));
            if let Ok(t) = table_at_mut(self.doc.as_table_mut(), parent, false) {
                t.remove(last);
            }
        }
        Ok(())
    }

    fn aot_len_check(&self, name: &str, i: usize) -> Result<(), String> {
        if i < self.array_len(name) {
            Ok(())
        } else {
            Err(format!("[[{name}]] #{i} does not exist"))
        }
    }

    /// Moves entry `from` so that it ends at index `to`.
    pub fn array_move(&mut self, name: &str, from: usize, to: usize) -> Result<(), String> {
        self.aot_len_check(name, from)?;
        self.aot_len_check(name, to)?;
        let a = self.aot_mut(name, false)?;
        let mut tables: Vec<Table> = a.iter().cloned().collect();
        let t = tables.remove(from);
        tables.insert(to, t);
        a.clear();
        for mut t in tables {
            // A parsed table renders at its old place in the file whatever its index: let the new order decide.
            forget_position(&mut t);
            a.push(t);
        }
        Ok(())
    }

    /// Entries of `[[name.sub]]` nested in `[[name]]` #i (e.g. `[[mode.bind]]`), read only.
    pub fn nested_len(&self, name: &str, i: usize, sub: &str) -> usize {
        self.aot(name).and_then(|a| a.get(i)).and_then(|t| t.get(sub)).and_then(Item::as_array_of_tables).map_or(0, |a| a.len())
    }

    pub fn nested_get(&self, name: &str, i: usize, sub: &str, j: usize, key: &str) -> Option<Val> {
        let t = self.aot(name)?.get(i)?.get(sub)?.as_array_of_tables()?.get(j)?;
        Val::from_toml(t.get(key)?.as_value()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# my rdwm config
[general]
border_px = 2   # thin
mfact = 0.6
tags = ["a", "b"]
future_key = "kept"   # not known to the app

# binds
[[bind]]
mods = ["super"]
key = "Return"
action = "spawn"
args = ["foot"]   # terminal

[[bind]]
keys = ["super+x", "r"]
action = "reload"

[decoration]
enabled = true

[decoration.bar]
font_size = 12

[[mode]]
name = "resize"
[[mode.bind]]
keys = "h"
action = "setmfact"
"#;

    fn cfg() -> Cfg {
        Cfg::parse(SAMPLE).unwrap()
    }

    #[test]
    fn untouched_document_round_trips_byte_for_byte() {
        assert_eq!(cfg().text(), SAMPLE);
    }

    #[test]
    fn set_keeps_comments_order_and_unknown_keys() {
        let mut c = cfg();
        c.set("general", "border_px", &Val::Int(4)).unwrap();
        let t = c.text();
        assert!(t.contains("border_px = 4   # thin"), "{t}");
        assert!(t.contains("future_key = \"kept\"   # not known to the app"));
        assert!(t.starts_with("# my rdwm config\n[general]\nborder_px = 4"));
        assert!(t.contains("# binds\n[[bind]]"));
        assert_eq!(t.replace("border_px = 4", "border_px = 2"), SAMPLE);
    }

    #[test]
    fn every_value_type_round_trips() {
        let mut c = cfg();
        c.set("general", "smart_gaps", &Val::Bool(true)).unwrap();
        c.set("general", "gap_inner", &Val::Int(8)).unwrap();
        c.set("general", "mfact", &Val::Float(0.123_456_789)).unwrap();
        c.set("colors", "focused", &Val::Str("#ff0000".into())).unwrap();
        c.set("general", "layouts", &Val::List(vec!["tile".into(), "bsp".into()])).unwrap();
        let c2 = Cfg::parse(&c.text()).unwrap();
        assert_eq!(c2.get("general", "smart_gaps"), Some(Val::Bool(true)));
        assert_eq!(c2.get("general", "gap_inner"), Some(Val::Int(8)));
        assert_eq!(c2.get("general", "mfact"), Some(Val::Float(0.123457)));
        assert_eq!(c2.get("colors", "focused"), Some(Val::Str("#ff0000".into())));
        assert_eq!(c2.get("general", "layouts"), Some(Val::List(vec!["tile".into(), "bsp".into()])));
        assert_eq!(c2.get("general", "future_key"), Some(Val::Str("kept".into())));
    }

    #[test]
    fn missing_sections_are_created_and_nested_ones_need_no_parent() {
        let mut c = Cfg::parse("# empty\n").unwrap();
        c.set("snap", "windows", &Val::Bool(false)).unwrap();
        c.set("decoration.bar", "font_size", &Val::Float(11.0)).unwrap();
        let t = c.text();
        assert!(t.contains("# empty\n"), "comment kept: {t}");
        assert!(t.contains("[snap]\nwindows = false"));
        assert!(t.contains("[decoration.bar]\nfont_size = 11.0"));
        assert!(!t.contains("[decoration]\n"), "no empty parent header: {t}");
        assert!(Cfg::parse(&t).is_ok());
    }

    #[test]
    fn remove_restores_default_and_keeps_the_rest() {
        let mut c = cfg();
        assert!(c.remove("general", "mfact"));
        assert!(!c.remove("general", "mfact"));
        assert!(!c.remove("nosuch", "x"));
        assert!(!c.text().contains("mfact = "));
        assert!(c.text().contains("border_px = 2   # thin"));
    }

    #[test]
    fn rejects_setting_through_a_non_table() {
        let mut c = cfg();
        assert!(c.set("general.border_px", "x", &Val::Int(1)).is_err());
        assert!(c.set("decoration", "bar", &Val::Int(1)).is_err());
        assert_eq!(c.text(), SAMPLE);
    }

    #[test]
    fn invalid_toml_is_rejected() {
        assert!(Cfg::parse("[general\nx=").is_err());
        assert!(Cfg::parse("a = 1\na = 2").is_err());
    }

    #[test]
    fn array_of_tables_edit() {
        let mut c = cfg();
        assert_eq!(c.array_len("bind"), 2);
        assert_eq!(c.array_get("bind", 0, "key"), Some(Val::Str("Return".into())));
        c.array_set("bind", 0, "key", &Val::Str("t".into())).unwrap();
        assert!(c.text().contains("key = \"t\""));
        assert!(c.text().contains("args = [\"foot\"]   # terminal"));
        let i = c.array_push("bind", &[("keys", Val::Str("super+q".into())), ("action", Val::Str("close".into()))]).unwrap();
        assert_eq!(i, 2);
        c.array_move("bind", 2, 0).unwrap();
        assert_eq!(c.array_get("bind", 0, "action"), Some(Val::Str("close".into())));
        assert_eq!(c.array_get("bind", 1, "key"), Some(Val::Str("t".into())));
        assert!(c.text().contains("# terminal"), "comments survive a reorder");
        let reread = Cfg::parse(&c.text()).unwrap();
        assert_eq!(reread.array_get("bind", 0, "action"), Some(Val::Str("close".into())), "the file has the new order too");
        assert_eq!(reread.array_get("bind", 1, "key"), Some(Val::Str("t".into())));
        let mut moved = Cfg::parse(&c.text()).unwrap();
        moved.array_move("bind", 0, 2).unwrap();
        assert_eq!(Cfg::parse(&moved.text()).unwrap().array_get("bind", 2, "action"), Some(Val::Str("close".into())), "a parsed file reorders too");
        c.array_remove("bind", 0).unwrap();
        c.array_remove("bind", 0).unwrap();
        c.array_remove("bind", 0).unwrap();
        assert_eq!(c.array_len("bind"), 0);
        assert!(!c.text().contains("[[bind]]"));
        assert!(c.array_remove("bind", 0).is_err());
        assert!(c.array_set("rule", 0, "x", &Val::Int(1)).is_err());
        assert!(Cfg::parse(&c.text()).is_ok());
    }

    #[test]
    fn nested_mode_binds_are_readable_and_survive_edits() {
        let mut c = cfg();
        assert_eq!(c.nested_len("mode", 0, "bind"), 1);
        assert_eq!(c.nested_get("mode", 0, "bind", 0, "action"), Some(Val::Str("setmfact".into())));
        c.array_push("rule", &[("class", Val::Str("mpv".into())), ("floating", Val::Bool(true))]).unwrap();
        assert!(c.text().contains("[[mode.bind]]"));
    }

    #[test]
    fn dotted_array_names() {
        let mut c = Cfg::parse("[wallpaper]\nmode = \"fit\"\n").unwrap();
        c.array_push("wallpaper.output", &[("name", Val::Str("DP-1".into())), ("mode", Val::Str("fill".into()))]).unwrap();
        assert!(c.text().contains("[[wallpaper.output]]"), "{}", c.text());
        assert_eq!(c.array_get("wallpaper.output", 0, "name"), Some(Val::Str("DP-1".into())));
        c.array_remove("wallpaper.output", 0).unwrap();
        assert_eq!(c.array_len("wallpaper.output"), 0);
        assert_eq!(c.get("wallpaper", "mode"), Some(Val::Str("fit".into())));
    }

    #[test]
    fn inline_arrays_are_refused_not_corrupted() {
        let mut c = Cfg::parse("bind = [{ key = \"a\" }]\n").unwrap();
        assert!(c.array_push("bind", &[("key", Val::Str("b".into()))]).is_err());
    }
}
