//! Writing a [`Def`] back out as a format-1 `theme.toml` (duplicate, export, "create theme from current settings").

use crate::theme::model::{Def, Kind, Tokens, TOKENS, V};
use serde_json::{Map, Value};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table};

/// Metadata for the file header; `None` fields are left out.
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub extends: Option<String>,
    pub screenshots: Vec<String>,
}

fn scalar(v: &Value) -> Option<toml_edit::Value> {
    Some(match v {
        Value::String(s) => s.as_str().into(),
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => {
                let f = n.as_f64()?;
                if f.fract() == 0.0 && f.abs() < 1e9 {
                    (f as i64).into()
                } else {
                    ((f * 1e6).round() / 1e6).into()
                }
            }
        },
        _ => return None,
    })
}

fn array(v: &[Value]) -> toml_edit::Value {
    let mut a = Array::new();
    for x in v {
        if let Some(s) = scalar(x) {
            a.push(s);
        }
    }
    toml_edit::Value::Array(a)
}

fn put(t: &mut Table, key: &str, v: &Value) {
    match v {
        Value::Array(a) => {
            t.insert(key, value(array(a)));
        }
        Value::Object(o) => {
            let mut it = InlineTable::new();
            for (k, x) in o {
                if let Some(s) = scalar(x) {
                    it.insert(k, s);
                }
            }
            t.insert(key, value(it));
        }
        other => {
            if let Some(s) = scalar(other) {
                t.insert(key, value(s));
            }
        }
    }
}

fn tokens_table(toks: &Tokens, want_color: bool, skip: &[&str]) -> Table {
    let mut t = Table::new();
    // schema order, not alphabetical
    for (k, kind, _) in TOKENS {
        if matches!(kind, Kind::Color) != want_color || skip.contains(k) {
            continue;
        }
        match toks.get(*k) {
            Some(V::S(s)) => {
                t.insert(k, value(s.as_str()));
            }
            Some(V::N(n)) => {
                put(&mut t, k, &serde_json::json!(*n));
            }
            Some(V::L(l)) => {
                put(&mut t, k, &serde_json::json!(l));
            }
            _ => {}
        }
    }
    t
}

/// Tokens that live in another section of format 1, and where.
const LIFTED: &[(&str, &str, &str)] = &[
    ("font_family", "font", "ui"),
    ("font_preset", "font", "preset"),
    ("font_ui", "font", "ui"),
    ("font_title", "font", "title"),
    ("font_mono", "font", "mono"),
    ("font_display", "font", "display"),
    ("font_scale", "font", "scale"),
    ("bar_style", "bar", "style"),
    ("bar_height", "bar", "height"),
    ("dock_icon_size", "dock", "size"),
    ("titlebar_style", "decoration", "style"),
    ("titlebar_height", "decoration", "height"),
];

