//! Widgets are a desktop preference shared by every theme. Edit the raw JSON so unknown
//! instance options and theme/user metadata survive enable, placement and visibility changes.

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Row};
use crate::widgets::catalog::{self, Entry};
use crate::widgets::config::{self, Instance, Place, WidgetsConfig};
use serde_json::{json, Value};
use std::path::Path;

pub(super) const LIMIT: usize = 32;

pub(super) fn read_widgets(path: &Path) -> Result<Value, String> {
    let doc: Value = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("The desktop settings file is not valid JSON: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("Could not read desktop settings: {e}")),
    };
    if !doc.is_object() {
        return Err("Desktop settings must be a JSON object".into());
    }
    let widgets = doc.get("widgets").cloned().unwrap_or_else(|| json!({}));
    if !widgets.is_object() {
        return Err("The widgets setting must be a JSON object; repair it in Config & Presets".into());
    }
    if widgets.get("items").is_some_and(|i| !i.is_array()) {
        return Err("The widget list must be an array; repair it in Config & Presets".into());
    }
    Ok(widgets)
}

fn entries(widgets: &Value) -> Vec<Entry> {
    catalog::list(&crate::theme::live::Source::discover().themes_dir, widgets.get("allow_exec").and_then(Value::as_bool).unwrap_or(false))
}

fn raw_items(widgets: &Value) -> &[Value] {
    widgets.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn identity_in<'a>(inst: &'a Instance, user: &Path) -> (Option<&'a str>, &'a str, bool) {
    if let Some(id) = inst.use_.strip_prefix("builtin:") {
        (None, id, true)
    } else if inst.source.is_none() && crate::widgets::samples::source(&inst.use_).is_some() && std::fs::symlink_metadata(user.join(format!("{}.toml", inst.use_))).is_err() {
        (None, &inst.use_, true)
    } else {
        (inst.source.as_deref(), &inst.use_, false)
    }
}

fn identity(inst: &Instance) -> (Option<&str>, &str, bool) {
    identity_in(inst, &config::user_dir())
}

fn same_widget(a: &Instance, b: &Instance) -> bool {
    identity(a) == identity(b)
}

fn title(inst: &Instance, catalog: &[Entry]) -> String {
    catalog.iter().find(|e| same_widget(&e.instance, inst)).map(|e| e.name.clone()).unwrap_or_else(|| inst.use_.trim_start_matches("builtin:").replace(['-', '_'], " "))
}

fn button(id: String, label: &str, danger: bool, enabled: bool) -> Ctl {
    let mut c = Ctl::new(id, Kind::Button { label: label.into(), danger });
    c.enabled = enabled;
    c
}

fn stepper(id: String, value: i32) -> Ctl {
    Ctl::new(id, Kind::Stepper { v: value.into(), min: -4000.0, max: 4000.0, step: 4.0, unit: " px".into() })
}

