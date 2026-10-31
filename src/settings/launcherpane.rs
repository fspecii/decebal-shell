//! The Launcher pane: which launcher style opens (`decebal.json` -> `"launcher": {"style": ...}`), independent of
//! the theme. The selector shows what is saved, never the theme's style: "Use theme default" is its own choice.

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Opt, Row};
use crate::launcher::style::{self, LauncherStyle, Placement};

/// The saved choice: `None` = follow the theme (absent, `"theme"` or a key this version does not know).
fn saved(m: &Model) -> Option<LauncherStyle> {
    m.shell.get("launcher").and_then(|l| l.get("style")).and_then(|v| v.as_str()).and_then(LauncherStyle::from_key)
}

fn options() -> Vec<Opt> {
    let mut v: Vec<Opt> = vec![("Use theme default".into(), style::THEME.into())];
    v.extend(style::ALL.iter().map(|s| (s.title().to_string(), s.key().to_string())));
    v
}

fn summary(s: LauncherStyle) -> String {
    let c = s.capabilities();
    let mut parts = vec![match c.placement {
        Placement::Anchored => "opens from the panel button",
        Placement::Centered => "centred card",
        Placement::FullScreen => "full screen",
    }];
    for (on, what) in [(c.categories, "categories"), (c.favorites, "pinned apps"), (c.recent, "recently used"), (c.paged, "pages"), (c.session, "power button")] {
        if on {
            parts.push(what);
        }
    }
    format!("{} ({})", s.description(), parts.join(", "))
}

pub fn items(m: &Model) -> Vec<Item> {
    let current = saved(m);
    let sel = Some(current.map_or(0, |s| style::ALL.iter().position(|x| *x == s).map_or(0, |i| i + 1)));
    let theme = crate::theme::th().launcher_style.clone();
    let effective = LauncherStyle::resolve(current.map(LauncherStyle::key), &theme);
    let follows = if current.is_none() { "Following the theme" } else { "Your choice" };
    vec![
        Item::Header("Launcher".into()),
        Item::Card(vec![
            Row::new("Launcher style")
                .detail("The look of the app launcher the panel button, the Launchpad shortcut and hot corners open. It stays when you change theme, palette or mode.")
                .ctl(Ctl::new("ln:style", Kind::Popup(options(), sel))),
            Row::new("In use").detail(format!("{follows}: {}.", effective.title())).ctl(Ctl::new("ln:open", Kind::Button { label: "Open launcher".into(), danger: false })),
        ]),
        Item::Header("Styles".into()),
        Item::Card(style::ALL.iter().map(|s| Row::new(s.title()).detail(summary(*s))).collect()),
        Item::Note {
            text: "Every style lists the same installed apps and works with every theme, light or dark. Type to search; arrows and Tab move; Enter opens; right-click or Ctrl+P pins an app; Escape or a click outside closes.".into(),
            warn: false,
        },
    ]
}

pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    let choice = match (parts, v) {
        (["ln", "style"], CtlVal::Str(key)) => LauncherStyle::from_key(&key),
        (["ln", "style"], CtlVal::Reset) => None,
        (["ln", "open"], CtlVal::Click) => {
            crate::launch::spawn_argv(&[crate::launch::sibling_or_path("declaunch"), "open".into()]);
            return;
        }
        _ => return,
    };
    // Through the persistence worker: the other keys of the `launcher` object (and of the file) are kept.
    let mut launcher = m.shell.get("launcher").filter(|v| v.is_object()).cloned().unwrap_or_else(|| serde_json::json!({}));
    launcher["style"] = serde_json::json!(choice.map_or(style::THEME, LauncherStyle::key));
    let said = format!("Launcher style: {}", choice.map_or("theme default", LauncherStyle::title));
    m.edit_json_said("shell", "launcher", Some(launcher), Some(said));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;

    fn popup(items: &[Item]) -> (Vec<Opt>, Option<usize>) {
        items
            .iter()
            .find_map(|i| match i {
                Item::Card(rows) => rows.iter().flat_map(|r| r.ctls.iter()).find(|c| c.id == "ln:style").map(|c| match &c.kind {
                    Kind::Popup(o, s) => (o.clone(), *s),
                    _ => panic!("not a popup"),
                }),
                _ => None,
            })
            .expect("the style selector")
    }

    #[test]
    fn the_selector_shows_the_saved_choice_never_the_themes() {
        let (mut m, _) = model_with("");
        let (opts, sel) = popup(&items(&m));
        assert_eq!(opts.len(), 8);
        assert_eq!(opts[0].1, "theme");
        assert_eq!(sel, Some(0), "nothing saved: Use theme default, whatever the theme's own style is");
        m.apply("ln:style", CtlVal::Str("xfce".into()));
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(m.paths.shell_json()).unwrap()).unwrap();
        assert_eq!(doc["launcher"]["style"], "xfce");
        let (opts, sel) = popup(&items(&m));
        assert_eq!(opts[sel.unwrap()].1, "xfce");
        m.apply("ln:style", CtlVal::Reset);
        assert_eq!(popup(&items(&m)).1, Some(0));
        m.apply("ln:style", CtlVal::Str("theme".into()));
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(m.paths.shell_json()).unwrap()).unwrap();
        assert_eq!(doc["launcher"]["style"], "theme");
    }

    #[test]
    fn every_style_is_described_with_its_capabilities() {
        let (m, _) = model_with("");
        let its = items(&m);
        let rows: Vec<&Row> = its.iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().collect();
        for s in style::ALL {
            let row = rows.iter().find(|r| r.label == s.title()).unwrap_or_else(|| panic!("{s:?} listed"));
            assert!(row.detail.starts_with(s.description()));
        }
    }
}
