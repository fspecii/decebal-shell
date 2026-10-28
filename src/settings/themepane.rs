//! The "Theme" pane: gallery of themes with live miniature previews, light/dark/auto, accent
//! swatches, translucency / corner radius / text size, and "also style rdwm".
//!
//! Choosing anything runs one transaction (see [`crate::theme::rdwm`]): validate rdwm's colour edit,
//! write it through the model's atomic store, then write the selection to `decebal.json`. Every
//! running decebal program notices that file within half a second.

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Row, Swatch, ThemeTile};
use crate::config::BarPosition;
use crate::theme::live::{system_dark, Source};
use crate::theme::model::{Mode, Rgb, Selection};
use crate::theme::rdwm;
use crate::theme::style::{self, Style};

/// macOS accent colours, in the order System Settings shows them.
pub const ACCENTS: &[(&str, &str)] =
    &[("Blue", "#0a84ff"), ("Purple", "#bf5af2"), ("Pink", "#ff375f"), ("Red", "#ff453a"), ("Orange", "#ff9f0a"), ("Yellow", "#ffd60a"), ("Green", "#30d158"), ("Graphite", "#8e8e93")];

pub(super) fn source(m: &Model) -> Source {
    Source { shell_json: m.paths.shell_json(), themes_dir: Source::discover().themes_dir }
}

/// What the pieces of the Appearance tabs share: the stored selection, the theme built from it and the system's light/dark.
pub struct Ctx {
    pub src: Source,
    pub sel: Selection,
    pub sys: bool,
    pub now: crate::theme::model::Theme,
    pub problem: Option<String>,
    pub current_id: String,
}

impl Ctx {
    pub fn new(m: &Model) -> Ctx {
        let src = source(m);
        let sel = src.selection();
        let sys = system_dark();
        let (now, problem) = src.build(&sel, sys);
        let current_id = src.find(&sel.name).map(|f| f.0).unwrap_or_default();
        Ctx { src, sel, sys, now, problem, current_id }
    }
}

/// Theme tab: the Style picker (theme, dock and menu bar together).
pub fn style_items(m: &Model, c: &Ctx) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(p) = &c.problem {
        out.push(Item::Note { text: format!("The selected theme could not be loaded ({p}); the default theme is shown instead."), warn: true });
    }
    let style_opts: Vec<(String, String)> = Style::ALL.iter().map(|s| (s.title().to_string(), s.as_str().to_string())).collect();
    let cur_style = style::detect(&c.current_id, style::dock_style(&m.dock));
    out.push(Item::Header("Style".into()));
    let mut shell_rows = vec![Row::new("Desktop style")
        .detail(match cur_style {
            Some(Style::Macos) => "macOS: menu bar and floating dock, macOS theme",
            Some(Style::Windows) => "Windows: Windows 11 theme and taskbar (the menu bar hides)",
            Some(Style::Caelestia) => "Caelestia: Material You colours, a pill bar (see Layout), no dock",
            None => "Custom: pick one to set theme, dock and menu bar together",
        })
        .ctl(Ctl::new("th:style", Kind::Seg(style_opts, cur_style.map(|s| Style::ALL.iter().position(|x| *x == s).unwrap_or(0)))))];
    if cur_style == Some(Style::Caelestia) {
        let pos = style::position_of(&m.shell);
        let opts: Vec<(String, String)> = BarPosition::ALL.iter().map(|p| (p.title().to_string(), p.as_str().to_string())).collect();
        shell_rows.push(
            Row::new("Layout")
                .detail(match pos {
                    BarPosition::Left => "Left: a vertical bar down the left edge; popouts open to the right",
                    BarPosition::Right => "Right: a vertical bar down the right edge; popouts open to the left",
                    BarPosition::Top => "Top: a horizontal bar along the top edge; popouts open downwards",
                    BarPosition::Bottom => "Bottom: a horizontal bar along the bottom edge; popouts open upwards",
                    BarPosition::Split => "Split: launcher and workspaces on top, tray and clock below",
                })
                .ctl(Ctl::new("th:layout", Kind::Seg(opts, BarPosition::ALL.iter().position(|p| *p == pos)))),
        );
    }
    out.push(Item::Card(shell_rows));
    out
}

