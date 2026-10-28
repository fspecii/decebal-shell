//! The "Fonts" section of the Theme pane: a preset picker, one family picker per role, a live preview and a reset button.
//!
//! Everything here is a pure function of the stored [`Selection`] and the resolved theme, so it is tested without a
//! window. Choosing writes the selection like every other Theme control (`themepane::commit`); the programs reload
//! the theme within half a second and redraw in the new fonts, the preview line included.

use super::model::CtlVal;
use super::ui::{Ctl, FontSample, Item, Kind, Row};
use crate::text::{Role, Weight};
use crate::theme::fontdb::{self, norm};
use crate::theme::fonts::{self, Cmd, FontChoice, PRESETS, SAMPLE};
use crate::theme::model::{Selection, Theme};
use std::time::Duration;

/// The popup value that means "back to what the preset says".
const DEFAULT: &str = "";
/// The popup value of the line that only shows a chain set by hand (decthemectl / decebal.json); picking it changes nothing.
const KEEP: &str = "\u{1}keep";

type Opt = (String, String);

fn role_id(r: Role) -> String {
    format!("th:font:{}", r.key())
}

/// "Not installed" marker for a preset in the picker.
fn preset_label(p: &fonts::Preset, db: &fontdb::FontDb) -> String {
    if p.installed(db) {
        format!("{} ({})", p.title, p.brand())
    } else {
        format!("{} (not installed)", p.title)
    }
}

pub fn items(sel: &Selection, now: &Theme) -> Vec<Item> {
    items_with(sel, now, &fontdb::rescan_if_older(Duration::from_secs(30)))
}

/// [`items`] against a given list of installed fonts.
pub fn items_with(sel: &Selection, now: &Theme, db: &fontdb::FontDb) -> Vec<Item> {
    let res = now.fonts.resolve(db);
    let current = fonts::preset(&now.fonts.preset);

    let mut presets: Vec<Opt> = vec![(format!("Theme default ({})", current.filter(|_| sel.fonts.preset.is_none()).map_or("inter", |p| p.id)), DEFAULT.to_string())];
    presets.extend(PRESETS.iter().map(|p| (preset_label(p, db), p.id.to_string())));
    let preset_sel = Some(sel.fonts.preset.as_deref().and_then(fonts::preset).map_or(0, |p| 1 + PRESETS.iter().position(|x| x.id == p.id).unwrap_or(0)));
    let preset_detail = match current {
        Some(p) if !p.installed(db) => format!("{}. {} is not installed; using {}", p.blurb, p.brand(), res.primary(Role::Ui).unwrap_or("the system font")),
        Some(p) => p.blurb.to_string(),
        None => String::new(),
    };

    let families = db.picker_families();
    let mut rows = vec![Row::new("Font preset").detail(preset_detail).ctl(Ctl::new("th:fontpreset", Kind::Popup(presets, preset_sel)))];
    for r in Role::ALL {
        rows.push(role_row(r, sel, &res, &now.fonts, &families, current.is_some_and(|p| !p.installed(db))));
    }
    rows.push(Row::new("Reset fonts").detail("Back to the theme's own fonts (the text size stays)").ctl(Ctl::new("th:fontreset", Kind::Button { label: "Reset".into(), danger: false })));

    let mono_digits = "0123456789 [] {} fn main() 日本語 😀  \u{f120} \u{f09b}";
    let samples = vec![
        FontSample { label: "Interface".into(), role: Role::Ui, text: SAMPLE.into(), px: 14.0, weight: Weight::Regular },
        FontSample { label: "Titles".into(), role: Role::Title, text: "Untitled document  日本語 😀 \u{f015} Settings".into(), px: 14.0, weight: Weight::SemiBold },
        FontSample { label: "Monospace".into(), role: Role::Mono, text: mono_digits.into(), px: 13.0, weight: Weight::Regular },
        FontSample { label: "Clock and numbers".into(), role: Role::Display, text: "12:45  0123456789  Mon 28 Oct".into(), px: 22.0, weight: Weight::Medium },
    ];

    let hidden = db.family_count().saturating_sub(families.len());
    let note = format!(
        "A character the first font lacks (CJK, emoji, icons) is drawn by the next font of the chain, then by Noto Sans CJK, an emoji font and a Nerd Font when installed. {} installed families are listed{}. Chains of several fonts: decthemectl font set ROLE A,B,C.",
        families.len(),
        if hidden > 0 { format!(" ({hidden} script-specific Noto families are used automatically and left out)") } else { String::new() }
    );
    vec![Item::Header("Fonts".into()), Item::FontSamples(samples), Item::Card(rows), Item::Note { text: note, warn: false }]
}

