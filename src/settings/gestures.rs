//! Trackpad swipes in Keyboard & Input: rdwm's `[gestures]` (enabled, threshold, natural, bindings).
//!
//! Every control exists only when the schema actually loaded (the installed `rdwmctl schema --offline`, else the
//! bundled snapshot) lists its key; otherwise the section says the controls need an updated rdwm and leaves the
//! file alone. Changes go through the normal TOML job (schema range check, `rdwmctl check-config`, persistence
//! worker); a patch touches only the changed key, so `bindings` and keys this version does not know stay as written.
//!
//! The bindings are listed read only: rdwm writes them as an inline array of tables (`bindings = [ {...} ]`), which
//! the table editor cannot edit in place, and replacing it would rewrite the user's list. A schema entry does not
//! prove that the hardware, driver and backend deliver swipe events (see rdwm's docs/gestures.md).

use super::model::Model;
use super::ui::{Item, Row};

const SECTION: &str = "gestures";

/// One `{ fingers, direction, action }` entry of `gestures.bindings`, as written in config.toml.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub fingers: Option<i64>,
    pub direction: String,
    pub action: String,
}

/// The bindings in the shown document: `None` when the key is absent (rdwm's defaults apply), else the list
/// (`[]` disables every swipe). Reads inline arrays of tables and `[[gestures.bindings]]` alike.
pub fn bindings(m: &Model) -> Option<Vec<Binding>> {
    let doc: toml_edit::DocumentMut = m.store.cfg.text().parse().ok()?;
    let item = doc.get(SECTION)?.as_table_like()?.get("bindings")?;
    let of = |t: &dyn toml_edit::TableLike| Binding {
        fingers: t.get("fingers").and_then(|v| v.as_integer()),
        direction: t.get("direction").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        action: t.get("action").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
    };
    if let Some(a) = item.as_array() {
        return Some(a.iter().filter_map(|v| v.as_inline_table()).map(|t| of(t)).collect());
    }
    if let Some(a) = item.as_array_of_tables() {
        return Some(a.iter().map(|t| of(t)).collect());
    }
    Some(Vec::new())
}

fn binding_row(b: &Binding) -> Row {
    let fingers = b.fingers.map_or("?".to_string(), |n| n.to_string());
    Row::new(format!("{fingers} fingers, swipe {}", b.direction)).detail(b.action.clone())
}

/// The keys of `[gestures]` this schema lists.
fn known(m: &Model) -> Vec<&'static str> {
    ["enabled", "threshold", "natural", "bindings"].into_iter().filter(|k| m.schema.has(SECTION, k)).collect()
}

