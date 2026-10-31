//! The "Themes" section (theme packages) of Appearance > Theme: the cards of [`super::themecards`], "create from current settings",
//! "install a file" and the market stub, and the clicks on them (`l:tcard:<action>:<id>`, `th:snapname`, `th:install`).

use super::model::{CtlVal, Model};
use super::themecards::Card;
use super::ui::{Ctl, Item, Kind, Row};
use crate::theme::engine::{apply as eapply, capture, store};
use crate::theme::live::Source;
use std::sync::Mutex;

/// Seconds a "Preview" keeps the theme before putting everything back.
pub const PREVIEW_SECS: u64 = 8;

static CONFIRM: Mutex<Option<String>> = Mutex::new(None);

fn source(m: &Model) -> Source {
    Source { shell_json: m.paths.shell_json(), themes_dir: Source::discover().themes_dir }
}

fn card_of(src: &Source, i: &store::Info, current: bool, confirm: &Option<String>) -> Card {
    let mut selection = src.selection();
    selection.name = i.id.clone();
    if !current {
        selection.palette = None;
        selection.accent = None;
        selection.dynamic = None;
    }
    if selection.mode == crate::theme::Mode::Auto {
        selection.mode = if crate::theme::live::system_dark() { crate::theme::Mode::Dark } else { crate::theme::Mode::Light };
    }
    let (preview, preview_status) = if i.error.is_some() {
        (None, "Theme cannot be rendered".into())
    } else {
        match crate::theme::engine::preview::cached(src, &selection) {
            Ok(Some(path)) => (Some(path), "Native preview · sample data".into()),
            Ok(None) => (None, "Generating native preview…".into()),
            Err(_) => (None, "Native preview unavailable".into()),
        }
    };
    let mut meta: Vec<String> = Vec::new();
    for s in [i.version.as_str(), i.author.as_str(), i.license.as_str()] {
        if !s.is_empty() {
            meta.push(s.to_string());
        }
    }
    meta.push(i.origin.as_str().to_string());
    if i.has_dark && i.has_light {
        meta.push("dark + light".into());
    }
    if i.widgets > 0 {
        meta.push(format!("{} widget{}", i.widgets, if i.widgets == 1 { "" } else { "s" }));
    }
    Card {
        id: i.id.clone(),
        name: i.name.clone(),
        meta: meta.join(" · "),
        description: i.description.clone(),
        tags: i.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join("  "),
        swatches: i.swatches,
        screenshot: preview,
        preview_request: i.error.is_none().then(|| (src.clone(), selection)),
        preview_status,
        current,
        deletable: i.origin == store::Origin::User,
        error: i.error.clone(),
        confirm_delete: confirm.as_deref() == Some(i.id.as_str()),
        hash_mismatch: i.hash == store::HashState::Mismatch,
    }
}

/// The section: the cards (installed themes first, built-ins after), the create / install rows and the market stub.
pub fn items(m: &Model) -> Vec<Item> {
    let src = source(m);
    let cur = src.find(&src.selection().name).map(|f| f.0).unwrap_or_default();
    let confirm = CONFIRM.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let infos: Vec<_> = store::gallery_list(&src).into_iter().filter(|i| i.origin == store::Origin::User || !crate::theme::palette::legacy_theme(&i.id)).collect();
    let mut cards: Vec<Card> = infos.iter().filter(|i| i.origin != store::Origin::Builtin).map(|i| card_of(&src, i, i.id == cur, &confirm)).collect();
    cards.extend(infos.iter().filter(|i| i.origin == store::Origin::Builtin).map(|i| card_of(&src, i, i.id == cur, &confirm)));
    let current_note = (!cards.iter().any(|c| c.current))
        .then(|| Item::Note { text: format!("Current theme: {}. Choose a desktop design below and its colours in Palettes.", src.build(&src.selection(), true).0.name), warn: false });
    let mut out = vec![
        Item::Header("Themes".into()),
        Item::Note {
            text: "Themes define the desktop design: bar, dock, window decorations and fonts. Applying a theme restores its colour palette. Wallpaper stays independent unless Follow theme wallpaper is on in Wallpaper settings. Choose other colours in Palettes; adjust details in Customise."
                .into(),
            warn: false,
        },
        Item::ThemeCards(cards),
    ];
    if let Some(note) = current_note {
        out.insert(1, note);
    }
    out.push(Item::Card(vec![
        Row::new("Create theme from current settings…")
            .detail("Saves your colours, fonts, titlebars, gaps, wallpaper, dock and widgets as a new package. Type a name and press Enter.")
            .ctl(Ctl::new("th:snapname", Kind::Text { val: String::new(), hint: "Name of the new theme".into() })),
        Row::new("Install a theme file")
            .detail("Path of a .dtheme file or a theme folder. It is checked first: packages can only contain data.")
            .ctl(Ctl::new("th:install", Kind::Text { val: String::new(), hint: "~/Downloads/mytheme.dtheme".into() })),
        Row::new("Get more themes").detail("The theme market is coming soon: browse, install and share community themes here.").ctl({
            let mut c = Ctl::new("th:market", Kind::Button { label: "Market (coming soon)".into(), danger: false });
            c.enabled = false;
            c
        }),
    ]));
    out.push(Item::Note {
        text: format!(
            "Themes live in {} (a folder each: theme.toml plus wallpapers, widgets, screenshots). decthemectl theme help lists the commands; docs/theme-engine.md describes the format.",
            src.themes_dir.display()
        ),
        warn: false,
    });
    out
}