fn role_row(r: Role, sel: &Selection, res: &fonts::Resolved, plan: &fonts::FontPlan, families: &[String], preset_missing: bool) -> Row {
    let mut opts: Vec<Opt> = vec![("Preset default".to_string(), DEFAULT.to_string())];
    let chosen = sel.fonts.role(r);
    let mut selected = Some(0);
    if let Some(chain) = chosen {
        let single = (chain.len() == 1).then(|| families.iter().position(|f| norm(f) == norm(&chain[0]))).flatten();
        match single {
            Some(i) => selected = Some(i + 1),
            None => {
                opts.push((format!("{} (set by hand)", chain.join(", ")), KEEP.to_string()));
                selected = Some(1);
            }
        }
    }
    opts.extend(families.iter().map(|f| (f.clone(), f.clone())));
    let using = res.primary(r).unwrap_or("nothing");
    let wanted = plan.chain(r).first().map(String::as_str).unwrap_or("");
    let detail = if (chosen.is_some() || preset_missing) && !wanted.is_empty() && norm(using) != norm(wanted) && !matches!(norm(wanted).as_str(), "sansserif" | "monospace") {
        format!("Using {using} ({wanted} is not installed)")
    } else {
        format!("Using {using}")
    };
    Row::new(r.label()).detail(detail).ctl(Ctl::new(role_id(r), Kind::Popup(opts, selected)))
}

