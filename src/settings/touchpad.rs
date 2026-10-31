//! Touchpad, mouse and key repeat settings of rdwm's Wayland session (`[input]` in config.toml, applied through libinput on
//! reload), shown in Keyboard & Input. The X11 tools of the pane above (`xinput`) cannot reach a Wayland
//! compositor, so this section edits the compositor's own config instead. Controls exist only for keys the
//! schema lists, so an older rdwm without them simply shows nothing here.

use super::model::Model;
use super::system::run::wayland_session;
use super::ui::{Item, Row};

const SECTION: &str = "input";

pub fn items(m: &Model) -> Vec<Item> {
    items_for(m, wayland_session())
}

fn items_for(m: &Model, wayland: bool) -> Vec<Item> {
    if !wayland || !m.schema.has(SECTION, "natural_scroll") {
        return Vec::new();
    }
    let mut rows: Vec<Row> = Vec::new();
    rows.extend(m.r_toggle(SECTION, "natural_scroll", "Natural scrolling", "Content follows your fingers on the touchpad and mouse wheel"));
    rows.extend(m.r_toggle(SECTION, "tap_to_click", "Tap to click", "Tap the touchpad instead of pressing it"));
    rows.extend(m.r_toggle(SECTION, "left_handed", "Left-handed buttons", "Swap the left and right buttons"));
    rows.extend(m.r_slider(SECTION, "pointer_accel", "Pointer speed", "-1 is slowest, 1 is fastest, 0 is the device default", 0.05, "", None));
    rows.extend(m.r_slider(SECTION, "repeat_delay", "Key repeat delay", "How long to hold a key before it repeats", 10.0, " ms", Some(1000.0)));
    rows.extend(m.r_slider(SECTION, "repeat_rate", "Key repeat rate", "Repeats per second; 0 turns key repeat off", 1.0, "/s", Some(100.0)));
    if rows.is_empty() {
        return Vec::new();
    }
    vec![
        Item::Header("Touchpad, mouse & keyboard (rdwm)".into()),
        Item::Note { text: "Native Wayland apps. Saved to the [input] section of rdwm's config and applied to every device straight away. Only the real Wayland session (started from the login screen) reads it; a nested rdwm window ignores it.".into(), warn: false },
        Item::Card(rows),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::{tests_support::model_with, CtlVal};
    use crate::settings::ui::Kind;

    fn kind<'a>(items: &'a [Item], id: &str) -> Option<&'a Kind> {
        items.iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().flat_map(|r| r.ctls.iter()).find(|c| c.id == id).map(|c| &c.kind)
    }

    #[test]
    fn natural_scrolling_toggle_writes_the_input_section_and_keeps_the_rest() {
        let (mut m, cfg) = model_with("# mine\n[general]\nborder_px = 2\n");
        let its = items_for(&m, true);
        assert!(matches!(kind(&its, "r:input:natural_scroll"), Some(Kind::Toggle(false))));
        assert!(kind(&its, "r:input:tap_to_click").is_some() && kind(&its, "r:input:pointer_accel").is_some());
        m.apply("r:input:natural_scroll", CtlVal::Bool(true));
        let t = std::fs::read_to_string(&cfg).unwrap();
        assert!(t.starts_with("# mine\n[general]\nborder_px = 2\n"), "{t}");
        assert!(t.contains("[input]") && t.contains("natural_scroll = true"), "{t}");
        assert!(matches!(kind(&items_for(&m, true), "r:input:natural_scroll"), Some(Kind::Toggle(true))));
    }

    #[test]
    fn an_rdwm_without_the_input_keys_shows_nothing() {
        let (mut m, _) = model_with("[general]\n");
        m.schema.keys.retain(|k| k.section != SECTION);
        assert!(items_for(&m, true).is_empty());
        let (m, _) = model_with("[general]\n");
        assert!(items_for(&m, false).is_empty(), "X11 sessions use the xinput section");
    }
}
