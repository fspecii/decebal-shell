//! The "Match wallpaper" section of the Palettes pane (and the hooks that make it follow wallpaper changes).
//!
//! An optional layer over *any* theme (see [`crate::theme::dynamic`]): a switch, the scheme, what it recolours (accent
//! only, tinted surfaces, the full palette), strength and contrast, whether light/dark follows the picture, the source
//! (the wallpaper, an image file, a colour), a strip previewing the derived palette and a "Refresh now" button.
//!
//! Kept out of `themepane.rs` so that file only has to call [`items`] and [`apply`]. Every change goes through the pane's
//! `commit` (validated rdwm edit, then the selection), so running programs follow within half a second.

use super::model::{CtlVal, Model};
use super::ui::{Chip, Ctl, Item, Kind, Row};
use crate::theme::dynamic;
use crate::theme::live::Source;
use crate::theme::material::Variant;
use crate::theme::model::{Dynamic, Origin, Rgb, Selection, Surfaces, Theme};
use std::path::Path;

fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

/// Wallpaper pane hint: the colours follow what is chosen there. `None` while the feature is off.
pub fn wallpaper_pane_note(m: &Model) -> Option<Item> {
    let d = Source { shell_json: m.paths.shell_json(), themes_dir: Source::discover().themes_dir }.selection().dynamic.filter(|d| d.enabled && d.source == Origin::Wallpaper)?;
    Some(Item::Note { text: format!("Match wallpaper is on ({}): the shell's colours follow the wallpaper you choose here. Adjust the matching controls here.", d.scheme.title()), warn: false })
}

/// The section's items. `now` is the theme as resolved with the layer applied.
pub fn items(sel: &Selection, now: &Theme) -> Vec<Item> {
    let d = sel.dynamic.clone().unwrap_or_default();
    let mut out = vec![Item::Header("Match wallpaper".into())];
    let switch = Row::new("Match wallpaper colours")
        .detail(if d.enabled {
            match &d.source {
                Origin::Wallpaper => format!("Following the wallpaper{}", d.image.as_deref().map(|i| format!(" ({})", file_name(i))).unwrap_or_default()),
                Origin::File(p) => format!("Colours of {}", file_name(p)),
                Origin::Color(c) => format!("Colours of {}", c.hex()),
            }
        } else {
            "Recolour the theme from the wallpaper's colours. Works with every theme; off by default".into()
        })
        .ctl(Ctl::new("th:dynamic", Kind::Toggle(d.enabled)));
    let mut rows = vec![switch];
    if d.enabled {
        let opts: Vec<(String, String)> = Variant::ALL.iter().map(|v| (v.title().to_string(), v.as_str().to_string())).collect();
        rows.push(Row::new("Scheme").detail(d.scheme.blurb()).ctl(Ctl::new("th:dyn-scheme", Kind::Popup(opts, Variant::ALL.iter().position(|v| *v == d.scheme)))));
        let surf: Vec<(String, String)> = Surfaces::ALL.iter().map(|s| (s.title().to_string(), s.as_str().to_string())).collect();
        rows.push(
            Row::new("Recolour")
                .detail(match d.surfaces {
                    Surfaces::Accent => "Only the accent; backgrounds and text stay as the theme has them",
                    Surfaces::Tint => "Accent plus backgrounds and text tinted; the theme keeps its lights and darks",
                    Surfaces::Full => "Blend into the wallpaper's whole palette (100 % replaces the theme's colours)",
                })
                .ctl(Ctl::new("th:dyn-surfaces", Kind::Seg(surf, Surfaces::ALL.iter().position(|s| *s == d.surfaces)))),
        );
        if d.surfaces != Surfaces::Accent {
            rows.push(
                Row::new("Strength")
                    .detail("How far backgrounds and text move")
                    .ctl(Ctl::new("th:dyn-strength", Kind::Slider { v: f64::from(d.strength) * 100.0, min: 0.0, max: 100.0, step: 5.0, unit: " %".into() })),
            );
        }
        rows.push(
            Row::new("Contrast")
                .detail("Softer or stronger separation between text, accent and surfaces")
                .ctl(Ctl::new("th:dyn-contrast", Kind::Slider { v: f64::from(d.contrast) * 100.0, min: -100.0, max: 100.0, step: 10.0, unit: " %".into() })),
        );
        rows.push(
            Row::new("Light or dark")
                .detail(if d.follow_mode {
                    format!("Following the picture: a {} one, so the {} variant", if d.colors.tone >= dynamic::LIGHT_TONE { "light" } else { "dark" }, if now.dark { "dark" } else { "light" })
                } else {
                    "The Mode setting above decides".into()
                })
                .ctl(Ctl::new("th:dyn-mode", Kind::Seg(vec![("Mode setting".into(), "keep".into()), ("Follow wallpaper".into(), "wallpaper".into())], Some(usize::from(d.follow_mode))))),
        );
        let src_idx = match d.source {
            Origin::Wallpaper => 0,
            Origin::File(_) => 1,
            Origin::Color(_) => 2,
        };
        rows.push(
            Row::new("Source")
                .detail("The wallpaper on screen, a particular image, or one colour")
                .ctl(Ctl::new("th:dyn-source", Kind::Seg(vec![("Wallpaper".into(), "wallpaper".into()), ("Image file".into(), "file".into()), ("Colour".into(), "color".into())], Some(src_idx)))),
        );
        match &d.source {
            Origin::File(p) => rows.push(Row::new("Image file").detail("PNG, JPEG or WebP").ctl(Ctl::new("th:dyn-value", Kind::Text { val: p.clone(), hint: "/path/to/image.png".into() }))),
            Origin::Color(c) => rows.push(Row::new("Colour").detail("The seed everything is derived from").ctl(Ctl::new("th:dyn-value", Kind::Text { val: c.hex(), hint: "#2a9d8f".into() }))),
            Origin::Wallpaper => {}
        }
        rows.push(Row::new("Refresh now").detail("Read the wallpaper again").ctl(Ctl::new("th:dyn-refresh", Kind::Button { label: "Refresh".into(), danger: false })));
    }
    out.push(Item::Card(rows));
    if d.enabled {
        let chips: Vec<Chip> = dynamic::preview(&d, now).into_iter().map(|(name, c)| Chip { name: name.to_string(), color: c.t() }).collect();
        out.push(Item::Palette(chips));
        let mut note = String::new();
        if !d.has_colour() {
            note.push_str("This picture has no colour to match (grey, black or white), so the theme keeps its own colours. ");
        }
        if sel.accent.is_some() {
            note.push_str("Your own accent colour (below) wins over the wallpaper's. ");
        }
        if d.source == Origin::Wallpaper {
            note.push_str("The colours follow when the wallpaper is changed here, in dechaven or with `decthemectl dynamic refresh`; run `decthemectl watch` from the session to follow changes made by any tool.");
        }
        if !note.is_empty() {
            out.push(Item::Note { text: note.trim().to_string(), warn: !d.has_colour() });
        }
    }
    out
}