pub fn items(m: &Model) -> Vec<Item> {
    let widgets = match read_widgets(&m.paths.shell_json()) {
        Ok(w) => w,
        Err(e) => return vec![Item::Note { text: e, warn: true }],
    };
    let enabled = widgets.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let catalog = entries(&widgets);
    let mut out = vec![
        Item::Header("Desktop widgets".into()),
        Item::Note { text: "Small, useful cards for your desktop. Themes choose their own widgets. If you customise this layout, applying a theme with widgets asks before replacing it. Themes without widgets hide the collection without deleting your saved layout.".into(), warn: false },
        Item::Card(vec![Row::new("Show widgets").detail("Hide every desktop widget without losing your choices or positions").ctl(Ctl::new("wg:enabled", Kind::Toggle(enabled)))]),
    ];
    out.extend(super::widgetbuilder::items(m));
    out.push(Item::Header("Your widgets".into()));
    if raw_items(&widgets).is_empty() {
        out.push(Item::Note { text: "No widgets added yet. Create one above or choose a widget from the collection below.".into(), warn: false });
    }
    let allow_exec = widgets.get("allow_exec").and_then(Value::as_bool).unwrap_or(false);
    let themes = crate::theme::live::Source::discover().themes_dir;
    for (i, raw) in raw_items(&widgets).iter().enumerate() {
        let prefix = format!("wg:item:{i}");
        let Some(inst) = Instance::from_json(raw) else {
            out.push(Item::Card(vec![Row::new("Unavailable widget").detail("This saved entry is not valid. Remove it or repair it in Config & Presets.").ctl(button(
                format!("{prefix}:remove"),
                "Remove",
                true,
                true,
            ))]));
            continue;
        };
        let resolved = config::resolve(&inst, &themes, allow_exec);
        let detail = match &resolved {
            Ok(r) => r.def.description.clone(),
            Err(e) => format!("Unavailable: {e}"),
        };
        let mut rows =
            vec![Row::new(title(&inst, &catalog)).detail(detail).ctl(Ctl::new(format!("{prefix}:enabled"), Kind::Toggle(inst.enabled))).ctl(button(format!("{prefix}:remove"), "Remove", true, true))];
        if inst.place == Place::Desktop {
            let anchors: Vec<_> = crate::theme::engine::schema::ANCHORS.iter().map(|a| (a.replace('-', " "), (*a).to_string())).collect();
            let selected = anchors.iter().position(|(_, a)| *a == inst.anchor);
            rows.push(Row::new("Position").detail("Anchor the widget to a screen edge or corner").ctl(Ctl::new(format!("{prefix}:anchor"), Kind::Popup(anchors, selected))));
            rows.push(Row::new("Horizontal offset").detail("Move left with negative values, right with positive values").ctl(stepper(format!("{prefix}:x"), inst.x)));
            rows.push(Row::new("Vertical offset").detail("Move up with negative values, down with positive values").ctl(stepper(format!("{prefix}:y"), inst.y)));
        } else {
            rows.push(Row::new("Placement").detail("This widget is embedded in the menu bar"));
        }
        out.push(Item::Card(rows));
    }
    if !enabled && !raw_items(&widgets).is_empty() {
        out.push(Item::Note { text: "Your widgets are hidden. Turn on Show widgets to display the enabled ones.".into(), warn: false });
    }
    out.push(Item::Header("Available widgets".into()));
    let mut available = Vec::new();
    for e in &catalog {
        let matching: Vec<_> = raw_items(&widgets).iter().filter_map(Instance::from_json).filter(|i| same_widget(i, &e.instance)).collect();
        let added = matching.iter().any(|i| i.enabled);
        let label = if added {
            "Added"
        } else if !matching.is_empty() {
            "Enable"
        } else {
            "Add"
        };
        let detail = e.error.as_ref().map(|err| format!("Unavailable: {err}")).unwrap_or_else(|| e.description.clone());
        available.push(Row::new(&e.name).detail(detail).ctl(button(format!("wg:add:{}", e.id), label, false, !added && e.error.is_none())));
    }
    if available.is_empty() {
        out.push(Item::Note { text: "No widget definitions are available.".into(), warn: false });
    } else {
        out.push(Item::Card(available));
    }
    out
}

fn change(widgets: &mut Value, parts: &[&str], value: CtlVal) -> Result<(), String> {
    match (parts, value) {
        (["wg", "enabled"], CtlVal::Bool(on)) => {
            widgets["enabled"] = json!(on);
        }
        (["wg", "add", key], CtlVal::Click) => {
            let catalog = entries(widgets);
            let entry = catalog.iter().find(|e| e.id == *key).ok_or("This widget is no longer available")?;
            if let Some(e) = &entry.error {
                return Err(e.clone());
            }
            let array = widgets.as_object_mut().unwrap().entry("items").or_insert_with(|| json!([])).as_array_mut().unwrap();
            if let Some(raw) = array.iter_mut().find(|v| Instance::from_json(v).is_some_and(|i| same_widget(&i, &entry.instance))) {
                raw["enabled"] = json!(true);
            } else {
                if array.len() >= LIMIT {
                    return Err(format!("You can add up to {LIMIT} widgets. Remove one before adding another."));
                }
                array.push(entry.instance.to_json());
            }
            widgets["enabled"] = json!(true);
        }
        (["wg", "item", index, action], value) => {
            let i = index.parse::<usize>().map_err(|_| "This widget is no longer available")?;
            let allow_exec = widgets.get("allow_exec").and_then(Value::as_bool).unwrap_or(false);
            let array = widgets.get_mut("items").and_then(Value::as_array_mut).ok_or("This widget is no longer available")?;
            if i >= array.len() {
                return Err("This widget is no longer available".into());
            }
            if *action == "remove" && value == CtlVal::Click {
                array.remove(i);
                return Ok(());
            }
            let raw = &mut array[i];
            if !raw.is_object() {
                return Err("This widget entry is invalid; remove it or repair it in Config & Presets".into());
            }
            match (*action, value) {
                ("enabled", CtlVal::Bool(on)) => {
                    if on {
                        let inst = Instance::from_json(raw).ok_or("This widget entry is invalid")?;
                        config::resolve(&inst, &crate::theme::live::Source::discover().themes_dir, allow_exec)?;
                    }
                    raw["enabled"] = json!(on);
                }
                ("anchor", CtlVal::Str(anchor)) if crate::theme::engine::schema::ANCHORS.contains(&anchor.as_str()) => raw["anchor"] = json!(anchor),
                (key @ ("x" | "y"), CtlVal::Num(n)) if n.is_finite() && (-4000.0..=4000.0).contains(&n) => raw[key] = json!(n.round() as i32),
                _ => return Err("That widget setting is not valid".into()),
            }
        }
        _ => return Err("That widget setting is not valid".into()),
    }
    Ok(())
}

