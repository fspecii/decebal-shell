//! The Icons pane's asset pack rows: the cursor theme (independent of the icon theme), the current theme's
//! suggestion with a button to follow it, and the packs decebal installed with their licence and pinned source.

use super::policy::{self, Choice, Owner};
use super::{cursor, install, Provides};
use crate::settings::model::{CtlVal, Model};
use crate::settings::ui::{Ctl, Item, Kind, Row};

fn owner_detail(o: &Owner) -> &'static str {
    match o {
        Owner::User => "your choice: kept when you change themes",
        Owner::Theme(_) => "suggested by the theme: follows the next theme",
        Owner::Default => "not chosen: the toolkit default",
    }
}

pub fn items(m: &Model) -> Vec<Item> {
    let path = m.paths.shell_json();
    let doc = crate::settings::io::json_load(&path);
    let root = super::default_root();
    let roots = super::roots_with(&root);
    let mut out = Vec::new();

    out.push(Item::Header("Cursor theme".into()));
    let cur = policy::current(&doc, Provides::Cursors);
    let themes = cursor::list_themes(&roots);
    let mut opts: Vec<(String, String)> = vec![("Not set".into(), "auto".into())];
    opts.extend(themes.iter().map(|(id, _)| (id.clone(), id.clone())));
    if let Some(c) = &cur {
        if !opts.iter().any(|(_, v)| v == c) {
            opts.push((format!("{c} (not installed)"), c.clone()));
        }
    }
    let sel = opts.iter().position(|(_, v)| Some(v.as_str()) == cur.as_deref().or(Some("auto")));
    let mut rows = vec![Row::new("Cursor theme")
        .detail(format!("The mouse pointer for apps (GTK, X11) through Apply to all apps; {}", owner_detail(&policy::owner(&doc, Provides::Cursors))))
        .ctl(Ctl::new("ic:cursor", Kind::Popup(opts, sel)))];
    let rec = policy::Recommendation::from_doc(&doc);
    if let Some(r) = &rec {
        let follows = [Provides::Icons, Provides::Cursors].iter().all(|k| r.get(*k).is_none() || policy::current(&doc, *k).as_deref() == r.get(*k));
        let mut c = Ctl::new("ic:follow-theme", Kind::Button { label: "Use the theme's suggestion".into(), danger: false });
        c.enabled = !follows && (r.icons.is_some() || r.cursors.is_some());
        rows.push(Row::new(format!("Suggested by {}", r.theme)).detail(format!("Icons {}, cursors {}", r.icons.as_deref().unwrap_or("-"), r.cursors.as_deref().unwrap_or("-"))).ctl(c));
    }
    out.push(Item::Card(rows));
    if let Some(r) = &rec {
        for miss in &r.missing {
            out.push(Item::Note {
                text: format!("The theme suggests the asset pack {miss}, which is not installed: decthemectl assets install {miss} (it downloads a pinned, verified upstream copy with its licence)."),
                warn: false,
            });
        }
    }

    let packs = install::installed(&root);
    if !packs.is_empty() {
        out.push(Item::Header("Installed asset packs".into()));
        out.push(Item::Card(
            packs
                .iter()
                .map(|(_, p)| {
                    let commit = p.source.commit.as_deref().map_or("?".to_string(), |c| c[..12.min(c.len())].to_string());
                    Row::new(format!("{} ({})", p.name, p.install_name)).detail(format!(
                        "{} - {} - {} @ {}{}",
                        p.provides.join(" + "),
                        p.license,
                        if p.source.repo.is_empty() { "local" } else { &p.source.repo },
                        commit,
                        if p.verified.tree || p.verified.archive_sha256 { ", verified" } else { ", unpinned" }
                    ))
                })
                .collect(),
        ));
    }
    out
}

/// `ic:cursor` and `ic:follow-theme`; `true` when handled.
pub fn apply(m: &mut Model, parts: &[&str], v: &CtlVal) -> bool {
    let path = m.paths.shell_json();
    let res = match (parts, v) {
        (["ic", "cursor"], CtlVal::Str(s)) => {
            let c = if s == "auto" { Choice::Auto } else { Choice::Name(s.clone()) };
            policy::choose_in_file(&path, Provides::Cursors, &c).map(|_| format!("Cursor theme: {}", if s == "auto" { "not set" } else { s }))
        }
        (["ic", "cursor"], CtlVal::Reset) => policy::choose_in_file(&path, Provides::Cursors, &Choice::Auto).map(|_| "Cursor theme: not set".to_string()),
        (["ic", "follow-theme"], CtlVal::Click) => policy::choose_in_file(&path, Provides::Icons, &Choice::Theme)
            .and_then(|_| policy::choose_in_file(&path, Provides::Cursors, &Choice::Theme))
            .map(|_| "Icons and cursor follow the theme".to_string()),
        _ => return false,
    };
    match res {
        Ok(msg) => {
            m.dirty = true;
            m.say(msg, false);
        }
        Err(e) => m.say(format!("Not saved: {e}"), true),
    }
    true
}
