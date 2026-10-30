//! Audited structural titlebar designs implemented by rdwm-deco. Not desktop palettes.
//! The three Xfce entries are rdwm's native approximations inspired by xfwm4 themes, not exact ports (docs/decorations.md).
use super::{
    schema::Schema,
    tomlcfg::{Cfg, Val},
};

pub const DESIGNS: &[(&str, &str)] = &[
    ("GNOME Adwaita", "adwaita"),
    ("GNOME Adwaita · light", "adwaita-light"),
    ("KDE Breeze", "breeze"),
    ("Arc", "arc"),
    ("Material / Materia", "materia"),
    ("macOS · traffic lights left", "macos"),
    ("macOS · light", "macos-light"),
    ("Windows · caption cells", "windows"),
    ("Windows · light", "windows-light"),
    ("Nord · traffic lights right", "nord"),
    ("Gruvbox · square buttons", "gruvbox"),
    ("Dracula · traffic lights left", "dracula"),
    ("Minimal · small dots", "minimal"),
    ("Classic · flat strip", "classic"),
    ("Xfce Default · raised controls", "xfce-default"),
    ("Xfce Daloa · blue caption cells", "xfce-daloa"),
    ("Xfce Kokodi · rounded blue", "xfce-kokodi"),
];

/// Respect an older live rdwm's actual supported set. The updated frozen schema is the offline fallback.
pub fn choices(schema: &Schema) -> Vec<(&'static str, &'static str)> {
    let supported = schema.get("decoration", "style");
    DESIGNS.iter().copied().filter(|(_, id)| supported.is_some_and(|m| m.choices.iter().any(|s| s == id))).collect()
}

/// Generated directly by the native rdwm-deco helper, not guessed generic macOS metrics.
pub fn default_value(style: &str, key: &str) -> Option<Val> {
    static CATALOGUE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let rows = CATALOGUE.get_or_init(|| serde_json::from_str(include_str!("decoration-catalogue.json")).expect("generated native decoration catalogue"));
    let value = rows.as_array()?.iter().find(|row| row["id"] == style)?.get("defaults")?.get(key)?;
    match value {
        serde_json::Value::String(s) => Some(Val::Str(s.clone())),
        serde_json::Value::Number(n) => n.as_i64().map(Val::Int),
        _ => None,
    }
}