/// Controls for keeping the wallpaper-derived colours as a reusable, independent palette.
pub fn save_items(m: &Model) -> Item {
    let mut save = Ctl::new("th:dyn-palette-save", Kind::Button { label: "Create and apply".into(), danger: false });
    save.enabled = m.current_wallpaper().is_some();
    Item::Card(vec![
        Row::new("Custom palette name")
            .detail("Save dark and light colours from this wallpaper; keep the current theme design")
            .ctl(Ctl::new("th:dyn-palette-name", Kind::Text { val: m.wallpaper_palette_name.clone(), hint: "Wallpaper colours".into() })),
        Row::new("Create wallpaper palette").detail("The saved palette is also available in Appearance → Palettes").ctl(save),
    ])
}

/// A change to the section's controls. Returns the new selection (`Ok(None)`: not one of ours).
pub fn apply(m: &mut Model, sel: &Selection, id: &str, v: CtlVal) -> Result<Option<Selection>, String> {
    let wall = m.current_wallpaper().map(std::path::PathBuf::from);
    let paths = m.paths.clone();
    let set = |f: &dyn Fn(&mut Dynamic)| dynamic::update_with(sel, &paths, wall.as_deref(), |d| f(d)).map(Some);
    match (id, v) {
        ("dyn-palette-name", CtlVal::Str(name)) => {
            m.wallpaper_palette_name = name;
            m.dirty = true;
            Ok(None)
        }
        ("dyn-palette-save", CtlVal::Click) => {
            let derived = dynamic::update_with(sel, &paths, wall.as_deref(), |d| {
                d.enabled = true;
                d.source = Origin::Wallpaper;
            })?;
            let d = derived.dynamic.clone().ok_or("No wallpaper colours available")?;
            // The palette file is written on the persistence worker; selecting it follows in order once it exists.
            let name = m.wallpaper_palette_name.clone();
            let asked = m.theme_intent;
            m.run_task(
                "palette-save",
                Vec::new(),
                move || crate::theme::palette::save_wallpaper(&name, &d, &crate::theme::palette::user_dir()).map(serde_json::Value::String),
                move |m, r| match r {
                    // any theme intent accepted while the file was being saved (design, mode, palette, accent, wallpaper
                    // colours, even a return to the same value) is newer: keep it, the palette is only saved
                    Ok(id) if m.theme_intent != asked => m.say(format!("Palette saved as {}; not selected, because the theme was changed meanwhile", id.as_str().unwrap_or_default()), false),
                    Ok(id) => {
                        let mut next = crate::theme::palette::select(&m.theme_selection(), id.as_str());
                        next.accent = None;
                        super::themepane::commit(m, next);
                    }
                    Err(e) => m.say(format!("Not applied: {e}"), true),
                },
            );
            Ok(None)
        }
        ("dynamic", CtlVal::Bool(true)) => dynamic::enable(sel, &paths, wall.as_deref()).map(Some),
        ("dynamic", CtlVal::Bool(false)) => Ok(Some(dynamic::disable(sel))),
        ("dyn-scheme", CtlVal::Str(s)) => match Variant::parse(&s) {
            Some(v) => set(&|d| d.scheme = v),
            None => Ok(None),
        },
        ("dyn-surfaces", CtlVal::Str(s)) => match Surfaces::parse(&s) {
            Some(v) => set(&|d| d.surfaces = v),
            None => Ok(None),
        },
        ("dyn-strength", CtlVal::Num(n)) => set(&|d| d.strength = (n / 100.0) as f32),
        ("dyn-strength", CtlVal::Reset) => set(&|d| d.strength = 0.5),
        ("dyn-contrast", CtlVal::Num(n)) => set(&|d| d.contrast = (n / 100.0) as f32),
        ("dyn-contrast", CtlVal::Reset) => set(&|d| d.contrast = 0.0),
        ("dyn-mode", CtlVal::Str(s)) => set(&|d| d.follow_mode = s == "wallpaper"),
        ("dyn-source", CtlVal::Str(s)) => {
            let origin = match s.as_str() {
                "wallpaper" => Origin::Wallpaper,
                // start from what is on screen, so the field is never empty
                "file" => match sel.dynamic.as_ref().and_then(|d| d.image.clone()).or_else(|| wall.as_ref().map(|w| w.to_string_lossy().into_owned())) {
                    Some(p) => Origin::File(p),
                    None => return Err("No wallpaper to start from; type the path of an image".into()),
                },
                "color" => Origin::Color(sel.dynamic.as_ref().map(|d| d.colors.seed).unwrap_or(Rgb(0x0a, 0x84, 0xff))),
                _ => return Ok(None),
            };
            set(&|d| d.source = origin.clone())
        }
        ("dyn-value", CtlVal::Str(s)) => {
            let origin = match sel.dynamic.as_ref().map(|d| &d.source) {
                Some(Origin::Color(_)) => Origin::Color(Rgb::parse(&s).ok_or_else(|| format!("`{s}` is not a colour; use #rrggbb"))?),
                _ => Origin::File(s.trim().to_string()),
            };
            set(&|d| d.source = origin.clone())
        }
        ("dyn-refresh", CtlVal::Click) => dynamic::reread(sel, &paths, wall.as_deref()).map(Some),
        _ => Ok(None),
    }
}