pub fn items(m: &Model) -> Vec<Item> {
    let mut out = vec![Item::Header("Trackpad swipes".into())];
    let have = known(m);
    let source = if m.schema.live { "the installed rdwmctl" } else { "the bundled rdwm schema (rdwmctl not found)" };
    if !have.contains(&"enabled") && !have.contains(&"threshold") && !have.contains(&"natural") {
        out.push(Item::Note {
            text: format!(
                "Three- and four-finger swipes need an updated rdwm: {source} does not list [gestures], so these controls are not shown. A [gestures] section already in config.toml is kept as it is; edit it in Config & Presets."
            ),
            warn: false,
        });
        return out;
    }
    // only keys this schema lists are offered (asking for others would also count them as missing)
    let mut rows = Vec::new();
    if have.contains(&"enabled") {
        rows.extend(m.r_toggle(SECTION, "enabled", "Swipe gestures", "Three- and four-finger trackpad swipes run window-manager actions"));
    }
    if let Some(meta) = m.schema.get(SECTION, "threshold") {
        let range = match (meta.min, meta.max) {
            (Some(a), Some(b)) => format!(" ({a:.0}–{b:.0})"),
            _ => String::new(),
        };
        let detail = format!("How far fingers travel before a swipe counts, in rdwm's logical touchpad delta units{range}, not millimetres; how far that is depends on the device and driver");
        rows.extend(m.r_slider(SECTION, "threshold", "Required swipe travel", &detail, 10.0, " units", None));
    }
    if have.contains(&"natural") {
        rows.extend(m.r_toggle(SECTION, "natural", "Reverse swipe direction", "Swaps left/right and up/down for swipes only; separate from natural scrolling of the pointer"));
    }
    out.push(Item::Card(rows));

    out.push(Item::Header("Swipe bindings".into()));
    match bindings(m) {
        Some(list) if list.is_empty() => out.push(Item::Card(vec![Row::new("No swipe bindings").detail("bindings = [] in config.toml: every swipe is off")])),
        Some(list) => out.push(Item::Card(list.iter().map(binding_row).collect())),
        None => out.push(Item::Card(vec![Row::new("rdwm's default bindings").detail("No bindings in config.toml, so the window manager's built-in swipes apply")])),
    }
    let editable = if have.contains(&"bindings") { "" } else { " This rdwm's schema does not describe the bindings either." };
    out.push(Item::Note {
        text: format!(
            "Bindings are shown, not edited, here: they are an inline list in config.toml, kept exactly as written when you change the settings above. Change them in Config & Presets (same actions as key bindings; providing a list replaces all defaults).{editable} Whether swipes arrive depends on the touchpad, its driver and the session (X11 or Wayland), not on these settings."
        ),
        warn: false,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::{tests_support::model_with, CtlVal};
    use crate::settings::schema::KeyMeta;
    use crate::settings::ui::Kind;
    use serde_json::json;

    /// The `[gestures]` keys as rdwm's parser accepts them (docs/gestures.md); the published schema entry is the
    /// WM owner's, this only stands in for it.
    pub fn with_gesture_schema(m: &mut Model) {
        let k = |key: &str, ty: &str, default: serde_json::Value, min: Option<f64>, max: Option<f64>, desc: &str| KeyMeta {
            section: SECTION.into(),
            key: key.into(),
            ty: ty.into(),
            default,
            min,
            max,
            choices: Vec::new(),
            desc: desc.into(),
        };
        m.schema.keys.push(k("enabled", "bool", json!(true), None, None, "Native trackpad swipes"));
        m.schema.keys.push(k("threshold", "float", json!(120.0), Some(40.0), Some(2000.0), "Swipe distance, logical delta units"));
        m.schema.keys.push(k("natural", "bool", json!(false), None, None, "Reverse swipe direction"));
    }

    fn rows(items: &[Item]) -> Vec<&Row> {
        items.iter().filter_map(|i| if let Item::Card(r) = i { Some(r.iter().collect::<Vec<_>>()) } else { None }).flatten().collect()
    }

    fn ctl<'a>(items: &'a [Item], id: &str) -> Option<&'a Kind> {
        rows(items).into_iter().flat_map(|r| r.ctls.iter()).find(|c| c.id == id).map(|c| &c.kind)
    }

    const INLINE: &str = "[general]\nborder_px = 2\n\n[gestures]\nenabled = true\nthreshold = 120.0\nnatural = false\nfuture_gesture_key = \"kept\"\n# my swipes\nbindings = [\n  { fingers = 3, direction = \"left\", action = \"viewnext\" },\n  { fingers = 4, direction = \"up\", action = \"focus_up\" },\n]\n";

    #[test]
    fn without_gesture_keys_in_the_schema_the_section_explains_and_offers_nothing() {
        let (m, _) = model_with(INLINE);
        assert!(!m.schema.has(SECTION, "enabled"), "the bundled snapshot predates [gestures]");
        let its = items(&m);
        assert!(ctl(&its, "r:gestures:enabled").is_none() && ctl(&its, "r:gestures:threshold").is_none());
        assert!(its.iter().any(|i| matches!(i, Item::Note { text, .. } if text.contains("need an updated rdwm") && text.contains("does not list [gestures]"))));
    }

    #[test]
    fn controls_follow_the_schema_and_show_the_file() {
        let (mut m, _) = model_with(INLINE);
        with_gesture_schema(&mut m);
        let its = items(&m);
        assert_eq!(ctl(&its, "r:gestures:enabled"), Some(&Kind::Toggle(true)));
        match ctl(&its, "r:gestures:threshold") {
            Some(Kind::Slider { v, min, max, unit, .. }) => assert_eq!((*v, *min, *max, unit.as_str()), (120.0, 40.0, 2000.0, " units")),
            k => panic!("{k:?}"),
        }
        assert_eq!(ctl(&its, "r:gestures:natural"), Some(&Kind::Toggle(false)));
        let shown: Vec<String> = rows(&its).iter().map(|r| format!("{} | {}", r.label, r.detail)).collect();
        assert!(shown.contains(&"3 fingers, swipe left | viewnext".to_string()) && shown.contains(&"4 fingers, swipe up | focus_up".to_string()), "{shown:?}");
        let detail = rows(&its).iter().find(|r| r.label == "Required swipe travel").unwrap().detail.clone();
        assert!(detail.contains("not millimetres") && detail.contains("40–2000"), "{detail}");
    }

    #[test]
    fn changing_a_setting_keeps_the_bindings_comments_and_unknown_keys_byte_for_byte() {
        let (mut m, cfg) = model_with(INLINE);
        with_gesture_schema(&mut m);
        m.apply("r:gestures:enabled", CtlVal::Bool(false));
        m.apply("r:gestures:threshold", CtlVal::Num(300.0));
        m.apply("r:gestures:natural", CtlVal::Bool(true));
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert_eq!(text, INLINE.replace("enabled = true", "enabled = false").replace("threshold = 120.0", "threshold = 300.0").replace("natural = false", "natural = true"));
        assert!(!m.notice.as_ref().unwrap().error, "{:?}", m.notice);
        // out of range: refused before anything is written
        m.apply("r:gestures:threshold", CtlVal::Num(5.0));
        assert!(m.notice.as_ref().unwrap().error);
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("threshold = 300.0"));
        // reset removes only that key
        m.apply("r:gestures:natural", CtlVal::Reset);
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(!text.contains("natural") && text.contains("future_gesture_key = \"kept\"") && text.contains("# my swipes"), "{text}");
    }

    #[test]
    fn absent_and_empty_bindings_say_what_applies() {
        let (mut m, _) = model_with("[gestures]\nenabled = true\n");
        with_gesture_schema(&mut m);
        assert_eq!(bindings(&m), None);
        assert!(rows(&items(&m)).iter().any(|r| r.label == "rdwm's default bindings"));
        let (mut m, _) = model_with("[gestures]\nbindings = []\n");
        with_gesture_schema(&mut m);
        assert_eq!(bindings(&m), Some(Vec::new()));
        assert!(rows(&items(&m)).iter().any(|r| r.label == "No swipe bindings"));
        let (m, _) = model_with("[[gestures.bindings]]\nfingers = 3\ndirection = \"down\"\naction = \"focus_down\"\n");
        assert_eq!(bindings(&m).unwrap()[0], Binding { fingers: Some(3), direction: "down".into(), action: "focus_down".into() });
    }
}
