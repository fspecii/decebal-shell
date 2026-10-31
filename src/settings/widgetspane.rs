//! Widgets are a desktop preference shared by every theme. Edit the raw JSON so unknown
//! instance options and theme/user metadata survive enable, placement and visibility changes.

use super::model::{CtlVal, Model, Notice, Pending};
use super::ui::{Ctl, Item, Kind, Row};
use crate::widgets::catalog::{self, Entry};
use crate::widgets::config::{self, Instance, Place, WidgetsConfig};
use serde_json::{json, Value};
use std::path::Path;

pub(super) const LIMIT: usize = 32;

/// The `widgets` object new edits start from: the shown one while a change is still being saved, else the file.
pub(super) fn read_widgets_now(m: &mut Model) -> Result<Value, String> {
    if m.saving() {
        if let Some(w) = m.json_value_now("shell", "widgets") {
            return Ok(w);
        }
    }
    read_widgets(&m.paths.shell_json())
}

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
    if super::widgetbuilder::creating(m) {
        m.say("Creating your widget; please wait before changing the layout", false);
        return;
    }
    if let ["wg", "new", action] = parts {
        return super::widgetbuilder::apply(m, action, value);
    }
    let mut widgets = match read_widgets_now(m) {
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

/// Label of the readiness task on the settings worker.
const HOST_TASK: &str = "Desktop widgets";

/// Writes the `widgets` object, starts the desktop host when something should show, and reports with `saved`. Returns whether the
/// worker accepted the change (not whether it has committed).
pub(super) fn save(m: &mut Model, widgets: Value, saved: &str) -> bool {
    save_with(m, widgets, saved, crate::widgets::host::ensure_running)
}

/// [`save`] with the host readiness wait given: `readiness` starts the host if needed and waits (bounded) until it has shown the
/// saved configuration. It runs on the ordered settings worker once the file is written, never on the UI thread: until it
/// answers the notice says the desktop is starting; then the saved message (Ready) or the host's problem (Degraded, or it did
/// not start) replaces it.
fn save_with(m: &mut Model, mut widgets: Value, saved: &str, readiness: impl FnOnce() -> Result<(), String> + Send + 'static) -> bool {
    widgets.as_object_mut().unwrap().entry("enabled").or_insert(json!(true));
    if widgets["enabled"] == json!(true) {
        widgets.as_object_mut().unwrap().remove("theme_widgets_absent");
    }
    let cfg = WidgetsConfig::from_json(&json!({"widgets": widgets}));
    let shows = cfg.enabled && cfg.items.iter().any(|i| i.enabled && i.place == Place::Desktop);
    let said = if cfg.enabled { saved.to_string() } else { "Widgets hidden; your choices and positions are saved".to_string() };
    let old_intent = m.widget_intent;
    m.widget_intent = m.widget_intent.wrapping_add(1);
    let intent = m.widget_intent;
    // the host is asked only once the file holds these widgets (a failed write never reaches it), behind every earlier change
    let then: super::model::AfterSave = Box::new(move |m: &mut Model| {
        if m.widget_intent != intent {
            return;
        }
        if shows {
            start_host(m, intent, said, readiness);
        } else {
            m.say(said, false);
        }
    });
    // derived from the widgets as they are now (the file, or the pending change it builds on)
    let base = m.json_value_now("shell", "widgets");
    let accepted = m.try_edit_json_then("shell", "widgets", Some(widgets), base, None, Some(then));
    if !accepted {
        m.widget_intent = old_intent;
    }
    accepted
}

/// After the save: queues the host readiness wait on the settings worker and reports its outcome.
fn start_host(m: &mut Model, intent: u64, said: String, readiness: impl FnOnce() -> Result<(), String> + Send + 'static) {
    if m.widget_intent != intent {
        return;
    }
    let done = move |m: &mut Model, r: Result<serde_json::Value, String>| {
        if m.widget_intent != intent {
            return;
        }
        match r {
            Ok(_) => m.say(said, false),
            Err(e) => m.say(format!("Widget choices saved, but the desktop could not display them: {e}"), true),
        }
    };
    if !m.run_task_quiet_done(HOST_TASK, move || readiness().map(|()| Value::Null), done) {
        // the queue refused the wait (it said why); the choices themselves are on disk
        m.say("Widget choices saved, but Settings could not start the desktop widgets; try again in a moment", true);
        return;
    }
    // still waiting (a worker thread; inline runs have answered already): say what is true right now
    if m.widget_intent == intent && m.pending.values().any(|p| matches!(p, Pending::Task { label: HOST_TASK, .. })) && !m.notice.as_ref().is_some_and(|n| n.error) {
        m.notice = Some(Notice { text: "Widget choices saved; starting the desktop widgets…".into(), error: false, at: std::time::Instant::now() });
        m.dirty = true;
    }
}

/// A newly created widget is already durable before this queues its optional readiness wait.
pub(super) fn created(m: &mut Model, intent: u64, said: String) {
    start_host(m, intent, said, crate::widgets::host::ensure_running);
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

    fn shown(m: &Model) -> (String, bool) {
        m.notice.as_ref().map(|n| (n.text.clone(), n.error)).unwrap_or_default()
    }

    /// A real settings worker thread, as in the app (tests otherwise write inline).
    fn threaded(m: &mut Model) {
        m.use_worker_with_sync_hook(std::sync::Arc::new(|_| Ok(())));
    }

    /// Handles results until nothing is pending (a finished save may queue the host wait behind it).
    fn settle_all(m: &mut Model) {
        for _ in 0..8 {
            m.settle();
            if m.pending.is_empty() {
                return;
            }
        }
        panic!("work still pending");
    }

    #[test]
    fn host_readiness_waits_on_the_settings_worker_after_the_file_is_written() {
        let (mut m, _) = model_with("[general]\n");
        save(&m, json!({"enabled":false,"items":[{"use":"builtin:clock-card","enabled":true,"future":1}],"kept":"yes"}));
        threaded(&mut m);
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let (seen_tx, seen) = std::sync::mpsc::channel::<Value>();
        let path = m.paths.shell_json();
        let started = std::time::Instant::now();
        let mut widgets = read_widgets(&path).unwrap();
        widgets["enabled"] = json!(true);
        let ok = save_with(&mut m, widgets, "Widget choices saved and applied", move || {
            // what the host will read: the saved file, at the time the wait begins
            seen_tx.send(serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()).unwrap();
            gate.recv_timeout(std::time::Duration::from_secs(20)).map_err(|_| "test gate".to_string())
        });
        assert!(ok, "the choices are accepted at once");
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "the UI thread does not wait for the host");
        assert_eq!(m.shell["widgets"]["enabled"], true, "shown while the worker writes it");
        // the UI thread handles the finished write (as its event loop would); only then is the wait queued
        let t0 = std::time::Instant::now();
        while m.pending.values().any(|p| matches!(p, Pending::Json { .. })) {
            assert!(t0.elapsed() < std::time::Duration::from_secs(20), "the write did not finish");
            let evs: Vec<_> = m.queue.lock().unwrap().drain(..).collect();
            evs.into_iter().for_each(|e| m.on_event(e));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let doc = seen.recv_timeout(std::time::Duration::from_secs(20)).expect("the wait runs on the worker");
        assert_eq!(doc["widgets"]["enabled"], true, "the host is asked only after the file holds the new choices");
        assert_eq!(doc["widgets"]["items"][0]["future"], 1);
        assert_eq!(doc["widgets"]["kept"], "yes", "unknown keys survive");
        // saved, the wait has not answered: the notice says the desktop is starting, not that it shows the widgets
        assert_eq!(shown(&m), ("Widget choices saved; starting the desktop widgets…".to_string(), false));
        release.send(()).unwrap();
        settle_all(&mut m);
        assert_eq!(shown(&m), ("Widget choices saved and applied".to_string(), false), "ready: the saved message");
    }

    #[test]
    fn a_degraded_or_absent_host_is_reported_after_the_save_and_hidden_widgets_never_start_one() {
        let (mut m, _) = model_with("[general]\n");
        save(&m, json!({"items":[{"use":"builtin:clock-card","enabled":true}]}));
        threaded(&mut m);
        let widgets = read_widgets(&m.paths.shell_json()).unwrap();
        assert!(save_with(&mut m, widgets, "Widget choices saved and applied", || Err("1 of 1 widget has no display to go on".into())));
        settle_all(&mut m);
        let (text, error) = shown(&m);
        assert!(error && text.starts_with("Widget choices saved, but the desktop could not display them: 1 of 1"), "{text}");
        assert_eq!(load(&m)["widgets"]["items"][0]["use"], "builtin:clock-card", "the choices stay saved");
        // hidden, or nothing on the desktop: no host is asked for
        let mut widgets = read_widgets(&m.paths.shell_json()).unwrap();
        widgets["enabled"] = json!(false);
        assert!(save_with(&mut m, widgets, "unused", || panic!("no host for hidden widgets")));
        settle_all(&mut m);
        assert_eq!(shown(&m), ("Widgets hidden; your choices and positions are saved".to_string(), false));
        let mut widgets = read_widgets(&m.paths.shell_json()).unwrap();
        widgets["enabled"] = json!(true);
        widgets["items"][0]["enabled"] = json!(false);
        assert!(save_with(&mut m, widgets, "Saved without cards", || panic!("no host without enabled cards")));
        settle_all(&mut m);
        assert_eq!(shown(&m), ("Saved without cards".to_string(), false));
    }

    #[test]
    fn an_older_readiness_result_cannot_replace_a_newer_hidden_notice() {
        for failed in [false, true] {
            let (mut m, _) = model_with("[general]\n");
            save(&m, json!({"enabled":false,"items":[{"use":"builtin:clock-card","enabled":true}]}));
            threaded(&mut m);
            let (release, wait) = std::sync::mpsc::channel();
            let (entered, started) = std::sync::mpsc::channel();
            let mut enabled = read_widgets(&m.paths.shell_json()).unwrap();
            enabled["enabled"] = json!(true);
            assert!(save_with(&mut m, enabled, "Old enabled notice", move || {
                entered.send(()).unwrap();
                wait.recv_timeout(std::time::Duration::from_secs(20)).map_err(|e| e.to_string())?;
                if failed {
                    Err("obsolete failure".into())
                } else {
                    Ok(())
                }
            }));
            // Dispatch only the save completion; the readiness task then blocks in the worker.
            m.settle();
            started.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
            let mut hidden = read_widgets(&m.paths.shell_json()).unwrap();
            hidden["enabled"] = json!(false);
            assert!(save_with(&mut m, hidden, "unused", || panic!("hidden layout has no host wait")));
            release.send(()).unwrap();
            settle_all(&mut m);
            assert_eq!(load(&m)["widgets"]["enabled"], false);
            assert_eq!(shown(&m), ("Widgets hidden; your choices and positions are saved".into(), false));
        }
    }

    #[test]
    fn a_failed_write_never_asks_the_host() {
        let (mut m, _) = model_with("[general]\n");
        std::fs::write(m.paths.shell_json(), "{not json").unwrap();
        let widgets = json!({"enabled":true,"items":[{"use":"builtin:clock-card","enabled":true}]});
        assert!(save_with(&mut m, widgets, "unused", || panic!("the host must not be asked for an unsaved change")), "accepted and failed are distinct outcomes");
        assert!(m.notice.as_ref().unwrap().error);
        assert_eq!(std::fs::read_to_string(m.paths.shell_json()).unwrap(), "{not json");
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
