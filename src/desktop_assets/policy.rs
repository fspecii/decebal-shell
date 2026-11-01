//! Who chose the icon and the cursor theme, and what applying a theme may change.
//!
//! The values themselves stay where they always were: `icons.theme` (`docs/icons.md`) and `system.cursorTheme`
//! (written to GTK / gsettings / X resources by "Apply to all apps"). `asset_packs` in `decebal.json` records, per
//! slot, its mode: **follow** (the default: applying a theme sets its matching pack, `{"mode": "follow", "theme",
//! "applied", "baseline"}`; `baseline` is put back when a later theme recommends nothing) or **custom** (`{"mode":
//! "custom"}`: an explicit choice themes never change). `restore` keeps the last custom value while a slot follows the
//! theme. Also: the current theme's `recommended` packs, `generation` (bumped by every theme apply and choice: a
//! background install applies only when it is unchanged) and `downloads` (`ask` turns automatic downloads off). A value
//! that no longer equals `applied` was edited by hand: it becomes custom. An old `{"user": true}` (a manual choice from
//! before slot modes) is custom too: no theme and no late background install ever overrides a manual choice.

use super::catalog;
use super::Provides;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const KEY: &str = "asset_packs";

/// Who decides a slot: the theme (the default) or the user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Applying a theme sets its matching pack (downloaded in the background when it is missing).
    #[default]
    Follow,
    /// An explicit choice: themes never change it, until the user picks Follow theme again.
    Custom,
}

/// One slot's record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotState {
    pub mode: Mode,
    /// A `{"user": true}` record written before slot modes existed: a manual choice, kept as custom.
    pub legacy: bool,
    pub theme: Option<String>,
    pub applied: Option<String>,
    pub baseline: Option<String>,
    /// The last custom value (`auto` for "not set"), offered for restoration while the slot follows the theme.
    pub restore: Option<String>,
}

impl SlotState {
    pub fn is_custom(&self) -> bool {
        self.mode == Mode::Custom
    }
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
    /// The recommended pack of each slot (pack id), installed or not.
    pub icons_pack: Option<String>,
    pub cursors_pack: Option<String>,
    /// How the icon recommendation was found: `catalogue`, `manifest` or `coverage:<basis>`, and why.
    pub basis: Option<String>,
}

impl Recommendation {
    pub fn get(&self, k: Provides) -> Option<&str> {
        match k {
            Provides::Icons => self.icons.as_deref(),
            Provides::Cursors => self.cursors.as_deref(),
        }
    }