/// Theme tab: light / dark / auto.
pub fn mode_items(c: &Ctx) -> Vec<Item> {
    let mode_opts = vec![("Light".to_string(), "light".to_string()), ("Dark".to_string(), "dark".to_string()), ("Auto".to_string(), "auto".to_string())];
    let mode_sel = Some(match c.sel.mode {
        Mode::Light => 0,
        Mode::Dark => 1,
        Mode::Auto => 2,
    });
    vec![
        Item::Header("Light or dark".into()),
        Item::Card(vec![Row::new("Mode")
            .detail(if c.sel.mode == Mode::Auto {
                format!("Following the system: {} right now", if c.sys { "dark" } else { "light" })
            } else {
                "Themes with both variants switch; single-variant themes keep their look".into()
            })
            .ctl(Ctl::new("th:mode", Kind::Seg(mode_opts, mode_sel)))]),
    ]
}

/// Customise > Colours: the colour scheme tiles (colours only, not the whole package).
pub fn scheme_items(c: &Ctx) -> Vec<Item> {
    let mut tiles = Vec::new();
    for e in c.src.list() {
        let selected = e.id == c.current_id;
        if let Some(err) = &e.error {
            if !e.builtin {
                let _ = err;
                tiles.push(ThemeTile { id: format!("l:theme:{}", e.id), name: format!("{} (error)", e.name), selected: false, error: true, colors: [(60, 20, 20); 7] });
                continue;
            }
        }
        // every tile previews its theme the way it would look right now: with "match wallpaper" applied when it is on
        let tile_sel = if selected { c.sel.clone() } else { Selection { mode: c.sel.mode, dynamic: c.sel.dynamic.clone(), ..Selection::default() } };
        let Ok(t) = c.src.theme_for(&e.id, &tile_sel, c.sys) else { continue };
        tiles.push(ThemeTile {
            id: format!("l:theme:{}", e.id),
            name: e.name.clone(),
            selected,
            error: false,
            colors: [t.background.t(), t.bar.t(), t.surface.t(), t.titlebar.t(), t.text.t(), t.accent.t(), t.dock.t()],
        });
    }
    vec![
        Item::Header("Colour scheme".into()),
        Item::Note { text: "Changes only the colours. To switch the whole look (fonts, windows, dock, wallpaper) pick a theme on the Theme tab.".into(), warn: false },
        Item::Themes(tiles),
    ]
}

/// Customise > Colours: accent swatches and the custom accent.
pub fn accent_items(c: &Ctx) -> Vec<Item> {
    let mut swatches = vec![Swatch {
        id: "l:accent:none".into(),
        name: "Theme default".into(),
        color: c.src.build(&Selection { accent: None, ..c.sel.clone() }, c.sys).0.accent.t(),
        selected: c.sel.accent.is_none(),
        unset: true,
    }];
    for (name, hex) in ACCENTS {
        let col = Rgb::parse(hex).unwrap();
        swatches.push(Swatch { id: format!("l:accent:{hex}"), name: (*name).into(), color: col.t(), selected: c.sel.accent == Some(col), unset: false });
    }
    vec![
        Item::Header("Accent colour".into()),
        Item::Swatches(swatches),
        Item::Card(vec![Row::new("Custom accent")
            .detail("A hex colour; empty = the theme's own")
            .ctl(Ctl::new("th:accenthex", Kind::Text { val: c.sel.accent.map(|x| x.hex()).unwrap_or_default(), hint: "#0a84ff".into() }))]),
    ]
}