/// Handles the controls of this section. `Some(true)` = the selection changed, `Some(false)` = handled, nothing to store, `None` = not ours.
pub fn apply(parts: &[&str], v: &CtlVal, sel: &mut Selection) -> Option<bool> {
    match (parts, v) {
        (["th", "fontpreset"], CtlVal::Str(s)) => {
            if s == DEFAULT {
                sel.fonts.preset = None;
                sel.font = None;
            } else {
                let p = fonts::preset(s)?;
                fonts::apply_cmd(sel, &Cmd::Preset(p.id.to_string()));
            }
            Some(true)
        }
        (["th", "font", role], CtlVal::Str(s)) => {
            let r = Role::parse(role)?;
            match s.as_str() {
                KEEP => return Some(false),
                DEFAULT => sel.fonts.roles[r.idx()] = None,
                fam => sel.fonts.roles[r.idx()] = Some(vec![fam.to_string()]),
            }
            Some(true)
        }
        (["th", "font", role], CtlVal::Reset) => {
            sel.fonts.roles[Role::parse(role)?.idx()] = None;
            Some(true)
        }
        (["th", "fontpreset"], CtlVal::Reset) | (["th", "fontreset"], CtlVal::Click) => {
            sel.fonts = FontChoice::default();
            sel.font = None;
            Some(true)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin;
    use crate::theme::fontdb::fake;

    fn theme(sel: &Selection) -> Theme {
        builtin::def(&sel.name).unwrap().resolve(&sel.name, true, sel)
    }

    fn popup<'a>(items: &'a [Item], id: &str) -> (&'a Vec<Opt>, Option<usize>) {
        for it in items {
            if let Item::Card(rows) = it {
                for r in rows {
                    for c in &r.ctls {
                        if let (true, Kind::Popup(o, s)) = (c.id == id, &c.kind) {
                            return (o, *s);
                        }
                    }
                }
            }
        }
        panic!("no popup {id}");
    }

    #[test]
    fn the_section_lists_presets_with_install_state_and_one_picker_per_role() {
        let sel = Selection::default();
        let items = items_with(&sel, &theme(&sel), &fake::db());
        let (presets, selected) = popup(&items, "th:fontpreset");
        assert_eq!(selected, Some(0), "no user choice: the theme default row");
        assert_eq!(presets.len(), PRESETS.len() + 1);
        let label = |id: &str| presets.iter().find(|o| o.1 == id).unwrap().0.clone();
        assert_eq!(label("segoe"), "Segoe (not installed)");
        assert_eq!(label("inter"), "Inter (Inter)");
        for r in Role::ALL {
            let (opts, sel) = popup(&items, &role_id(r));
            assert_eq!(sel, Some(0));
            assert_eq!(opts[0].1, DEFAULT);
            assert!(opts.iter().any(|o| o.0 == "Noto Sans CJK JP") && !opts.iter().any(|o| o.0 == "Noto Sans Adlam"));
        }
        assert!(items.iter().any(|i| matches!(i, Item::FontSamples(s) if s.len() == 4 && s[0].text.contains("日本語") && s[0].text.contains('😀'))));
    }

    #[test]
    fn choices_are_stored_and_shown_as_selected() {
        let mut sel = Selection::default();
        assert_eq!(apply(&["th", "fontpreset"], &CtlVal::Str("segoe".into()), &mut sel), Some(true));
        assert_eq!(sel.fonts.preset.as_deref(), Some("segoe"));
        assert_eq!(apply(&["th", "font", "mono"], &CtlVal::Str("Liberation Mono".into()), &mut sel), Some(true));
        assert_eq!(sel.fonts.roles[Role::Mono.idx()], Some(vec!["Liberation Mono".to_string()]));
        let items = items_with(&sel, &theme(&sel), &fake::db());
        let (presets, selected) = popup(&items, "th:fontpreset");
        assert_eq!(presets[selected.unwrap()].1, "segoe");
        let (opts, selected) = popup(&items, "th:font:mono");
        assert_eq!(opts[selected.unwrap()].1, "Liberation Mono");
        // the interface row explains the substitution
        let detail = items.iter().find_map(|i| if let Item::Card(rows) = i { rows.iter().find(|r| r.label == "Interface").map(|r| r.detail.clone()) } else { None }).unwrap();
        assert!(detail.contains("Using Noto Sans") && detail.contains("Segoe UI Variable is not installed"), "{detail}");
        // back to the preset's default for one role, then everything
        assert_eq!(apply(&["th", "font", "mono"], &CtlVal::Str(DEFAULT.into()), &mut sel), Some(true));
        assert!(sel.fonts.roles[2].is_none() && sel.fonts.preset.is_some());
        assert_eq!(apply(&["th", "fontreset"], &CtlVal::Click, &mut sel), Some(true));
        assert!(sel.fonts.is_empty());
    }

    #[test]
    fn a_chain_set_by_hand_is_shown_and_not_clobbered() {
        let mut sel = Selection::default();
        sel.fonts.roles[0] = Some(vec!["Inter".into(), "Noto Sans CJK".into()]);
        let items = items_with(&sel, &theme(&sel), &fake::db());
        let (opts, selected) = popup(&items, "th:font:ui");
        assert_eq!(opts[selected.unwrap()].0, "Inter, Noto Sans CJK (set by hand)");
        assert_eq!(apply(&["th", "font", "ui"], &CtlVal::Str(KEEP.into()), &mut sel), Some(false));
        assert_eq!(sel.fonts.roles[0].as_ref().unwrap().len(), 2);
    }

    #[test]
    fn theme_default_clears_the_preset_and_unknown_controls_are_not_ours() {
        let mut sel = Selection::default();
        sel.fonts.preset = Some("fira".into());
        assert_eq!(apply(&["th", "fontpreset"], &CtlVal::Str(DEFAULT.into()), &mut sel), Some(true));
        assert!(sel.fonts.preset.is_none());
        assert_eq!(apply(&["th", "radius"], &CtlVal::Num(3.0), &mut sel), None);
        assert_eq!(apply(&["th", "font", "sparkle"], &CtlVal::Str("x".into()), &mut sel), None);
        assert_eq!(apply(&["th", "fontpreset"], &CtlVal::Str("comic".into()), &mut sel), None);
    }
}
