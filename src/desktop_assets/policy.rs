//! Who chose the icon and the cursor theme, and what applying a theme may change.
//!
//! The values themselves stay where they always were: `icons.theme` (`docs/icons.md`) and `system.cursorTheme`
//! (written to GTK / gsettings / X resources by "Apply to all apps"). `asset_packs` in `decebal.json` records, per
//! slot, either `{"user": true}` (an explicit choice: themes never change it) or what a theme set
//! (`{"theme", "applied", "baseline"}`: `baseline` is put back when a later theme recommends nothing), plus the
//! current theme's `recommended` packs. A value that no longer equals `applied` was edited by hand: it becomes explicit.

use super::catalog;
use super::Provides;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const KEY: &str = "asset_packs";

/// One slot's record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotState {
    pub user: bool,
    pub theme: Option<String>,
    pub applied: Option<String>,
    pub baseline: Option<String>,
}

/// Who decides a slot now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    User,
    Theme(String),
    /// Nobody chose: the desktop's / toolkit's default.
    Default,
}

/// What the theme being applied suggests, resolved to installed theme names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recommendation {
    pub theme: String,
    pub icons: Option<String>,
    pub cursors: Option<String>,
    /// Recommended catalogue packs that are not installed (pack ids).
    pub missing: Vec<String>,
}

impl Recommendation {
    pub fn get(&self, k: Provides) -> Option<&str> {
        match k {
            Provides::Icons => self.icons.as_deref(),
            Provides::Cursors => self.cursors.as_deref(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_none() && self.cursors.is_none() && self.missing.is_empty()
    }

    fn to_json(&self) -> Value {
        json!({"theme": self.theme, "icons": self.icons, "cursors": self.cursors, "missing": self.missing})
    }

    pub fn from_doc(doc: &Value) -> Option<Recommendation> {
        let r = doc.get(KEY)?.get("recommended")?;
        let s = |k: &str| r.get(k).and_then(Value::as_str).map(String::from);
        Some(Recommendation {
            theme: s("theme").unwrap_or_default(),
            icons: s("icons"),
            cursors: s("cursors"),
            missing: r.get("missing").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        })
    }
}

/// The selected theme name of a slot; `None` = automatic / unset.
pub fn current(doc: &Value, k: Provides) -> Option<String> {
    let v = match k {
        Provides::Icons => doc.get("icons").and_then(|i| i.get("theme")).and_then(Value::as_str),
        Provides::Cursors => doc.get("system").and_then(|s| s.get("cursorTheme")).and_then(Value::as_str),
    };
    v.map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto")).map(String::from)
}

fn obj<'a>(v: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let o = v.as_object_mut().expect("decebal.json is an object");
    let e = o.entry(key.to_string()).or_insert_with(|| json!({}));
    if !e.is_object() {
        *e = json!({});
    }
    e.as_object_mut().expect("object")
}

pub fn set_current(doc: &mut Value, k: Provides, v: Option<&str>) {
    match k {
        Provides::Icons => {
            obj(doc, "icons").insert("theme".into(), json!(v.unwrap_or("auto")));
        }
        Provides::Cursors => match v {
            Some(v) => {
                obj(doc, "system").insert("cursorTheme".into(), json!(v));
            }
            None => {
                if let Some(s) = doc.get_mut("system").and_then(Value::as_object_mut) {
                    s.remove("cursorTheme");
                    if s.is_empty() {
                        doc.as_object_mut().expect("object").remove("system");
                    }
                }
            }
        },
    }
}

pub fn state(doc: &Value, k: Provides) -> SlotState {
    let Some(s) = doc.get(KEY).and_then(|a| a.get(k.id())) else { return SlotState::default() };
    let t = |key: &str| s.get(key).and_then(Value::as_str).map(String::from);
    SlotState { user: s.get("user").and_then(Value::as_bool).unwrap_or(false), theme: t("theme"), applied: t("applied"), baseline: t("baseline") }
}

fn put_state(doc: &mut Value, k: Provides, st: &SlotState) {
    let a = obj(doc, KEY);
    if st.user {
        a.insert(k.id().into(), json!({"user": true}));
    } else if st.applied.is_some() {
        a.insert(k.id().into(), json!({"theme": st.theme, "applied": st.applied, "baseline": st.baseline}));
    } else {
        a.remove(k.id());
    }
    if a.is_empty() {
        doc.as_object_mut().expect("object").remove(KEY);
    }
}

pub fn owner(doc: &Value, k: Provides) -> Owner {
    let st = state(doc, k);
    let cur = current(doc, k);
    if st.user || (st.applied.is_some() && cur != st.applied) {
        Owner::User
    } else if let (Some(t), Some(_)) = (&st.theme, &st.applied) {
        Owner::Theme(t.clone())
    } else {
        // nothing chosen, or set before decebal recorded who chose it: the next theme's suggestion may change it
        Owner::Default
    }
}

/// Applies a theme's recommendation to `doc` (a parsed `decebal.json`): explicit choices stay, theme-owned slots
/// follow the new theme (or go back to their baseline). Returns notes for the apply report.
pub fn apply_theme(doc: &mut Value, rec: &Recommendation) -> Vec<String> {
    let mut notes = Vec::new();
    for k in [Provides::Icons, Provides::Cursors] {
        let mut st = state(doc, k);
        let cur = current(doc, k);
        if !st.user && st.applied.is_some() && cur != st.applied {
            st.user = true; // edited by hand since a theme set it
        }
        let want = rec.get(k).map(String::from);
        if st.user {
            if let Some(w) = &want {
                if cur.as_deref() != Some(w) {
                    notes.push(format!("{} theme `{}` kept (your choice); the theme suggests `{w}`", k.id().trim_end_matches('s'), cur.as_deref().unwrap_or("auto")));
                }
            }
        } else if let Some(w) = want {
            if st.applied.is_none() {
                st.baseline = cur.clone();
            }
            set_current(doc, k, Some(&w));
            st.applied = Some(w);
            st.theme = Some(rec.theme.clone());
        } else if st.applied.is_some() {
            set_current(doc, k, st.baseline.as_deref());
            st = SlotState::default();
        }
        put_state(doc, k, &st);
    }
    for m in &rec.missing {
        notes.push(format!("the theme suggests the asset pack `{m}`, which is not installed (decthemectl assets install {m})"));
    }
    if rec.is_empty() {
        if let Some(a) = doc.get_mut(KEY).and_then(Value::as_object_mut) {
            a.remove("recommended");
            if a.is_empty() {
                doc.as_object_mut().expect("object").remove(KEY);
            }
        }
    } else {
        obj(doc, KEY).insert("recommended".into(), rec.to_json());
    }
    notes
}

/// What the user picked for a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Name(String),
    Auto,
    /// Give the slot back to the theme's recommendation.
    Theme,
}