pub fn apply(m: &mut Model, parts: &[&str], value: CtlVal) {
    if let ["wg", "new", action] = parts {
        return super::widgetbuilder::apply(m, action, value);
    }
    let mut widgets = match read_widgets(&m.paths.shell_json()) {
        Ok(w) => w,
        Err(e) => {
            m.say(e, true);
            return;
        }
    };
    if let Err(e) = change(&mut widgets, parts, value) {
        m.say(e, true);
        return;
    }
    save(m, widgets, "Widget choices saved and applied");
}

/// Writes the `widgets` object, starts the desktop host when something should show, and reports with `saved`. Returns whether the
/// settings file now holds `widgets`.
pub(super) fn save(m: &mut Model, mut widgets: Value, saved: &str) -> bool {
    widgets.as_object_mut().unwrap().entry("enabled").or_insert(json!(true));
    if widgets["enabled"] == json!(true) {
        widgets.as_object_mut().unwrap().remove("theme_widgets_absent");
    }
    m.edit_json("shell", "widgets", Some(widgets.clone()));
    if m.notice.as_ref().is_some_and(|n| n.error) || m.shell.get("widgets") != Some(&widgets) {
        return false;
    }
    let cfg = WidgetsConfig::from_json(&json!({"widgets": widgets}));
    if cfg.enabled && cfg.items.iter().any(|i| i.enabled && i.place == Place::Desktop) {
        #[cfg(feature = "x11")]
        if let Err(e) = crate::widgets::host::ensure_running() {
            m.say(format!("Widget choices saved, but the desktop could not display them: {e}"), true);
            return true;
        }
        #[cfg(not(feature = "x11"))]
        {
            m.say("Widget choices saved. Desktop widgets require an X11 session in this build.", true);
            return true;
        }
    }
    m.say(if cfg.enabled { saved } else { "Widgets hidden; your choices and positions are saved" }, false);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;

    fn save(m: &Model, widgets: Value) {
        std::fs::write(m.paths.shell_json(), json!({"theme": {"name": "windows11", "palette": "nord"}, "widgets": widgets}).to_string()).unwrap();
    }
    fn load(m: &Model) -> Value {
        serde_json::from_str(&std::fs::read_to_string(m.paths.shell_json()).unwrap()).unwrap()
    }

    #[test]
    fn user_sample_shadow_has_a_distinct_identity_from_builtin() {
        let dir = std::env::temp_dir().join(format!("widget-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bare = Instance::desktop("clock-card", "top-left", 0, 0);
        let builtin = Instance::desktop("builtin:clock-card", "top-left", 0, 0);
        assert_eq!(identity_in(&bare, &dir), identity_in(&builtin, &dir));
        std::fs::write(dir.join("clock-card.toml"), "invalid but shadows the builtin").unwrap();
        assert_ne!(identity_in(&bare, &dir), identity_in(&builtin, &dir));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn master_visibility_and_per_widget_toggles_preserve_every_saved_option() {
        let (mut m, _) = model_with("[general]\n");
        let widget = json!({"use":"builtin:clock-card", "enabled":true, "anchor":"bottom-right", "x":-44, "y":-72, "options":{"format":"24h"}, "future":17});
        save(&m, json!({"items":[widget.clone()],"allow_exec":true,"future":"kept"}));
        m.apply("wg:enabled", CtlVal::Bool(false));
        let doc = load(&m);
        assert_eq!(doc["widgets"]["items"][0], widget);
        assert_eq!(doc["widgets"]["future"], "kept");
        assert_eq!(doc["widgets"]["allow_exec"], true);
        assert_eq!(doc["theme"]["palette"], "nord");
        m.apply("wg:item:0:enabled", CtlVal::Bool(false));
        m.apply("wg:item:0:enabled", CtlVal::Bool(true));
        assert_eq!(load(&m)["widgets"]["items"][0], widget);
        m.apply("wg:enabled", CtlVal::Bool(true));
        assert_eq!(load(&m)["widgets"]["items"][0], widget);
    }

    #[test]
    fn adding_to_hidden_empty_desktop_reveals_collection() {
        let (mut m, _) = model_with("[general]\n");
        save(&m, json!({"enabled":false,"items":[],"future":"preserved"}));
        m.apply("wg:add:builtin.next-clock", CtlVal::Click);
        let doc = load(&m);
        assert_eq!(doc["widgets"]["enabled"], true);
        assert_eq!(doc["widgets"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(doc["widgets"]["future"], "preserved");
    }

    #[test]
    fn disabled_builtin_alias_is_enabled_without_adding_a_duplicate() {
        let (mut m, _) = model_with("[general]\n");
        save(&m, json!({"enabled":false,"items":[{"use":"clock-card", "enabled":false, "x":91, "options":{"a":1}}]}));
        m.apply("wg:add:builtin.clock-card", CtlVal::Click);
        let doc = load(&m);
        assert_eq!(doc["widgets"]["enabled"], true);
        assert_eq!(doc["widgets"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(doc["widgets"]["items"][0]["enabled"], true);
        assert_eq!(doc["widgets"]["items"][0]["x"], 91);
        assert_eq!(doc["widgets"]["items"][0]["options"]["a"], 1);
    }

    #[test]
    fn fresh_disk_state_is_used_and_invalid_files_are_not_replaced() {
        let (mut m, _) = model_with("[general]\n");
        save(&m, json!({"items":[{"use":"builtin:clock-card","enabled":false,"future":3}]}));
        m.apply("wg:item:0:x", CtlVal::Num(-48.0));
        assert_eq!(load(&m)["widgets"]["items"][0]["future"], 3);
        assert_eq!(load(&m)["widgets"]["items"][0]["x"], -48);
        std::fs::write(m.paths.shell_json(), "{not json").unwrap();
        m.apply("wg:enabled", CtlVal::Bool(false));
        assert_eq!(std::fs::read_to_string(m.paths.shell_json()).unwrap(), "{not json");
        assert!(m.notice.as_ref().unwrap().error);
    }

    #[test]
    fn removing_one_instance_preserves_other_instances_and_the_theme() {
        let (mut m, _) = model_with("[general]\n");
        let other = json!({"use":"builtin:cpu-ram-ring", "enabled":false, "x":-20});
        save(&m, json!({"enabled":false,"items":[{"use":"builtin:clock-card"},other.clone()]}));
        m.apply("wg:item:0:remove", CtlVal::Click);
        let doc = load(&m);
        assert_eq!(doc["widgets"]["items"], json!([other]));
        assert_eq!(doc["widgets"]["enabled"], false);
        assert_eq!(doc["theme"]["name"], "windows11");
    }

    #[test]
    fn widgets_have_a_sidebar_page_and_are_found_by_search() {
        let pane = super::super::system::registry::pane_index("widgets").unwrap();
        let hits = super::super::system::registry::search("widgets", &[]);
        assert!(hits.iter().any(|h| h.pane == pane));
        let (m, _) = model_with("[general]\n");
        assert!(m.items(pane).iter().any(|i| matches!(i, Item::Card(rows) if rows.iter().any(|r| r.ctls.iter().any(|c| c.id=="wg:enabled")))));
    }
}