fn slug(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').chars().take(48).collect()
}

fn free_id(src: &Source, base: &str) -> String {
    let taken = |id: &str| src.themes_dir.join(id).exists() || src.themes_dir.join(format!("{id}.toml")).exists() || crate::theme::builtin::source(id).is_some();
    if !taken(base) {
        return base.to_string();
    }
    (2..100).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).unwrap_or_else(|| format!("{base}-x"))
}

fn downloads() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    ["Downloads", "Documents"].iter().map(|d| home.join(d)).find(|d| d.is_dir()).unwrap_or(home)
}

fn set_confirm(id: Option<&str>) {
    *CONFIRM.lock().unwrap_or_else(|e| e.into_inner()) = id.map(String::from);
}

/// Returns false for ids that are not ours.
pub fn apply(m: &mut Model, parts: &[&str], v: &CtlVal) -> bool {
    let src = source(m);
    match (parts, v) {
        (["l", "tcard", act, id], CtlVal::Click) => {
            let id = *id;
            if *act != "delete" {
                set_confirm(None);
            }
            match *act {
                "apply" => {
                    let base = m.theme_selection();
                    let mut sel = base.clone();
                    sel.name = id.to_string();
                    if super::widgetchoice::request(m, &sel, true) {
                        return true;
                    }
                    let request = crate::settings::persist::ThemeRequest::Package { name: id.to_string(), base };
                    m.apply_theme(src.clone(), m.paths.clone(), request, None, super::model::ThemeDone::Package);
                }
                "preview" => {
                    let (s2, p2, id2, wake) = (src.clone(), m.paths.clone(), id.to_string(), m.wake.clone());
                    m.say(format!("Previewing {id} for {PREVIEW_SECS} seconds…"), false);
                    std::thread::spawn(move || {
                        let cancel = std::sync::atomic::AtomicBool::new(false);
                        let _ = eapply::preview(&s2, &p2, &id2, PREVIEW_SECS, &cancel);
                        wake();
                    });
                }
                "dup" => {
                    let new = free_id(&src, &format!("{id}-copy"));
                    match store::duplicate(&src, id, &new, None) {
                        Ok(_) => m.say(format!("Created {new}; edit it in {}", src.themes_dir.join(&new).display()), false),
                        Err(e) => m.say(format!("Not duplicated: {e}"), true),
                    }
                }
                "export" => {
                    let out = downloads().join(format!("{id}.dtheme"));
                    match store::export(&src, id, &out) {
                        Ok(_) => m.say(format!("Exported to {}", out.display()), false),
                        Err(e) => m.say(format!("Not exported: {e}"), true),
                    }
                }
                "delete" => {
                    let armed = CONFIRM.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(id);
                    if armed {
                        set_confirm(None);
                        match store::uninstall(&src, id) {
                            Ok(p) => m.say(format!("Removed {}", p.display()), false),
                            Err(e) => m.say(format!("Not removed: {e}"), true),
                        }
                    } else {
                        set_confirm(Some(id));
                        m.say("Click Delete again to remove this theme", false);
                    }
                }
                _ => return false,
            }
            m.dirty = true;
            true
        }
        (["th", "snapname"], CtlVal::Str(name)) => {
            let name = name.trim();
            if name.is_empty() {
                return true;
            }
            let base = slug(name);
            if base.is_empty() {
                m.say("Pick a name with letters or digits", true);
                return true;
            }
            let id = free_id(&src, &base);
            match capture::from_current(&src, &m.paths, &id, name) {
                Ok(c) => m.say(
                    format!(
                        "Created theme {id} in {}{}",
                        c.dir.display(),
                        if c.notes.is_empty() { String::new() } else { format!(" ({} note{})", c.notes.len(), if c.notes.len() == 1 { "" } else { "s" }) }
                    ),
                    false,
                ),
                Err(e) => m.say(format!("Not created: {e}"), true),
            }
            m.dirty = true;
            true
        }
        (["th", "install"], CtlVal::Str(path)) => {
            let path = path.trim();
            if path.is_empty() {
                return true;
            }
            let full = super::panes::expand_tilde(path);
            match store::install(&src, std::path::Path::new(&full), false) {
                Ok((i, r)) => m.say(format!("Installed {}{}", i.id, if r.all_warnings().is_empty() { String::new() } else { " (with warnings, see decthemectl theme validate)".to_string() }), false),
                Err(e) => m.say(format!("Not installed: {}", e.lines().next().unwrap_or("")), true),
            }
            m.dirty = true;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;

    #[test]
    fn the_gallery_shows_desktop_designs_and_not_legacy_colour_variants() {
        let (m, _) = crate::settings::model::tests_support::model_with("[general]\n");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let cards = loop {
            let cards = items(&m).into_iter().find_map(|i| if let Item::ThemeCards(c) = i { Some(c) } else { None }).unwrap();
            if !cards.is_empty() {
                break cards;
            }
            assert!(std::time::Instant::now() < deadline, "gallery discovery timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        for id in ["macos", "windows11", "caelestia", "nothing-dot", "brutalist"] {
            assert!(cards.iter().any(|c| c.id == id), "missing design {id}");
        }
        for id in ["nord", "dracula", "tokyo-night", "graphite", "high-contrast", "caelestia-mocha", "nothing-light", "terminal-amber"] {
            assert!(!cards.iter().any(|c| c.id == id), "colour variant {id} in Themes");
        }
    }
}