fn slider(id: &str, label: &str, detail: &str, v: f64, min: f64, max: f64, unit: &str) -> Row {
    Row::new(label).detail(detail).ctl(Ctl::new(id, Kind::Slider { v, min, max, step: 1.0, unit: unit.into() }))
}

/// Customise > Colours: translucency and corner radius.
pub fn surface_items(c: &Ctx) -> Vec<Item> {
    vec![
        Item::Header("Menus, dock and corners".into()),
        Item::Card(vec![
            slider("th:opacity", "Translucency", "Opacity of menus, dock and launchers (the menu bar stays solid)", f64::from(c.sel.opacity.unwrap_or(1.0)) * 100.0, 40.0, 100.0, " %"),
            slider("th:radius", "Corner radius", "Menus, cards and window title bars", f64::from(c.now.radius), 0.0, 24.0, " px"),
        ]),
    ]
}

/// Customise > Fonts: the text size slider (the font pickers are [`super::fontpane`]).
pub fn text_size_items(c: &Ctx) -> Vec<Item> {
    vec![Item::Card(vec![slider("th:fontscale", "Text size", "All decebal text", f64::from(c.sel.font_scale.unwrap_or(1.0)) * 100.0, 80.0, 140.0, " %")])]
}

/// Customise > Window style: should rdwm's title bars, borders and strip follow the theme.
pub fn frame_sync_rows(c: &Ctx) -> Vec<Item> {
    vec![Item::Card(vec![Row::new("Windows follow the theme")
        .detail("Colour window title bars, borders and the window manager strip with the theme")
        .ctl(Ctl::new("th:sync", Kind::Toggle(c.sel.sync_rdwm)))])]
}

/// Theme tab: where the theme files live and the ones that failed to load.
pub fn library_note(c: &Ctx) -> Vec<Item> {
    let broken: Vec<String> = c.src.list().iter().filter_map(|e| e.error.as_ref().map(|err| format!("{}: {err}", e.id))).collect();
    let mut note = format!(
        "Your own themes: put .toml files in {} (decthemectl export nord > mine.toml is a good start). decthemectl list, apply and get do the same from a terminal.",
        c.src.themes_dir.display()
    );
    if !broken.is_empty() {
        note.push_str(&format!("\nIgnored: {}", broken.join("; ")));
    }
    vec![Item::Note { text: note, warn: !broken.is_empty() }]
}

/// Every piece of the old Theme pane in one list (the tabs of [`super::appearance`] show them split up; tests use this).
pub fn items(m: &Model) -> Vec<Item> {
    let c = Ctx::new(m);
    let mut out = style_items(m, &c);
    out.extend(scheme_items(&c));
    out.extend(super::packages::items(m));
    out.extend(mode_items(&c));
    out.extend(frame_sync_rows(&c));
    out.extend(super::wallpapercolors::items(&c.sel, &c.now));
    out.extend(accent_items(&c));
    out.extend(surface_items(&c));
    out.extend(text_size_items(&c));
    out.extend(super::fontpane::items(&c.sel, &c.now));
    out.extend(library_note(&c));
    out.extend(super::systempane::items(m));
    out
}

/// Runs the transaction for a changed selection; true when it was written.
pub(super) fn commit(m: &mut Model, sel: Selection) -> bool {
    let src = source(m);
    let (theme, problem) = src.build(&sel, system_dark());
    if let Some(p) = problem {
        m.say(format!("Not applied: {p}"), true);
        return false;
    }
    if sel.sync_rdwm {
        m.notice = None;
        m.edit(|c| rdwm::apply_to(c, &theme));
        if m.notice.as_ref().is_some_and(|n| n.error) {
            return false; // rdwm's file would be invalid: nothing was written, the selection stays as it was
        }
    }
    match src.write_selection(&sel) {
        Ok(()) => {
            crate::theme::live::reload();
            crate::theme::system::request(&m.paths.shell_json()); // "Apply to all apps", when on
            m.dirty = true;
            if m.notice.is_none() {
                m.say(format!("Theme: {}", theme.name), false);
            }
            true
        }
        Err(e) => {
            m.say(format!("Not saved: {e}"), true);
            false
        }
    }
}