/// Choosing structure restores native geometry instead of carrying macOS metrics into every style.
/// Colors, fonts, DPI and an explicitly chosen zoom behavior remain independent.
pub fn choose(c: &mut Cfg, id: &str) -> Result<(), String> {
    if !DESIGNS.iter().any(|(_, s)| *s == id) {
        return Err("Unsupported button design".into());
    }
    c.set("decoration", "style", &Val::Str(id.into()))?;
    c.set("decoration", "enabled", &Val::Bool(true))?;
    c.set("decoration", "buttons", &Val::Bool(true))?;
    for key in ["height", "radius", "title_align", "border_px", "button_size", "button_gap", "button_margin"] {
        c.remove("decoration", key);
    }
    if c.get("decoration", "zoom_button").is_none() {
        c.set("decoration", "zoom_button", &Val::Str("maximize".into()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_matches_supported_schema_and_never_offers_no_buttons() {
        let schema = Schema::snapshot();
        let options = choices(&schema);
        assert_eq!(options.len(), 17);
        for (_, id) in &options {
            assert!(default_value(id, "height").is_some());
        }
        assert!(!options.iter().any(|(_, s)| *s == "borderonly"));
        for id in ["adwaita", "breeze", "arc", "materia", "xfce-default", "xfce-daloa", "xfce-kokodi"] {
            assert!(options.iter().any(|(_, s)| *s == id));
        }
        // A live rdwm from before the Xfce styles (the previous frozen set) must not be offered IDs it cannot parse.
        let mut previous = schema.clone();
        previous.keys.iter_mut().find(|m| m.section == "decoration" && m.key == "style").unwrap().choices =
            ["macos", "macos-light", "classic", "windows", "windows-light", "adwaita", "adwaita-light", "breeze", "arc", "materia", "nord", "gruvbox", "dracula", "minimal", "borderonly"]
                .map(String::from)
                .to_vec();
        let offered = choices(&previous);
        assert_eq!(offered.len(), 14);
        assert!(!offered.iter().any(|(_, s)| s.starts_with("xfce-")), "unsupported IDs are never offered to an older rdwm");
        let mut older = schema;
        older.keys.iter_mut().find(|m| m.section == "decoration" && m.key == "style").unwrap().choices = vec!["macos".into(), "classic".into()];
        assert_eq!(choices(&older), vec![("macOS · traffic lights left", "macos"), ("Classic · flat strip", "classic")]);
    }
    #[test]
    fn xfce_designs_use_the_native_renderers_own_metrics() {
        // Values exported by the actual rdwm-deco renderer (tools/build-decoration-preview.py metadata), not guessed defaults.
        let expected = [("xfce-default", 29, 3, 19, 2, 5, "center"), ("xfce-daloa", 24, 0, 24, 0, 0, "left"), ("xfce-kokodi", 30, 8, 20, 3, 6, "center")];
        for (id, height, radius, size, gap, margin, align) in expected {
            assert_eq!(default_value(id, "height"), Some(Val::Int(height)), "{id}");
            assert_eq!(default_value(id, "radius"), Some(Val::Int(radius)), "{id}");
            assert_eq!(default_value(id, "button_size"), Some(Val::Int(size)), "{id}");
            assert_eq!(default_value(id, "button_gap"), Some(Val::Int(gap)), "{id}");
            assert_eq!(default_value(id, "button_margin"), Some(Val::Int(margin)), "{id}");
            assert_eq!(default_value(id, "title_align"), Some(Val::Str(align.into())), "{id}");
            assert!(DESIGNS.iter().any(|(label, s)| *s == id && label.starts_with("Xfce")));
        }
    }

    #[test]
    fn every_design_has_a_complete_catalogue_row_and_a_schema_choice() {
        let schema = Schema::snapshot();
        let supported = &schema.get("decoration", "style").unwrap().choices;
        for (label, id) in DESIGNS {
            assert!(supported.iter().any(|c| c == id), "{label} missing from the frozen schema");
            for key in ["height", "radius", "button_size", "button_gap", "button_margin", "border_px", "title_align"] {
                assert!(default_value(id, key).is_some(), "{id} lacks native default {key}");
            }
        }
        let mut ids: Vec<_> = DESIGNS.iter().map(|(_, s)| *s).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), DESIGNS.len(), "duplicate design id");
    }

    #[test]
    fn every_design_restores_geometry_and_all_controls_without_recoloring() {
        for (_, id) in DESIGNS {
            let mut cfg = Cfg::parse(
                "[decoration]\nstyle=\"macos\"\nenabled=false\nbuttons=false\nheight=20\nradius=2\nbutton_size=8\nbutton_gap=2\nbutton_margin=5\nfocused_bg=\"#123456\"\nfont=\"Inter\"\nscale=2.0\n",
            )
            .unwrap();
            choose(&mut cfg, id).unwrap();
            assert_eq!(cfg.get("decoration", "style"), Some(Val::Str((*id).into())));
            for key in ["enabled", "buttons"] {
                assert_eq!(cfg.get("decoration", key), Some(Val::Bool(true)));
            }
            for key in ["height", "radius", "button_size", "button_gap", "button_margin"] {
                assert!(cfg.get("decoration", key).is_none());
            }
            assert_eq!(cfg.get("decoration", "zoom_button"), Some(Val::Str("maximize".into())));
            assert_eq!(cfg.get("decoration", "focused_bg"), Some(Val::Str("#123456".into())));
            assert_eq!(cfg.get("decoration", "scale"), Some(Val::Float(2.0)));
        }
        let mut cfg = Cfg::parse("[decoration]\nzoom_button=\"fullscreen\"\n").unwrap();
        choose(&mut cfg, "breeze").unwrap();
        assert_eq!(cfg.get("decoration", "zoom_button"), Some(Val::Str("fullscreen".into())), "explicit behavior survives a structural edit");
    }
}