    pub fn pack(&self, k: Provides) -> Option<&str> {
        match k {
            Provides::Icons => self.icons_pack.as_deref(),
            Provides::Cursors => self.cursors_pack.as_deref(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_none() && self.cursors.is_none() && self.missing.is_empty()
    }

    fn to_json(&self) -> Value {
        json!({"theme": self.theme, "icons": self.icons, "cursors": self.cursors, "missing": self.missing,
               "icons_pack": self.icons_pack, "cursors_pack": self.cursors_pack, "basis": self.basis})
    }

    pub fn from_doc(doc: &Value) -> Option<Recommendation> {
        let r = doc.get(KEY)?.get("recommended")?;
        let s = |k: &str| r.get(k).and_then(Value::as_str).map(String::from);
        Some(Recommendation {
            theme: s("theme").unwrap_or_default(),
            icons: s("icons"),
            cursors: s("cursors"),
            missing: r.get("missing").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            icons_pack: s("icons_pack"),
            cursors_pack: s("cursors_pack"),
            basis: s("basis"),
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
            // One icon choice drives both shell and toolkit apps. Retire the older second authority.
            if let Some(system) = doc.get_mut("system").and_then(Value::as_object_mut) {
                system.remove("iconTheme");
            }
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
    let user = s.get("user").and_then(Value::as_bool).unwrap_or(false);
    let mode = match t("mode").as_deref() {
        Some("custom") => Mode::Custom,
        Some(_) => Mode::Follow,
        None if user => Mode::Custom,
        None => Mode::Follow,
    };
    SlotState { mode, legacy: user && t("mode").is_none(), theme: t("theme"), applied: t("applied"), baseline: t("baseline"), restore: t("restore") }
}

fn put_state(doc: &mut Value, k: Provides, st: &SlotState) {
    let a = obj(doc, KEY);
    let mut o = Map::new();
    match st.mode {
        Mode::Custom if st.legacy => {
            o.insert("user".into(), json!(true));
        }
        Mode::Custom => {
            o.insert("mode".into(), json!("custom"));
        }
        Mode::Follow => {
            o.insert("mode".into(), json!("follow"));
            for (key, v) in [("theme", &st.theme), ("applied", &st.applied), ("baseline", &st.baseline)] {
                if v.is_some() {
                    o.insert(key.into(), json!(v));
                }
            }
        }
    }
    if let Some(r) = &st.restore {
        o.insert("restore".into(), json!(r));
    }
    if st.mode == Mode::Follow && st.applied.is_none() && st.theme.is_none() && st.restore.is_none() {
        a.remove(k.id());
    } else {
        a.insert(k.id().into(), Value::Object(o));
    }
    if a.is_empty() {
        doc.as_object_mut().expect("object").remove(KEY);
    }
}

/// The theme id `decebal.json` selects (`theme.name`, what `theme apply` writes with the slots).
pub fn selected_theme(doc: &Value) -> Option<&str> {
    doc.get("theme")?.get("name")?.as_str()
}

/// The counter every theme apply and every slot choice bumps: a background completion applies only when it is unchanged.
pub fn generation(doc: &Value) -> u64 {
    doc.get(KEY).and_then(|a| a.get("generation")).and_then(Value::as_u64).unwrap_or(0)
}

/// The stored value is external input: anything but a u64 counts as 0, and the maximum wraps to 1 instead of panicking
/// (only equality is ever compared, never order).
fn bump(doc: &mut Value) -> u64 {
    let g = generation(doc).checked_add(1).unwrap_or(1);
    obj(doc, KEY).insert("generation".into(), json!(g));
    g
}

/// Whether missing matching packs are downloaded by themselves (`auto`, the default) or only offered (`ask`).
pub fn downloads_auto(doc: &Value) -> bool {
    doc.get(KEY).and_then(|a| a.get("downloads")).and_then(Value::as_str) != Some("ask")
}

pub fn set_downloads(doc: &mut Value, auto: bool) {
    let a = obj(doc, KEY);
    if auto {
        a.remove("downloads");
    } else {
        a.insert("downloads".into(), json!("ask"));
    }
    if a.is_empty() {
        doc.as_object_mut().expect("object").remove(KEY);
    }
}

pub fn owner(doc: &Value, k: Provides) -> Owner {
    let st = state(doc, k);
    let cur = current(doc, k);
    if st.is_custom() || ((st.applied.is_some() || st.theme.is_some()) && cur != st.applied) {
        Owner::User
    } else if let (Some(t), Some(_)) = (&st.theme, &st.applied) {
        Owner::Theme(t.clone())
    } else {
        // nothing chosen, or set before decebal recorded who chose it: the next theme sets it
        Owner::Default
    }
}

fn label(k: Provides) -> &'static str {
    match k {
        Provides::Icons => "icon theme",
        Provides::Cursors => "cursor theme",
    }
}

/// Applies a theme's recommendation to `doc` (a parsed `decebal.json`): slots that follow the theme take its pack (or,
/// while that pack is being downloaded, keep their value), custom slots stay. Returns notes for the apply report.
pub fn apply_theme(doc: &mut Value, rec: &Recommendation) -> Vec<String> {
    let mut notes = Vec::new();
    let auto = downloads_auto(doc);
    for k in [Provides::Icons, Provides::Cursors] {
        let mut st = state(doc, k);
        let cur = current(doc, k);
        // an old `{"user": true}` record is a manual choice like any custom one: kept (Follow theme in Icons hands it
        // back), never overridden by a theme or a late download
        if st.mode == Mode::Follow && (st.applied.is_some() || st.theme.is_some()) && cur != st.applied {
            // edited by hand since a theme set it: that is a custom choice
            st = SlotState { mode: Mode::Custom, ..SlotState::default() };
        }
        let want = rec.get(k).map(String::from);
        if st.is_custom() {
            if let Some(w) = want.as_deref().or(rec.pack(k)) {
                if cur.as_deref() != Some(w) {
                    notes.push(format!("{} `{}` kept (custom); the theme matches `{w}` (Settings > Icons > Follow theme)", label(k), cur.as_deref().unwrap_or("auto")));
                }
            }
        } else if let Some(w) = want {
            if st.applied.is_none() {
                st.baseline = cur.clone();
            }
            set_current(doc, k, Some(&w));
            st.applied = Some(w);
            st.theme = Some(rec.theme.clone());
        } else if let Some(p) = rec.pack(k) {
            // the matching pack is missing: keep what is shown until it is installed (no flash to something else)
            if st.applied.is_none() {
                st.baseline = cur.clone();
            }
            st.applied = cur.clone();
            st.theme = Some(rec.theme.clone());
            // with automatic downloads the background job's own note says what is fetched (follow::request)
            if !auto {
                notes.push(format!("the theme's {} `{p}` is not installed (downloads are set to ask: decthemectl assets install {p})", label(k)));
            }
        } else if st.applied.is_some() || st.theme.is_some() {
            // the theme matches nothing for this slot: back to the value from before themes set it
            set_current(doc, k, st.baseline.as_deref());
            st = SlotState { restore: st.restore, ..SlotState::default() };
        }
        put_state(doc, k, &st);
    }
    if rec.is_empty() && rec.icons_pack.is_none() && rec.cursors_pack.is_none() {
        if let Some(a) = doc.get_mut(KEY).and_then(Value::as_object_mut) {
            a.remove("recommended");
        }
    } else {
        obj(doc, KEY).insert("recommended".into(), rec.to_json());
    }
    bump(doc);
    notes
}

/// What the user picked for a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Custom: this theme.
    Name(String),
    /// Custom: not set (the toolkit default).
    Auto,
    /// Follow the theme again (the default): the custom value is kept for restoration.
    Theme,
    /// Custom again, with the value kept when the slot went back to the theme.
    Restore,
}

pub fn choose(doc: &mut Value, k: Provides, c: &Choice) {
    let st = state(doc, k);
    match c {
        Choice::Name(n) => {
            set_current(doc, k, Some(n));
            put_state(doc, k, &SlotState { mode: Mode::Custom, ..SlotState::default() });
        }
        Choice::Auto => {
            set_current(doc, k, None);
            put_state(doc, k, &SlotState { mode: Mode::Custom, ..SlotState::default() });
        }
        Choice::Restore => {
            let Some(r) = st.restore.clone() else { return };
            set_current(doc, k, Some(r.as_str()).filter(|r| *r != "auto"));
            put_state(doc, k, &SlotState { mode: Mode::Custom, ..SlotState::default() });
        }
        Choice::Theme => {
            let rec = Recommendation::from_doc(doc).unwrap_or_default();
            let cur = current(doc, k);
            let restore = if st.is_custom() { Some(cur.clone().unwrap_or_else(|| "auto".into())) } else { st.restore.clone() };
            let mut fresh = SlotState { restore, ..SlotState::default() };
            if let Some(w) = rec.get(k).map(String::from) {
                fresh.baseline = if st.applied.is_some() && !st.is_custom() { st.baseline } else { cur.clone() };
                set_current(doc, k, Some(&w));
                fresh.applied = Some(w);
                fresh.theme = Some(rec.theme.clone());
            } else if rec.pack(k).is_some() {
                // its pack is still missing: the background workflow applies it once installed
                fresh.baseline = cur.clone();
                fresh.applied = cur;
                fresh.theme = Some(rec.theme.clone());
            }
            put_state(doc, k, &fresh);
        }
    }
    if k == Provides::Icons {
        if let Some(system) = doc.get_mut("system").and_then(Value::as_object_mut) {
            system.remove("iconTheme");
        }
    }
    bump(doc);
}

/// Marks a slot as chosen by the user, in the file (`decthemectl icons set`, the cursor setting).
/// One transaction on the file as it is (Settings' `json_update_doc`: under the file lock, applied again on top of a
/// save made meanwhile), so a concurrent writer's keys, also inside `asset_packs`, are never overwritten with a stale copy.
pub fn mark_user(shell_json: &Path, k: Provides) -> Result<(), String> {
    crate::settings::io::json_update_doc(shell_json, |doc| {
        if !state(doc, k).is_custom() || state(doc, k).legacy {
            let restore = state(doc, k).restore;
            put_state(doc, k, &SlotState { mode: Mode::Custom, restore, ..SlotState::default() });
        }
        bump(doc);
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

/// A pack installed in the background for generation `g` of the theme `theme`: the slots that still follow that theme
/// (same generation, same recommendation, value unchanged) take it. Returns what was applied, or why nothing was.
pub fn complete(doc: &mut Value, g: u64, theme: &str, installed: &[(Provides, String)]) -> Result<Vec<(Provides, String)>, String> {
    if generation(doc) != g {
        return Err(format!("superseded: a theme or a choice changed since (generation {} -> {})", g, generation(doc)));
    }
    let rec = Recommendation::from_doc(doc).ok_or("superseded: no theme recommendation any more")?;
    if rec.theme != theme {
        return Err(format!("superseded: the theme is now {}", rec.theme));
    }
    // the theme actually selected, not only the recorded recommendation: a direct edit of `theme.name` that left the
    // metadata as it was must not receive the old theme's pack (exact theme id; no alias or fallback resolution)
    match selected_theme(doc) {
        Some(t) if t == theme => {}
        Some(t) => return Err(format!("superseded: the selected theme is now {t}")),
        None => return Err("superseded: no theme is selected".into()),
    }
    let mut done = Vec::new();
    for (k, name) in installed {
        let st = state(doc, *k);
        if st.is_custom() || st.legacy || st.theme.as_deref() != Some(theme) || current(doc, *k) != st.applied {
            continue;
        }
        set_current(doc, *k, Some(name));
        put_state(doc, *k, &SlotState { applied: Some(name.clone()), ..st });
        done.push((*k, name.clone()));
    }
    // the recommendation now names what is installed
    let mut rec = rec;
    for (k, name) in installed {
        match k {
            Provides::Icons => rec.icons = Some(name.clone()),
            Provides::Cursors => rec.cursors = Some(name.clone()),
        }
        if let Some(p) = rec.pack(*k).map(String::from) {
            rec.missing.retain(|m| *m != p);
        }
    }
    obj(doc, KEY).insert("recommended".into(), rec.to_json());
    Ok(done)
}

/// Is a theme of this name installed and usable for `k`?
pub fn is_installed(roots: &crate::icons::resolve::Roots, k: Provides, name: &str) -> bool {
    match k {
        Provides::Icons => crate::icons::resolve::list_themes(roots).iter().any(|t| t.id == name && t.kind.selectable()),
        Provides::Cursors => super::cursor::usable(roots, name),
    }
}

/// The recommendation for `theme_id`: its companion `[[recommend]]`, else the theme's own `[icons] theme` (installed, or
/// a catalogue pack of that name), else decebal's coverage table. Packs that are not installed are listed as missing.
pub fn recommendation(set: &catalog::Set, theme_id: &str, manifest_icons: Option<&str>, installed: &dyn Fn(Provides, &str) -> bool) -> Recommendation {
    let mut r = Recommendation { theme: theme_id.to_string(), ..Recommendation::default() };
    let take = |r: &mut Recommendation, k: Provides, id: &str| -> bool {
        let Some(e) = set.find(id).filter(|e| e.pack.id == id && e.pack.provides(k)) else { return false };
        match k {
            Provides::Icons => r.icons_pack = Some(e.pack.id.clone()),
            Provides::Cursors => r.cursors_pack = Some(e.pack.id.clone()),
        }
        if installed(k, &e.pack.install_name) {
            match k {
                Provides::Icons => r.icons = Some(e.pack.install_name.clone()),
                Provides::Cursors => r.cursors = Some(e.pack.install_name.clone()),
            }
        } else if !r.missing.contains(&e.pack.id) {
            r.missing.push(e.pack.id.clone());
        }
        true
    };
    if let Some(c) = set.recommendation(theme_id) {
        for (k, id) in [(Provides::Icons, &c.icons), (Provides::Cursors, &c.cursors)] {
            if let Some(id) = id {
                if take(&mut r, k, id) && k == Provides::Icons {
                    r.basis = Some("catalogue".into());
                }
            }
        }
    }
    if r.icons_pack.is_none() {
        if let Some(m) = manifest_icons.map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto")) {
            if installed(Provides::Icons, m) {
                r.icons = Some(m.to_string());
                r.basis = Some("manifest".into());
            } else if let Some(e) = set.entries().find(|e| e.pack.install_name == m && e.pack.provides(Provides::Icons)) {
                let id = e.pack.id.clone();
                take(&mut r, Provides::Icons, &id);
                r.basis = Some("manifest".into());
            } else {
                // named by the theme but neither installed nor in a catalogue: the coverage table decides below
                r.basis = Some(format!("manifest asks for `{m}`, which is not installed and has no pinned pack"));
            }
        }
    }
    if r.icons.is_none() && r.icons_pack.is_none() {
        if let Some(c) = super::coverage::find(theme_id) {
            if let Some(id) = &c.icons {
                take(&mut r, Provides::Icons, id);
            }
            if r.cursors_pack.is_none() {
                if let Some(id) = &c.cursors {
                    take(&mut r, Provides::Cursors, id);
                }
            }
            r.basis = Some(match r.basis.take() {
                Some(b) => format!("coverage:{} ({}; {b})", c.basis, c.reason),
                None => format!("coverage:{} ({})", c.basis, c.reason),
            });
        }
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
        Recommendation { theme: theme.into(), icons: icons.map(String::from), cursors: cursors.map(String::from), ..Default::default() }
    }

    #[test]
    fn icon_choices_and_theme_following_retire_the_legacy_app_only_override() {
        let mut doc = json!({"system":{"iconTheme":"OldPapirus","enabled":true},"icons":{"theme":"Adwaita"}});
        apply_theme(&mut doc, &rec("deepin", Some("bloom"), None));
        assert_eq!(current(&doc, Provides::Icons).as_deref(), Some("bloom"));
        assert!(doc["system"].get("iconTheme").is_none());
        assert_eq!(doc["system"]["enabled"], true);
        doc["system"]["iconTheme"] = json!("stale");
        choose(&mut doc, Provides::Icons, &Choice::Name("CustomIcons".into()));
        assert!(doc["system"].get("iconTheme").is_none());
        assert_eq!(owner(&doc, Provides::Icons), Owner::User);
        apply_theme(&mut doc, &rec("lingmo", Some("ocean"), None));
        assert_eq!(current(&doc, Provides::Icons).as_deref(), Some("CustomIcons"));
        choose(&mut doc, Provides::Icons, &Choice::Auto);
        assert_eq!(current(&doc, Provides::Icons), None);
        assert!(doc["system"].get("iconTheme").is_none());
        let mut no_recommendation = json!({"system":{"iconTheme":"stale"},"icons":{"theme":"Adwaita"}});
        choose(&mut no_recommendation, Provides::Icons, &Choice::Theme);
        assert!(no_recommendation["system"].get("iconTheme").is_none());
    }

    #[test]
    fn slots_follow_the_theme_by_default_over_a_baseline() {
        let mut d = json!({"icons": {"theme": "Papirus", "size_scale": 0.9}, "system": {"enabled": true, "cursorTheme": "Adwaita"}});
        let g0 = generation(&d);
        apply_theme(&mut d, &rec("deepin", Some("bloom"), Some("bloom")));
        assert_eq!((current(&d, Provides::Icons).as_deref(), current(&d, Provides::Cursors).as_deref()), (Some("bloom"), Some("bloom")));
        assert_eq!(d["icons"]["size_scale"], 0.9, "other icon keys stay");
        assert_eq!(owner(&d, Provides::Icons), Owner::Theme("deepin".into()));
        assert_eq!((d[KEY]["recommended"]["icons"].as_str(), d[KEY]["icons"]["mode"].as_str()), (Some("bloom"), Some("follow")));
        assert!(generation(&d) > g0, "every theme apply is a new generation");
        apply_theme(&mut d, &rec("lingmo", Some("ocean"), None));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("ocean"));
        assert_eq!(current(&d, Provides::Cursors).as_deref(), Some("Adwaita"), "no cursor match: the baseline is back");
        assert_eq!(state(&d, Provides::Icons).baseline.as_deref(), Some("Papirus"), "the baseline survives theme to theme");
        apply_theme(&mut d, &Recommendation { theme: "nord".into(), ..Default::default() });
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Papirus"));
        assert!(d[KEY].get("icons").is_none() && d[KEY].get("recommended").is_none(), "nothing left to record but the generation");
        assert_eq!(d["system"]["enabled"], true);
    }

    #[test]
    fn custom_choices_and_hand_edits_survive_themes_and_can_be_restored() {
        let mut d = json!({});
        choose(&mut d, Provides::Icons, &Choice::Name("Papirus".into()));
        choose(&mut d, Provides::Cursors, &Choice::Auto);
        let notes = apply_theme(&mut d, &rec("deepin", Some("bloom"), Some("bloom")));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Papirus"));
        assert_eq!(current(&d, Provides::Cursors), None, "an explicit auto is a custom choice too");
        assert!(notes.iter().any(|n| n.contains("kept (custom)") && n.contains("bloom")), "{notes:?}");
        assert_eq!((owner(&d, Provides::Icons), d[KEY]["icons"]["mode"].as_str()), (Owner::User, Some("custom")));
        // Follow theme again: the custom value is kept for restoration
        choose(&mut d, Provides::Icons, &Choice::Theme);
        assert_eq!((current(&d, Provides::Icons).as_deref(), owner(&d, Provides::Icons)), (Some("bloom"), Owner::Theme("deepin".into())));
        assert_eq!(state(&d, Provides::Icons).restore.as_deref(), Some("Papirus"));
        apply_theme(&mut d, &rec("lingmo", Some("ocean"), None));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("ocean"), "following: the next theme applies its icons");
        assert_eq!(state(&d, Provides::Icons).restore.as_deref(), Some("Papirus"), "and the restore value survives");
        choose(&mut d, Provides::Icons, &Choice::Restore);
        assert_eq!((current(&d, Provides::Icons).as_deref(), owner(&d, Provides::Icons)), (Some("Papirus"), Owner::User));
        // hand edit after a theme set the value: custom from then on
        let mut d = json!({});
        apply_theme(&mut d, &rec("deepin", Some("bloom"), None));
        d["icons"]["theme"] = json!("Tela");
        assert_eq!(owner(&d, Provides::Icons), Owner::User);
        apply_theme(&mut d, &rec("lingmo", Some("ocean"), None));
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Tela"));
        assert!(state(&d, Provides::Icons).is_custom(), "recorded as custom");
        choose(&mut d, Provides::Icons, &Choice::Theme);
        assert_eq!((current(&d, Provides::Icons).as_deref(), owner(&d, Provides::Icons)), (Some("ocean"), Owner::Theme("lingmo".into())));
        assert_eq!(state(&d, Provides::Icons).baseline.as_deref(), Some("Tela"));
    }

    #[test]
    fn the_generation_is_external_input_and_never_panics() {
        let mut d = json!({KEY: {"generation": u64::MAX}});
        choose(&mut d, Provides::Icons, &Choice::Auto);
        assert_eq!(generation(&d), 1, "the maximum wraps to 1");
        for bad in [json!("7"), json!(-3), json!(1.5), json!(null), json!({"x": 1})] {
            let mut d = json!({KEY: {"generation": bad}});
            assert_eq!(generation(&d), 0);
            apply_theme(&mut d, &rec("deepin", Some("bloom"), None));
            assert_eq!(generation(&d), 1);
        }
    }

    #[test]
    fn old_manual_choices_stay_custom_until_the_user_follows_the_theme() {
        let mut d = json!({"theme": {"name": "deepin"}, "icons": {"theme": "Papirus"}, KEY: {"icons": {"user": true}, "cursors": {"user": true}}});
        let notes = apply_theme(&mut d, &rec("deepin", Some("bloom"), Some("bloom")));
        assert_eq!((current(&d, Provides::Icons).as_deref(), current(&d, Provides::Cursors)), (Some("Papirus"), None), "a manual choice is never overridden");
        assert_eq!(owner(&d, Provides::Icons), Owner::User);
        assert!(notes.iter().any(|n| n.contains("kept (custom)") && n.contains("bloom")), "{notes:?}");
        // and no late background install changes it either
        let g = generation(&d);
        assert!(complete(&mut d, g, "deepin", &[(Provides::Icons, "bloom".into())]).unwrap().is_empty());
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Papirus"));
        choose(&mut d, Provides::Icons, &Choice::Theme);
        assert_eq!((current(&d, Provides::Icons).as_deref(), state(&d, Provides::Icons).restore.as_deref()), (Some("bloom"), Some("Papirus")));
    }

    #[test]
    fn a_missing_pack_keeps_the_shown_value_and_completes_only_for_its_generation() {
        let mut d = json!({"theme": {"name": "deepin"}, "icons": {"theme": "Adwaita"}});
        let r = Recommendation { theme: "deepin".into(), icons_pack: Some("deepin-bloom".into()), missing: vec!["deepin-bloom".into()], ..Default::default() };
        apply_theme(&mut d, &r);
        assert_eq!(current(&d, Provides::Icons).as_deref(), Some("Adwaita"), "no flash to something else meanwhile");
        assert_eq!(state(&d, Provides::Icons).theme.as_deref(), Some("deepin"), "the slot waits for deepin's pack");
        // a later theme with no match for the slot drops the wait and puts the baseline back
        let mut later = d.clone();
        apply_theme(&mut later, &rec("nord", None, None));
        assert_eq!((state(&later, Provides::Icons), current(&later, Provides::Icons).as_deref()), (SlotState::default(), Some("Adwaita")));
        let g = generation(&d);
        let mut stale = d.clone();
        assert!(complete(&mut stale, g - 1, "deepin", &[(Provides::Icons, "bloom".into())]).unwrap_err().contains("superseded"));
        assert!(complete(&mut stale, g, "lingmo", &[(Provides::Icons, "bloom".into())]).unwrap_err().contains("the theme is now deepin"));
        let mut edited = d.clone();
        edited["theme"]["name"] = json!("nord");
        assert!(complete(&mut edited, g, "deepin", &[(Provides::Icons, "bloom".into())]).unwrap_err().contains("selected theme is now nord"));
        assert_eq!(current(&edited, Provides::Icons).as_deref(), Some("Adwaita"), "a direct theme edit gets nothing");
        let mut unselected = d.clone();
        unselected.as_object_mut().unwrap().remove("theme");
        assert!(complete(&mut unselected, g, "deepin", &[(Provides::Icons, "bloom".into())]).unwrap_err().contains("no theme is selected"));
        assert_eq!(complete(&mut d, g, "deepin", &[(Provides::Icons, "bloom".into())]).unwrap(), vec![(Provides::Icons, "bloom".to_string())]);
        assert_eq!((current(&d, Provides::Icons).as_deref(), owner(&d, Provides::Icons)), (Some("bloom"), Owner::Theme("deepin".into())));
        assert!(Recommendation::from_doc(&d).unwrap().missing.is_empty());
        // downloads set to ask: offered, not promised
        let mut d = json!({});
        set_downloads(&mut d, false);
        assert!(!downloads_auto(&d));
        assert!(apply_theme(&mut d, &r).iter().any(|n| n.contains("downloads are set to ask")));
        set_downloads(&mut d, true);
        assert!(downloads_auto(&d) && d[KEY].get("downloads").is_none());
    }

    #[test]
    fn recommendations_count_only_installed_packs_and_fall_back_to_the_manifest() {
        let mut set = catalog::Set::default();
        set.push(catalog::parse(catalog::tests::GOOD, "t").unwrap());
        let none = |_: Provides, _: &str| false;
        // the catalogue's pack wins over the manifest's name, also while it still has to be downloaded
        let r = recommendation(&set, "deepin", Some("Adwaita"), &none);
        assert_eq!((r.icons.as_deref(), r.cursors.as_deref(), r.missing.as_slice()), (None, None, &["deepin-bloom".to_string()][..]));
        assert_eq!((r.icons_pack.as_deref(), r.cursors_pack.as_deref(), r.basis.as_deref()), (Some("deepin-bloom"), Some("deepin-bloom"), Some("catalogue")));
        // a manifest name that is installed is used as it is; one that is neither installed nor pinned is left to coverage
        let r = recommendation(&set, "sunset", Some("Adwaita"), &|_, n| n == "Adwaita");
        assert_eq!((r.icons.as_deref(), r.basis.as_deref()), (Some("Adwaita"), Some("manifest")));
        let r = recommendation(&set, "sunset", Some("Yaru-purple-dark"), &none);
        assert!(r.icons.is_none() && r.basis.as_deref().is_some_and(|b| b.contains("not installed")), "{r:?}");
        let all = |_: Provides, n: &str| n == "bloom";
        let r = recommendation(&set, "deepin", Some("Adwaita"), &all);
        assert_eq!((r.icons.as_deref(), r.cursors.as_deref()), (Some("bloom"), Some("bloom")));
        let r = recommendation(&set, "nord", Some("auto"), &all);
        assert!(r.is_empty());
        let mut d = json!({});
        let notes = apply_theme(&mut d, &recommendation(&set, "deepin", None, &none));
        assert!(notes.is_empty(), "the background job reports the download: {notes:?}");
        assert_eq!(d[KEY]["recommended"]["missing"][0], "deepin-bloom");
        assert!(d.get("icons").is_none(), "nothing installed: nothing changes yet");
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
        assert!(state(&d, Provides::Cursors).is_custom());
        assert_eq!(d["theme"]["name"], "nord");
        // handing a slot back to the theme is one transaction too
        choose_in_file(&f, Provides::Icons, &Choice::Theme).unwrap();
        let d = crate::settings::io::json_load(&f);
        assert_eq!(state(&d, Provides::Icons), SlotState { restore: Some("bloom".into()), ..SlotState::default() }, "no recommendation for icons: the slot follows, the custom value kept");
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
        assert_eq!((d[KEY]["icons"]["mode"].as_str(), d["system"]["cursorTheme"].as_str()), (Some("custom"), Some("DMZ-White")));
        assert_eq!((d["theme"]["name"].as_str(), d["wallpaper"]["x"].as_i64(), d["icons"]["fallback"].as_str()), (Some("nord"), Some(1), Some("none")));
    }
}