pub fn def_to_manifest(def: &Def, h: &Header) -> String {
    let mut doc = DocumentMut::new();
    let r = doc.as_table_mut();
    r.insert("format", value(1));
    r.insert("id", value(h.id.as_str()));
    r.insert("name", value(h.name.as_str()));
    r.insert("version", value(h.version.as_deref().unwrap_or("1.0.0")));
    if let Some(a) = &h.author {
        r.insert("author", value(a.as_str()));
    }
    r.insert("license", value(h.license.as_deref().unwrap_or("MIT")));
    let desc = h.description.clone().unwrap_or_else(|| def.description.clone());
    if !desc.is_empty() {
        r.insert("description", value(desc));
    }
    r.insert("min_shell_version", value("0.1.0"));
    if !h.tags.is_empty() {
        r.insert("tags", value(array(&h.tags.iter().map(|t| Value::from(t.as_str())).collect::<Vec<_>>())));
    }
    if !h.screenshots.is_empty() {
        r.insert("screenshots", value(array(&h.screenshots.iter().map(|t| Value::from(t.as_str())).collect::<Vec<_>>())));
    }
    if let Some(e) = &h.extends {
        r.insert("extends", value(e.as_str()));
    }
    if !def.dark_default && def.dark.is_none() && def.light.is_none() {
        r.insert("dark", value(false));
    }

    // colours
    let mut colors = Table::new();
    colors.set_implicit(true);
    for (k, item) in tokens_table(&def.common, true, &[]).iter() {
        colors.insert(k, item.clone());
    }
    for (name, toks) in [("dark", &def.dark), ("light", &def.light)] {
        if let Some(toks) = toks {
            colors.insert(name, Item::Table(tokens_table(toks, true, &[])));
        }
    }
    if !colors.is_empty() {
        r.insert("colors", Item::Table(colors));
    }

    // sections: extras plus the lifted tokens
    let mut sections: Map<String, Value> = def.extra.clone();
    sections.shift_remove("meta");
    sections.shift_remove("integrity");
    let widgets = sections.shift_remove("widgets");
    // the old single `family` is the font of text, titles and display numbers
    if let Some(V::S(f)) = def.common.get("font_family").filter(|_| !["font_ui", "font_title", "font_display"].iter().any(|k| def.common.contains_key(*k))) {
        let o = sections.entry("font".to_string()).or_insert_with(|| Value::Object(Map::new()));
        if let Some(o) = o.as_object_mut() {
            for role in ["ui", "title", "display"] {
                o.entry(role.to_string()).or_insert_with(|| serde_json::json!([f]));
            }
        }
    }
    for (tok, sec, key) in LIFTED {
        let v = match def.common.get(*tok) {
            Some(V::S(s)) if !s.is_empty() => Value::from(s.as_str()),
            Some(V::N(n)) => serde_json::json!(*n),
            Some(V::L(l)) if !l.is_empty() => serde_json::json!(l),
            _ => continue,
        };
        let entry = sections.entry(sec.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if let Some(o) = entry.as_object_mut() {
            o.entry(key.to_string()).or_insert(v);
        }
    }
    let mut metrics = tokens_table(&def.common, false, &LIFTED.iter().map(|l| l.0).collect::<Vec<_>>());
    if let Some(Value::Object(extra)) = sections.shift_remove("metrics") {
        for (k, v) in extra {
            put(&mut metrics, &k, &v);
        }
    }
    if !metrics.is_empty() {
        r.insert("metrics", Item::Table(metrics));
    }
    for name in ["font", "wallpaper", "decoration", "bar", "dock", "icons", "hot_corners", "control_center", "dashboard", "desktop", "rdwm"] {
        if let Some(Value::Object(o)) = sections.shift_remove(name) {
            let mut t = Table::new();
            for (k, v) in &o {
                put(&mut t, k, v);
            }
            if !t.is_empty() {
                r.insert(name, Item::Table(t));
            }
        }
    }
    if let Some(Value::Array(ws)) = widgets {
        let mut aot = ArrayOfTables::new();
        for w in ws {
            let Some(o) = w.as_object() else { continue };
            let mut t = Table::new();
            for (k, v) in o {
                if k != "options" {
                    put(&mut t, k, v);
                }
            }
            if let Some(Value::Object(opts)) = o.get("options") {
                let mut ot = Table::new();
                for (k, v) in opts {
                    put(&mut ot, k, v);
                }
                if !ot.is_empty() {
                    t.insert("options", Item::Table(ot));
                }
            }
            aot.push(t);
        }
        if !aot.is_empty() {
            r.insert("widgets", Item::ArrayOfTables(aot));
        }
    }
    doc.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::engine::manifest;
    use crate::theme::model::Selection;

    fn round(def: &Def) -> Def {
        let text = def_to_manifest(def, &Header { id: "x".into(), name: def.name.clone(), ..Header::default() });
        manifest::analyze(&text).def.unwrap_or_else(|| panic!("re-parse failed:\n{}\n{:?}", text, manifest::analyze(&text).diags))
    }

    #[test]
    fn every_builtin_survives_a_write_and_reparse() {
        for (id, _) in crate::theme::builtin::BUILTIN {
            let d = crate::theme::builtin::def(id).unwrap();
            let r = round(&d);
            for dark in [true, false] {
                if (dark && !d.has_dark()) || (!dark && !d.has_light()) {
                    continue;
                }
                assert_eq!(d.resolve(id, dark, &Selection::default()), r.resolve(id, dark, &Selection::default()), "{id} dark={dark}");
            }
            assert_eq!(d.extra.get("decoration"), r.extra.get("decoration"), "{id}");
        }
    }

    #[test]
    fn legacy_files_are_upgraded() {
        let legacy = Def::parse("name = \"Old\"\n[metrics]\nradius = 5\nbar_style = \"floating\"\ntitlebar_height = 30\n[font]\nfamily = \"Foo\"\n[dark]\nbackground = \"#101010\"\ntext = \"#eeeeee\"\naccent = \"#ff0000\"\n").unwrap();
        let text = def_to_manifest(&legacy, &Header { id: "old".into(), name: "Old".into(), ..Header::default() });
        assert!(text.contains("format = 1") && text.contains("[colors.dark]") && text.contains("ui = [\"Foo\"]") && text.contains("style = \"floating\"") && text.contains("height = 30"), "{text}");
        let again = manifest::analyze(&text).def.unwrap();
        assert_eq!(legacy.resolve("o", true, &Selection::default()), again.resolve("o", true, &Selection::default()));
    }
}