/// The wallpaper changed (chosen in the Wallpaper pane, or noticed in rdwm's state): re-read it when "match wallpaper"
/// follows the wallpaper. Does nothing while the feature is off.
pub fn follow(m: &mut Model, path: &str) {
    // the newest accepted selection (a theme change may still be saving)
    let sel = m.theme_selection();
    if !sel.dynamic.as_ref().is_some_and(|d| d.enabled && d.source == Origin::Wallpaper) {
        return;
    }
    let p = super::panes::expand_tilde(path);
    if let Ok(new) = dynamic::reread(&sel, &m.paths, Some(Path::new(&p))) {
        if new != sel {
            super::themepane::commit(m, new);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;
    use crate::theme::material::Variant;

    fn selection(m: &Model) -> Selection {
        super::super::themepane::source(m).selection()
    }

    fn built(m: &Model) -> Theme {
        let src = super::super::themepane::source(m);
        src.build(&src.selection(), true).0
    }

    fn png(name: &str, rgb: [u8; 3]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("decebal-wc-{name}-{}.png", std::process::id()));
        image::RgbImage::from_pixel(32, 20, image::Rgb(rgb)).save(&p).unwrap();
        p
    }

    fn section(m: &Model) -> Vec<Item> {
        let items = super::super::themepane::items(m);
        let start = items.iter().position(|i| matches!(i, Item::Header(h) if h == "Match wallpaper")).expect("section present");
        let end = items[start + 1..].iter().position(|i| matches!(i, Item::Header(_))).map_or(items.len(), |e| start + 1 + e);
        items[start..end].to_vec()
    }

    fn ctl_ids(items: &[Item]) -> Vec<String> {
        items.iter().filter_map(|i| if let Item::Card(rows) = i { Some(rows) } else { None }).flatten().flat_map(|r| r.ctls.iter().map(|c| c.id.clone())).collect()
    }

    #[test]
    fn off_shows_only_the_switch_and_changes_nothing() {
        let (m, _) = model_with("[general]\n");
        let s = section(&m);
        assert_eq!(ctl_ids(&s), vec!["th:dynamic"]);
        assert!(!s.iter().any(|i| matches!(i, Item::Palette(_))));
        assert!(selection(&m).dynamic.is_none(), "default: off, nothing stored");
    }

    #[test]
    fn switching_on_works_with_any_theme_and_shows_the_controls_and_a_palette() {
        let (mut m, _) = model_with("[general]\n");
        let img = png("on", [30, 150, 90]);
        std::fs::write(&m.paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", img.display())).unwrap();
        m.store = crate::settings::io::Store::load(&m.paths.rdwm_config, &m.paths.rdwmctl);
        m.apply("l:theme:windows11", CtlVal::Click);
        let plain = built(&m);
        m.apply("th:dynamic", CtlVal::Bool(true));
        let s = selection(&m);
        assert_eq!(s.name, "windows11", "the theme is not switched");
        let d = s.dynamic.clone().expect("on");
        assert!(d.enabled && d.surfaces == Surfaces::Tint);
        assert_ne!(built(&m).accent, plain.accent);
        let ids = ctl_ids(&section(&m));
        for want in ["th:dynamic", "th:dyn-scheme", "th:dyn-surfaces", "th:dyn-strength", "th:dyn-contrast", "th:dyn-mode", "th:dyn-source", "th:dyn-refresh"] {
            assert!(ids.contains(&want.to_string()), "{want} in {ids:?}");
        }
        let Some(Item::Palette(chips)) = section(&m).into_iter().find(|i| matches!(i, Item::Palette(_))) else { panic!("no palette strip") };
        assert!(chips.len() >= 8 && chips[0].name == "Seed");
        // accent-only hides the strength slider
        m.apply("th:dyn-surfaces", CtlVal::Str("accent".into()));
        assert!(!ctl_ids(&section(&m)).contains(&"th:dyn-strength".to_string()));
        m.apply("th:dynamic", CtlVal::Bool(false));
        assert!(!selection(&m).dynamic.unwrap().enabled);
        assert_eq!(ctl_ids(&section(&m)), vec!["th:dynamic"]);
        let _ = std::fs::remove_file(img);
    }

    #[test]
    fn every_control_changes_the_stored_setting() {
        let (mut m, _) = model_with("[general]\n");
        let img = png("ctl", [200, 60, 40]);
        std::fs::write(&m.paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", img.display())).unwrap();
        m.store = crate::settings::io::Store::load(&m.paths.rdwm_config, &m.paths.rdwmctl);
        m.apply("th:dynamic", CtlVal::Bool(true));
        m.apply("th:dyn-scheme", CtlVal::Str("palette".into()));
        m.apply("th:dyn-surfaces", CtlVal::Str("full".into()));
        m.apply("th:dyn-strength", CtlVal::Num(80.0));
        m.apply("th:dyn-contrast", CtlVal::Num(-40.0));
        m.apply("th:dyn-mode", CtlVal::Str("wallpaper".into()));
        let d = selection(&m).dynamic.unwrap();
        assert_eq!((d.scheme, d.surfaces, d.follow_mode), (Variant::Palette, Surfaces::Full, true));
        assert!((d.strength - 0.8).abs() < 1e-3 && (d.contrast + 0.4).abs() < 1e-3);
        m.apply("th:dyn-scheme", CtlVal::Str("nonsense".into()));
        assert_eq!(selection(&m).dynamic.unwrap().scheme, Variant::Palette, "unknown scheme ignored");
        // source: file, colour, back to the wallpaper
        m.apply("th:dyn-source", CtlVal::Str("file".into()));
        assert_eq!(selection(&m).dynamic.unwrap().source, Origin::File(img.to_string_lossy().into_owned()));
        let other = png("ctl2", [20, 60, 220]);
        m.apply("th:dyn-value", CtlVal::Str(other.to_string_lossy().into_owned()));
        let d = selection(&m).dynamic.unwrap();
        assert_eq!(d.image.as_deref(), other.to_str());
        assert!((200.0..300.0).contains(&dynamic::hue_of(d.colors.seed)), "blue picture, got {}", d.colors.seed.hex());
        m.apply("th:dyn-value", CtlVal::Str("/no/such/file.png".into()));
        assert!(m.notice.as_ref().unwrap().error, "a missing image is refused");
        assert_eq!(selection(&m).dynamic.unwrap().image.as_deref(), other.to_str(), "and changes nothing");
        m.apply("th:dyn-source", CtlVal::Str("color".into()));
        m.apply("th:dyn-value", CtlVal::Str("#ff8800".into()));
        assert_eq!(selection(&m).dynamic.unwrap().colors.seed, Rgb(0xff, 0x88, 0x00));
        m.apply("th:dyn-value", CtlVal::Str("orange".into()));
        assert!(m.notice.as_ref().unwrap().error);
        m.apply("th:dyn-source", CtlVal::Str("wallpaper".into()));
        assert_eq!(selection(&m).dynamic.unwrap().source, Origin::Wallpaper);
        m.apply("th:dyn-strength", CtlVal::Reset);
        assert!((selection(&m).dynamic.unwrap().strength - 0.5).abs() < 1e-3);
        let _ = (std::fs::remove_file(img), std::fs::remove_file(other));
    }

    #[test]
    fn a_new_wallpaper_chosen_in_the_wallpaper_pane_recolours_only_while_on() {
        let (mut m, _) = model_with("[general]\n");
        let (a, b) = (png("wa", [30, 150, 90]), png("wb", [220, 40, 40]));
        follow(&mut m, b.to_str().unwrap());
        assert!(selection(&m).dynamic.is_none(), "off means off: nothing stored, nothing decoded");
        std::fs::write(&m.paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", a.display())).unwrap();
        m.store = crate::settings::io::Store::load(&m.paths.rdwm_config, &m.paths.rdwmctl);
        m.apply("th:dynamic", CtlVal::Bool(true));
        let before = selection(&m).dynamic.unwrap().colors.seed;
        follow(&mut m, b.to_str().unwrap());
        let after = selection(&m).dynamic.unwrap();
        assert_ne!(after.colors.seed, before);
        assert!(dynamic::hue_of(after.colors.seed) < 40.0 || dynamic::hue_of(after.colors.seed) > 340.0, "red now: {}", after.colors.seed.hex());
        assert_eq!(after.image.as_deref(), b.to_str());
        // a pinned source ignores wallpaper changes
        m.apply("th:dyn-source", CtlVal::Str("file".into()));
        let pinned = selection(&m).dynamic.unwrap().colors;
        follow(&mut m, a.to_str().unwrap());
        assert_eq!(selection(&m).dynamic.unwrap().colors, pinned);
        // a Wallpaper-pane hint appears while following
        m.apply("th:dyn-source", CtlVal::Str("wallpaper".into()));
        assert!(wallpaper_pane_note(&m).is_some());
        m.apply("th:dynamic", CtlVal::Bool(false));
        assert!(wallpaper_pane_note(&m).is_none());
        follow(&mut m, a.to_str().unwrap());
        assert!(!selection(&m).dynamic.unwrap().enabled);
        let _ = (std::fs::remove_file(a), std::fs::remove_file(b));
    }

    #[test]
    fn refresh_now_reads_the_wallpaper_again() {
        let (mut m, _) = model_with("[general]\n");
        let img = png("rf", [30, 150, 90]);
        std::fs::write(&m.paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", img.display())).unwrap();
        m.store = crate::settings::io::Store::load(&m.paths.rdwm_config, &m.paths.rdwmctl);
        m.apply("th:dynamic", CtlVal::Bool(true));
        let h1 = dynamic::hue_of(selection(&m).dynamic.unwrap().colors.seed);
        image::RgbImage::from_pixel(33, 20, image::Rgb([40, 60, 220])).save(&img).unwrap();
        m.apply("th:dyn-refresh", CtlVal::Click);
        let h2 = dynamic::hue_of(selection(&m).dynamic.unwrap().colors.seed);
        assert!((h1 - h2).abs() > 60.0, "{h1} -> {h2}");
        let _ = std::fs::remove_file(img);
    }

    #[test]
    fn grey_wallpapers_say_so() {
        let (mut m, _) = model_with("[general]\n");
        let img = png("grey", [120, 120, 120]);
        std::fs::write(&m.paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", img.display())).unwrap();
        m.store = crate::settings::io::Store::load(&m.paths.rdwm_config, &m.paths.rdwmctl);
        m.apply("th:dynamic", CtlVal::Bool(true));
        assert!(section(&m).iter().any(|i| matches!(i, Item::Note { text, warn: true } if text.contains("no colour"))));
        let _ = std::fs::remove_file(img);
    }
}