pub fn choose(doc: &mut Value, k: Provides, c: &Choice) {
    match c {
        Choice::Name(n) => {
            set_current(doc, k, Some(n));
            put_state(doc, k, &SlotState { user: true, ..SlotState::default() });
        }
        Choice::Auto => {
            set_current(doc, k, None);
            put_state(doc, k, &SlotState { user: true, ..SlotState::default() });
        }
        Choice::Theme => {
            let rec = Recommendation::from_doc(doc).unwrap_or_default();
            let st = state(doc, k);
            let mut fresh = SlotState::default();
            if let Some(w) = rec.get(k).map(String::from) {
                fresh.baseline = if st.applied.is_some() { st.baseline } else { current(doc, k) };
                set_current(doc, k, Some(&w));
                fresh.applied = Some(w);
                fresh.theme = Some(rec.theme.clone());
            }
            put_state(doc, k, &fresh);
        }
    }
}

/// Marks a slot as chosen by the user, in the file (the Icons pane, `decthemectl icons set`, the cursor setting).
/// One transaction on the file as it is (Settings' `json_update_doc`: under the file lock, applied again on top of a
/// save made meanwhile), so a concurrent writer's keys, also inside `asset_packs`, are never overwritten with a stale copy.
pub fn mark_user(shell_json: &Path, k: Provides) -> Result<(), String> {
    crate::settings::io::json_update_doc(shell_json, |doc| {
        if !state(doc, k).user {
            put_state(doc, k, &SlotState { user: true, ..SlotState::default() });
        }
        Ok(())
    })
    .map(|_| ())
}

/// Changes the slot in `shell_json`: the selected value (`icons.theme` or `system.cursorTheme`) and who owns it
/// (`asset_packs`) in one atomic transaction on the file as it is; every other key, and every key another writer
/// changed meanwhile, is kept. The same change the Icons pane makes through `edit_shell_doc`.
pub fn choose_in_file(shell_json: &Path, k: Provides, c: &Choice) -> Result<(), String> {
    crate::settings::io::json_update_doc(shell_json, |doc| {
        choose(doc, k, c);
        Ok(())
    })
    .map(|_| ())
}

/// Is a theme of this name installed and usable for `k`?
pub fn is_installed(roots: &crate::icons::resolve::Roots, k: Provides, name: &str) -> bool {
    match k {
        Provides::Icons => crate::icons::resolve::list_themes(roots).iter().any(|t| t.id == name && t.kind.selectable()),
        Provides::Cursors => super::cursor::usable(roots, name),
    }
}

