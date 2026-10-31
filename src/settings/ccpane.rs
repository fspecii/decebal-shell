//! The Control Center section of the Desktop & Dock pane: on/off, the corner a summoned panel opens in, its look and which tile
//! groups it shows. Self-contained on purpose (the pane only calls [`items`] and [`apply`]); everything is stored in `decebal.json`
//! under `control_center` (see docs/theming.md) and every running program follows the file within half a second.

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Opt, Row};
use crate::controlcenter::settings::{Corner, Settings, Style, Tiles, POSITION_NAMES, STYLE_NAMES};

const TILE_LABELS: [(&str, &str, &str); 7] = [
    ("connectivity", "Wi-Fi, Bluetooth, Network", "The connectivity tile"),
    ("focus", "Focus, Night Shift, Dark Mode", "The tiles on the right of the first row"),
    ("display", "Display", "Brightness slider"),
    ("sound", "Sound", "Volume slider"),
    ("media", "Now Playing", "Media controls"),
    ("toggles", "Record, Idle, Game Mode, DND", "The decutil quick toggles"),
    ("actions", "Stay Awake, Screenshot, Notifications, Settings", "The row of actions at the bottom"),
];

fn title(word: &str) -> String {
    // acronyms keep their capitals
    if word == "gnome" {
        return "GNOME".into();
    }
    if word == "ukui" {
        return "UKUI".into();
    }
    let mut c = word.split('-').map(|w| {
        let mut ch = w.chars();
        ch.next().map(|f| f.to_uppercase().collect::<String>() + ch.as_str()).unwrap_or_default()
    });
    c.next().into_iter().chain(c).collect::<Vec<_>>().join(" ")
}

fn choices(names: &[&str]) -> Vec<Opt> {
    names.iter().map(|n| (title(n), n.to_string())).collect()
}

fn current(m: &Model) -> Settings {
    m.shell.get("control_center").map(Settings::from_json).unwrap_or_default()
}

/// The header, the settings card and a note, ready to append to a pane.
pub fn items(m: &Model) -> Vec<Item> {
    let s = current(m);
    let pos = POSITION_NAMES.iter().position(|n| *n == s.position.as_str());
    let style = STYLE_NAMES.iter().position(|n| *n == s.style.as_str());
    let mut rows = vec![
        Row::new("Show the Control Center").detail("Its button in the bar or taskbar, `decutil control-center` and the hot-corner action").ctl(Ctl::new("cc:enabled", Kind::Toggle(s.enabled))),
        Row::new("Opens in").detail("Where a key or hot corner opens it. Auto: beside the bar's own button").ctl(Ctl::new("cc:position", Kind::Popup(choices(&POSITION_NAMES), pos))),
        Row::new("Look")
            .detail("Auto follows the desktop theme. GNOME draws GNOME Shell's quick settings (pill toggles, slider rows) with Adwaita symbolic icons when installed. Dux uses package Material icons when available, otherwise standard symbols.")
            .ctl(Ctl::new("cc:style", Kind::Popup(choices(&STYLE_NAMES), style))),
    ];
    for (name, label, detail) in TILE_LABELS {
        rows.push(Row::new(label).detail(detail).ctl(Ctl::new(format!("cc:tile:{name}"), Kind::Toggle(s.tiles.get(name).unwrap_or(true)))));
    }
    vec![
        Item::Header("Control Center".into()),
        Item::Card(rows),
        Item::Note { text: "Bind a key to `decutil control-center toggle` to open it in any theme, with or without a bar. A theme can set these too ([control_center] in theme.toml); applying a theme overwrites them.".into(), warn: false },
    ]
}

/// Handles `cc:*` controls: reads the object, changes one key, writes it back whole.
pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    let mut s = current(m);
    let said = match (parts, v) {
        (["cc", "enabled"], CtlVal::Bool(b)) => {
            s.enabled = b;
            if b {
                "Control Center on"
            } else {
                "Control Center off"
            }
        }
        (["cc", "position"], CtlVal::Str(p)) => match Corner::parse(&p) {
            Some(c) => {
                s.position = c;
                "Control Center position saved"
            }
            None => return,
        },
        (["cc", "style"], CtlVal::Str(p)) => match Style::parse(&p) {
            Some(st) => {
                s.style = st;
                "Control Center look saved"
            }
            None => return,
        },
        (["cc", "tile", name], CtlVal::Bool(on)) => {
            let mut t: Tiles = s.tiles;
            t.set(name, on);
            if t.names().is_empty() {
                m.say("Keep at least one tile group", true);
                m.dirty = true;
                return;
            }
            s.tiles = t;
            "Control Center tiles saved"
        }
        _ => return,
    };
    // Keys of the object this version does not know (a newer decebal's, a theme's) are kept.
    let mut obj = m.shell.get("control_center").and_then(|v| v.as_object()).cloned().unwrap_or_default();
    if let serde_json::Value::Object(known) = s.to_json() {
        obj.extend(known);
    }
    m.edit_json_said("shell", "control_center", Some(serde_json::Value::Object(obj)), Some(said.into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_read_well() {
        assert_eq!(title("top-right"), "Top Right");
        assert_eq!(title("auto"), "Auto");
        assert_eq!(title("gnome"), "GNOME");
        assert_eq!(choices(&crate::controlcenter::settings::STYLE_NAMES).last().unwrap().0, "UKUI");
        assert!(choices(&crate::controlcenter::settings::STYLE_NAMES).iter().any(|c| c.0 == "GNOME"));
        assert_eq!(choices(&["fluent"]), vec![("Fluent".to_string(), "fluent".to_string())]);
    }

    #[test]
    fn every_tile_group_has_a_row() {
        let names: Vec<&str> = TILE_LABELS.iter().map(|t| t.0).collect();
        assert_eq!(names, crate::controlcenter::settings::TILE_NAMES);
    }
}