/// A new wallpaper was chosen in the Wallpaper pane: the colours follow it when "Match wallpaper" is on.
pub fn follow_wallpaper(m: &mut Model, path: &str) {
    super::wallpapercolors::follow(m, path);
}

pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    if super::packages::apply(m, parts, &v) {
        return;
    }
    let src = source(m);
    let mut sel = src.selection();
    if let Some(changed) = super::fontpane::apply(parts, &v, &mut sel) {
        if changed {
            commit(m, sel);
        }
        return;
    }
    match (parts, v) {
        (["l", "theme", id], CtlVal::Click) => {
            let mode = src.find(id).and_then(|f| f.1);
            sel.name = (*id).to_string();
            if let Some(mode) = mode {
                sel.mode = mode;
            }
        }
        (["l", "accent", "none"], CtlVal::Click) => sel.accent = None,
        (["l", "accent", hex], CtlVal::Click) => sel.accent = Rgb::parse(hex),
        (["th", "mode"], CtlVal::Str(s)) => sel.mode = Mode::parse(&s).unwrap_or(sel.mode),
        (["th", "sync"], CtlVal::Bool(b)) => sel.sync_rdwm = b,
        (["th", "style"], CtlVal::Str(s)) => {
            let Some(st) = Style::parse(&s) else { return };
            if commit(m, style::selection_for(st, &sel)) {
                for (key, val) in style::dock_edits(st) {
                    m.edit_json("dock", key, val);
                }
                let bar = style::bar_value(st, m.shell.get("bar"));
                m.edit_json("shell", "bar", Some(bar));
                let dock = style::dock_value(st, m.shell.get("dock"));
                m.edit_json("shell", "dock", Some(dock));
                m.say(format!("Shell style: {}", st.title()), false);
            }
            return;
        }
        (["th", "layout"], CtlVal::Str(s)) => {
            let Some(pos) = BarPosition::parse(&s) else { return };
            let bar = style::position_value(pos, m.shell.get("bar"));
            m.edit_json("shell", "bar", Some(bar));
            m.say(format!("Caelestia layout: {}", pos.title()), false);
            return;
        }
        // the "Match wallpaper" section (th:dynamic, th:dyn-*) lives in wallpapercolors.rs
        (["th", id], v) if id.starts_with("dyn") => match super::wallpapercolors::apply(m, &sel, id, v) {
            Ok(Some(new)) => sel = new,
            Ok(None) => return,
            Err(e) => {
                m.say(format!("Not applied: {e}"), true);
                return;
            }
        },
        (["th", "accenthex"], CtlVal::Str(s)) => {
            if s.trim().is_empty() {
                sel.accent = None;
            } else if let Some(c) = Rgb::parse(&s) {
                sel.accent = Some(c);
            } else {
                m.say(format!("`{s}` is not a colour; use #rrggbb"), true);
                return;
            }
        }
        (["th", "accenthex"], CtlVal::Reset) => sel.accent = None,
        (["th", "opacity"], CtlVal::Num(n)) => sel.opacity = Some((n / 100.0).clamp(0.4, 1.0) as f32),
        (["th", "opacity"], CtlVal::Reset) => sel.opacity = None,
        (["th", "radius"], CtlVal::Num(n)) => sel.radius = Some(n.clamp(0.0, 24.0) as f32),
        (["th", "radius"], CtlVal::Reset) => sel.radius = None,
        (["th", "fontscale"], CtlVal::Num(n)) => sel.font_scale = Some((n / 100.0).clamp(0.8, 1.4) as f32),
        (["th", "fontscale"], CtlVal::Reset) => sel.font_scale = None,
        (["th", "reset-colours"], CtlVal::Click) => {
            sel.accent = None;
            sel.opacity = None;
            sel.radius = None;
        }
        (["th", "reset"], CtlVal::Click) => {
            sel = Selection { name: sel.name.clone(), mode: sel.mode, sync_rdwm: sel.sync_rdwm, font: sel.font.clone(), fonts: sel.fonts.clone(), ..Selection::default() }
        }
        _ => return,
    }
    commit(m, sel);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;

    fn selection(m: &Model) -> Selection {
        source(m).selection()
    }

    #[test]
    fn gallery_lists_every_builtin_and_marks_the_current_one() {
        let (m, _) = model_with("[general]\n");
        let items = items(&m);
        let Some(Item::Themes(tiles)) = items.iter().find(|i| matches!(i, Item::Themes(_))) else { panic!("no gallery") };
        assert!(tiles.len() >= crate::theme::builtin::BUILTIN.len());
        assert_eq!(tiles.iter().filter(|t| t.selected).count(), 1);
        assert!(tiles.iter().any(|t| t.id == "l:theme:windows11" && t.name == "Windows 11"));
        assert!(items.iter().any(|i| matches!(i, Item::Swatches(s) if s.len() == ACCENTS.len() + 1)));
    }

    #[test]
    fn choosing_a_theme_writes_selection_and_rdwm_colours_in_one_go() {
        let (mut m, cfg) = model_with("# keep me\n[general]\ntags = [\"a\"]\n");
        m.apply("l:theme:nord", CtlVal::Click);
        assert_eq!(selection(&m).name, "nord");
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.starts_with("# keep me\n"), "{text}");
        assert!(text.contains("focused_bg") && text.contains("[decoration.bar]"));
        m.apply("l:theme:windows11", CtlVal::Click);
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("style = \"classic\""), "{text}");
    }

    #[test]
    fn controls_set_overrides_and_reset_clears_them() {
        let (mut m, _) = model_with("");
        m.apply("th:mode", CtlVal::Str("light".into()));
        m.apply("l:accent:#ff9f0a", CtlVal::Click);
        m.apply("th:opacity", CtlVal::Num(80.0));
        m.apply("th:radius", CtlVal::Num(4.0));
        m.apply("th:fontscale", CtlVal::Num(110.0));
        let s = selection(&m);
        assert_eq!((s.mode, s.accent, s.radius), (Mode::Light, Rgb::parse("#ff9f0a"), Some(4.0)));
        assert!((s.opacity.unwrap() - 0.8).abs() < 1e-3 && (s.font_scale.unwrap() - 1.1).abs() < 1e-3);
        m.apply("th:accenthex", CtlVal::Str("nonsense".into()));
        assert_eq!(selection(&m).accent, Rgb::parse("#ff9f0a"), "a bad colour is refused and keeps the old one");
        assert!(m.notice.as_ref().unwrap().error);
        m.apply("th:reset", CtlVal::Click);
        let s = selection(&m);
        assert_eq!((s.mode, s.accent, s.opacity, s.radius, s.font_scale), (Mode::Light, None, None, None, None));
        m.apply("th:sync", CtlVal::Bool(false));
        assert!(!selection(&m).sync_rdwm);
    }

    #[test]
    fn the_shell_style_picker_leads_the_pane_and_sets_theme_dock_and_bar() {
        let (mut m, cfg) = model_with("[general]\n");
        let its = items(&m);
        let first_ctl =
            its.iter()
                .find_map(|i| if let Item::Card(rows) = i { rows.first().and_then(|r| r.ctls.first()).map(|c| (c.id.clone(), matches!(&c.kind, Kind::Seg(o, _) if o.len() == 3))) } else { None });
        assert_eq!(first_ctl, Some(("th:style".to_string(), true)), "the first control is the three-way style picker");
        assert!(matches!(its.iter().position(|i| matches!(i, Item::Header(h) if h == "Style")), Some(p) if p < 2), "it is at the top");

        m.apply("th:style", CtlVal::Str("caelestia".into()));
        assert_eq!(selection(&m).name, "caelestia");
        assert_eq!(m.dock["style"], "mac");
        assert_eq!(m.shell["bar"]["enabled"], true);
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("focused_bg"), "rdwm follows too");
        assert_eq!(m.shell["dock"]["enabled"], "auto", "Caelestia has no dock: it hides under the vertical bar");

        m.apply("th:style", CtlVal::Str("windows".into()));
        assert_eq!(selection(&m).name, "windows11");
        assert_eq!((m.dock["style"].as_str(), m.dock["windowsAlign"].as_str()), (Some("windows"), Some("center")));
        assert_eq!(m.shell["bar"]["enabled"], "auto");
        assert_eq!(m.shell["dock"]["enabled"], true, "the dock is back");
        let file = crate::settings::io::json_load(&m.paths.dock_settings());
        assert_eq!(file["style"], "windows", "written to dock-settings.json");

        m.apply("th:style", CtlVal::Str("macos".into()));
        assert_eq!(selection(&m).name, "macos");
        assert_eq!(crate::settings::io::json_load(&m.paths.dock_settings())["style"], "mac");
        assert_eq!(m.shell["dock"]["enabled"], true);
        let sel_idx = |m: &Model| match items(m).iter().find(|i| matches!(i, Item::Card(_))) {
            Some(Item::Card(rows)) => match rows.first().and_then(|r| r.ctls.first()).map(|c| &c.kind) {
                Some(Kind::Seg(_, s)) => *s,
                _ => panic!("not a segmented control"),
            },
            _ => panic!("no card"),
        };
        assert_eq!(sel_idx(&m), Some(0), "macOS is highlighted");
        m.apply("l:theme:nord", CtlVal::Click);
        assert_eq!(sel_idx(&m), None, "a hand-picked theme is Custom");
    }

    fn layout_row(its: &[Item]) -> Option<(Vec<String>, Option<usize>)> {
        its.iter().find_map(|i| match i {
            Item::Card(rows) => rows.iter().find(|r| r.label == "Layout").and_then(|r| r.ctls.first()).and_then(|c| match &c.kind {
                Kind::Seg(opts, sel) => Some((opts.iter().map(|o| o.1.clone()).collect(), *sel)),
                _ => None,
            }),
            _ => None,
        })
    }

    #[test]
    fn the_layout_row_belongs_to_the_caelestia_style_and_sets_the_bar_position() {
        let (mut m, _) = model_with("[general]\n");
        assert!(layout_row(&items(&m)).is_none(), "macOS has no layouts");
        m.apply("th:style", CtlVal::Str("windows".into()));
        assert!(layout_row(&items(&m)).is_none(), "nor has Windows");
        m.apply("th:style", CtlVal::Str("caelestia".into()));
        let (values, sel) = layout_row(&items(&m)).expect("Caelestia shows the Layout row");
        assert_eq!(values, ["left", "right", "top", "bottom", "split"]);
        assert_eq!(sel, Some(0), "left is the default");
        // it sits in the Shell style card, right under the style picker
        let first_card = items(&m).into_iter().find_map(|i| if let Item::Card(rows) = i { Some(rows) } else { None }).unwrap();
        assert_eq!(first_card.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["Desktop style", "Layout"]);
        for (i, word) in ["left", "right", "top", "bottom", "split"].iter().enumerate() {
            m.apply("th:layout", CtlVal::Str((*word).into()));
            assert_eq!(m.shell["bar"]["position"], *word);
            assert_eq!(crate::settings::io::json_load(&m.paths.shell_json())["bar"]["position"], *word, "written to decebal.json");
            assert_eq!(layout_row(&items(&m)).unwrap().1, Some(i));
            assert_eq!(m.shell["bar"]["enabled"], true, "the other bar keys stay");
            assert_eq!(selection(&m).name, "caelestia", "only the position changed");
            assert_eq!(source(&m).build(&selection(&m), true).0.pill_bar(), BarPosition::parse(word), "and the live theme sees it");
        }
        m.apply("th:layout", CtlVal::Str("sideways".into()));
        assert_eq!(m.shell["bar"]["position"], "split", "an unknown word is ignored");
        // choosing the style again keeps the layout; another style hides the row but remembers it
        m.apply("th:style", CtlVal::Str("caelestia".into()));
        assert_eq!(layout_row(&items(&m)).unwrap().1, Some(4));
        m.apply("th:style", CtlVal::Str("macos".into()));
        assert!(layout_row(&items(&m)).is_none());
        assert_eq!(m.shell["bar"]["position"], "split");
    }

    #[test]
    fn every_theme_decthemectl_lists_loads_in_the_pane_without_the_could_not_be_loaded_note() {
        // A long-running decsettings that predates a theme (an old binary has no "caelestia" in its table) reports
        // "no theme caelestia"; a current one must load everything the CLI lists, in both modes.
        let (mut m, _) = model_with("[general]\n");
        let entries = source(&m).list();
        assert!(entries.iter().any(|e| e.id == "caelestia"), "the listing has Caelestia");
        for e in entries.iter().filter(|e| e.error.is_none()) {
            for mode in ["light", "dark", "auto"] {
                m.apply("l:theme:caelestia", CtlVal::Click); // start from somewhere else so every pick is a change
                m.apply("th:mode", CtlVal::Str(mode.into()));
                m.apply(&format!("l:theme:{}", e.id), CtlVal::Click);
                let sel = selection(&m);
                assert_eq!(sel.name, e.id, "{} / {mode}: the pick was stored", e.id);
                let its = items(&m);
                let warn = its.iter().find_map(|i| match i {
                    Item::Note { text, warn: true } if text.contains("could not be loaded") => Some(text.clone()),
                    _ => None,
                });
                assert!(warn.is_none(), "{} / {mode}: {warn:?}", e.id);
                let Some(Item::Themes(tiles)) = its.iter().find(|i| matches!(i, Item::Themes(_))) else { panic!("no gallery") };
                let chosen: Vec<&str> = tiles.iter().filter(|t| t.selected).map(|t| t.id.as_str()).collect();
                assert_eq!(chosen, [format!("l:theme:{}", e.id)], "{} / {mode}: exactly its own tile is highlighted", e.id);
                let (theme, problem) = source(&m).build(&sel, true);
                assert!(problem.is_none() && theme.id == e.id, "{} / {mode}: {problem:?}", e.id);
            }
        }
    }

    #[test]
    fn a_theme_that_really_does_not_exist_still_gets_the_note() {
        let (m, _) = model_with("[general]\n");
        source(&m).write_selection(&Selection { name: "no-such-theme".into(), ..Selection::default() }).unwrap();
        let its = items(&m);
        assert!(its.iter().any(|i| matches!(i, Item::Note { text, warn: true } if text.contains("no theme `no-such-theme`") && text.contains("default theme is shown instead"))));
    }

    #[test]
    fn a_broken_rdwm_file_blocks_the_whole_change() {
        let (mut m, cfg) = model_with("[general]\n");
        m.apply("l:theme:dracula", CtlVal::Click);
        assert_eq!(selection(&m).name, "dracula");
        // simulate a file rdwm would reject by pointing the store at a checker that always fails
        m.store.rdwmctl = "/bin/false".into();
        let before = std::fs::read_to_string(&cfg).unwrap();
        m.apply("l:theme:nord", CtlVal::Click);
        assert_eq!(selection(&m).name, "dracula", "selection unchanged when rdwm's file would be invalid");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), before);
        assert!(m.notice.as_ref().unwrap().error);
    }
}