/// The recommendation for `theme_id`: its companion `[[recommend]]` (when the pack is installed), else the theme's
/// own `[icons] theme` for icons (the behaviour before asset packs).
pub fn recommendation(set: &catalog::Set, theme_id: &str, manifest_icons: Option<&str>, installed: &dyn Fn(Provides, &str) -> bool) -> Recommendation {
    let mut r = Recommendation { theme: theme_id.to_string(), ..Recommendation::default() };
    if let Some(c) = set.recommendation(theme_id) {
        for (k, id) in [(Provides::Icons, &c.icons), (Provides::Cursors, &c.cursors)] {
            let Some(id) = id else { continue };
            let Some(e) = set.find(id) else { continue };
            if installed(k, &e.pack.install_name) {
                match k {
                    Provides::Icons => r.icons = Some(e.pack.install_name.clone()),
                    Provides::Cursors => r.cursors = Some(e.pack.install_name.clone()),
                }
            } else if !r.missing.contains(&e.pack.id) {
                r.missing.push(e.pack.id.clone());
            }
        }
    }
    if r.icons.is_none() {
        r.icons = manifest_icons.map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto")).map(String::from);
    }
    r
}

/// The hook `theme apply` calls on the `decebal.json` it is about to write.
pub fn on_theme_apply(shell: &mut Value, theme_id: &str, manifest_icons: Option<&str>) -> Vec<String> {
    let set = catalog::Set::load();
    let roots = super::roots_with(&super::default_root());
    let rec = recommendation(&set, theme_id, manifest_icons, &|k, n| is_installed(&roots, k, n));
    apply_theme(shell, &rec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(theme: &str, icons: Option<&str>, cursors: Option<&str>) -> Recommendation {
        Recommendation { theme: theme.into(), icons: icons.map(String::from), cursors: cursors.map(String::from), missing: vec![] }
    }

    #[test]
    fn theme_recommendations_layer_over_the_baseline() {
        let mut d = json!({"icons": {"theme": "Papirus", "size_scale": 0.9}, "system": {"enabled": true, "cursorTheme": "Adwaita"}});
        apply_theme(&mut d, &rec("deepin", Some("bloom"), Some("bloom")));
        assert_eq!((current(&d, Provides::Icons).as_deref(), current(&d, Provides::Cursors).as_deref()), (Some("bloom"), Some("bloom")));
        assert_eq!(d["icons"]["size_scale"], 0.9, "other icon keys stay");
        assert_eq!(owner(&d, Provides::Icons), Owner::Theme("deepin".into()));
        assert_eq!(d[KEY]["recommended"]["icons"], "bloom");
        apply_theme(&mut d, &rec("lingmo", Some("ocean"), None));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("ocean"));
        assert_eq!(current(&d, Provides::Cursors).as_deref(), Some("Adwaita"), "no cursor suggestion: the baseline is back");
        assert_eq!(state(&d, Provides::Icons).baseline.as_deref(), Some("Papirus"), "the baseline survives theme to theme");
        apply_theme(&mut d, &Recommendation { theme: "nord".into(), ..Default::default() });
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Papirus"));
        assert!(d.get(KEY).is_none(), "nothing left to record");
        assert_eq!(d["system"]["enabled"], true);
    }

    #[test]
    fn explicit_choices_and_hand_edits_survive_themes() {
        let mut d = json!({});
        choose(&mut d, Provides::Icons, &Choice::Name("Papirus".into()));
        choose(&mut d, Provides::Cursors, &Choice::Auto);
        let notes = apply_theme(&mut d, &rec("deepin", Some("bloom"), Some("bloom")));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Papirus"));
        assert_eq!(current(&d, Provides::Cursors), None, "an explicit auto is a choice too");
        assert!(notes.iter().any(|n| n.contains("kept (your choice)") && n.contains("bloom")), "{notes:?}");
        assert_eq!(owner(&d, Provides::Icons), Owner::User);
        // hand edit after a theme set the value
        let mut d = json!({});
        apply_theme(&mut d, &rec("deepin", Some("bloom"), None));
        d["icons"]["theme"] = json!("Tela");
        assert_eq!(owner(&d, Provides::Icons), Owner::User);
        apply_theme(&mut d, &rec("lingmo", Some("ocean"), None));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Tela"));
        assert!(state(&d, Provides::Icons).user, "recorded as explicit");
        // handing it back to the theme
        choose(&mut d, Provides::Icons, &Choice::Theme);
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("ocean"));
        assert_eq!(owner(&d, Provides::Icons), Owner::Theme("lingmo".into()));
        assert_eq!(state(&d, Provides::Icons).baseline.as_deref(), Some("Tela"));
        apply_theme(&mut d, &rec("nord", None, None));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Tela"));
    }

    #[test]
    fn recommendations_count_only_installed_packs_and_fall_back_to_the_manifest() {
        let mut set = catalog::Set::default();
        set.push(catalog::parse(catalog::tests::GOOD, "t").unwrap());
        let none = |_: Provides, _: &str| false;
        let r = recommendation(&set, "deepin", Some("Adwaita"), &none);
        assert_eq!((r.icons.as_deref(), r.cursors.as_deref(), r.missing.as_slice()), (Some("Adwaita"), None, &["deepin-bloom".to_string()][..]));
        let all = |_: Provides, n: &str| n == "bloom";
        let r = recommendation(&set, "deepin", Some("Adwaita"), &all);
        assert_eq!((r.icons.as_deref(), r.cursors.as_deref()), (Some("bloom"), Some("bloom")));
        let r = recommendation(&set, "nord", Some("auto"), &all);
        assert!(r.is_empty());
        let mut d = json!({});
        let notes = apply_theme(&mut d, &recommendation(&set, "deepin", None, &none));
        assert!(notes.iter().any(|n| n.contains("assets install deepin-bloom")), "{notes:?}");
        assert_eq!(d[KEY]["recommended"]["missing"][0], "deepin-bloom");
        assert!(d.get("icons").is_none(), "nothing installed: nothing changes");
    }

    #[test]
    fn file_choices_are_one_transaction_and_keep_concurrent_writers() {
        let s = super::super::testkit::Scratch::new("policy-race");
        let f = s.path().join("decebal.json");
        std::fs::write(&f, r#"{"theme": {"name": "nord"}, "asset_packs": {"recommended": {"theme": "deepin", "count": 0}}}"#).unwrap();
        const N: i64 = 150;
        // another writer keeps changing a key inside `asset_packs`, the object a choice rewrites
        let f2 = f.clone();
        let other = std::thread::spawn(move || {
            for _ in 0..N {
                crate::settings::io::json_update_doc(&f2, |d| {
                    let n = d[KEY]["recommended"]["count"].as_i64().unwrap_or(0);
                    d[KEY]["recommended"]["count"] = json!(n + 1);
                    Ok(())
                })
                .unwrap();
            }
        });
        for i in 0..N {
            let name = if i % 2 == 0 { "Papirus" } else { "bloom" };
            choose_in_file(&f, Provides::Icons, &Choice::Name(name.into())).unwrap();
            mark_user(&f, Provides::Cursors).unwrap();
        }
        other.join().unwrap();
        let d = crate::settings::io::json_load(&f);
        assert_eq!(d[KEY]["recommended"]["count"].as_i64(), Some(N), "no increment of the other writer was overwritten by a stale copy");
        assert_eq!(d[KEY]["recommended"]["theme"], "deepin");
        assert_eq!((current(&d, Provides::Icons).as_deref(), owner(&d, Provides::Icons)), (Some("bloom"), Owner::User), "the value and its owner agree");
        assert!(state(&d, Provides::Cursors).user);
        assert_eq!(d["theme"]["name"], "nord");
        // handing a slot back to the theme is one transaction too
        choose_in_file(&f, Provides::Icons, &Choice::Theme).unwrap();
        let d = crate::settings::io::json_load(&f);
        assert_eq!(state(&d, Provides::Icons), SlotState::default(), "no recommendation for icons: the slot is released");
        assert_eq!(d[KEY]["recommended"]["count"].as_i64(), Some(N));
        // a file that is not a JSON object is refused, not overwritten
        std::fs::write(&f, "[1, 2]").unwrap();
        assert!(choose_in_file(&f, Provides::Icons, &Choice::Auto).is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[1, 2]");
    }

    #[test]
    fn file_helpers_touch_only_their_keys() {
        let s = super::super::testkit::Scratch::new("policy");
        let f = s.path().join("decebal.json");
        std::fs::write(&f, r#"{"theme": {"name": "nord"}, "icons": {"theme": "Adwaita", "fallback": "none"}, "wallpaper": {"x": 1}}"#).unwrap();
        mark_user(&f, Provides::Icons).unwrap();
        choose_in_file(&f, Provides::Cursors, &Choice::Name("DMZ-White".into())).unwrap();
        let d = crate::settings::io::json_load(&f);
        assert_eq!((d[KEY]["icons"]["user"].as_bool(), d["system"]["cursorTheme"].as_str()), (Some(true), Some("DMZ-White")));
        assert_eq!((d["theme"]["name"].as_str(), d["wallpaper"]["x"].as_i64(), d["icons"]["fallback"].as_str()), (Some("nord"), Some(1), Some("none")));
    }
}
