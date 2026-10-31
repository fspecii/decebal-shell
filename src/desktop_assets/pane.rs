//! The Icons pane's asset pack rows: the cursor theme (independent of the icon theme), the current theme's
//! suggestion with a button to follow it, and the packs decebal installed with their licence and pinned source.

use super::policy::{self, Choice, Owner};
use super::{cursor, install, Provides};
use crate::settings::model::{CtlVal, Model};
use crate::settings::ui::{Ctl, Item, Kind, Row};

type Listing = (Vec<(String, std::path::PathBuf)>, Vec<(std::path::PathBuf, install::Provenance)>);
type ListingCache = Option<(Vec<crate::config::Stamp>, Listing)>;
static LISTING: std::sync::Mutex<ListingCache> = std::sync::Mutex::new(None);

/// Installed cursor themes and decebal packs, rescanned only when an icon folder changed (like the icon theme list).
fn listing(root: &std::path::Path, roots: &crate::icons::resolve::Roots) -> Listing {
    let mut stamps = roots.stamps();
    stamps.push(crate::config::Stamp::of(root));
    let mut g = LISTING.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some((s, l)) if *s == stamps => l.clone(),
        _ => {
            let l = (cursor::list_themes(roots), install::installed(root));
            *g = Some((stamps, l.clone()));
            l
        }
    }
}

/// The pack being installed in the background (at most one), and the outcome of the last install.
static JOB: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static LAST: std::sync::Mutex<Option<(String, bool, String)>> = std::sync::Mutex::new(None);
#[cfg(test)]
static PROGRAM: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
#[cfg(test)]
static CATALOGS: std::sync::Mutex<Option<super::catalog::Set>> = std::sync::Mutex::new(None);

fn catalogs() -> super::catalog::Set {
    #[cfg(test)]
    if let Some(s) = CATALOGS.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return s;
    }
    super::catalog::Set::load()
}

/// `decthemectl` next to the running program, else from `PATH`.
fn program() -> String {
    #[cfg(test)]
    if let Some(p) = PROGRAM.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return p;
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("decthemectl"))).filter(|p| p.is_file()).map_or_else(|| "decthemectl".into(), |p| p.display().to_string())
}

/// Starts `decthemectl assets install ID` (fixed arguments, no shell) on a worker thread that waits for it, records
/// the outcome and wakes the Settings loop through its queue (like its other workers), so the pane updates when the
/// install ends, without any input.
fn start_install(pack: &str, queue: crate::settings::io::Queue, wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<(), String> {
    let mut g = JOB.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return Err("another pack is being installed".into());
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let log = std::env::temp_dir().join(format!("decebal-asset-install-{}-{nanos}.log", std::process::id()));
    let out = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(program())
        .args(["assets", "install", pack])
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("could not start decthemectl: {e}"))?;
    *g = Some(pack.to_string());
    let pack = pack.to_string();
    std::thread::spawn(move || {
        let ok = child.wait().is_ok_and(|s| s.success());
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let line = text.lines().find(|l| l.starts_with("installed") || l.starts_with("already") || l.starts_with("replaced") || l.starts_with("decthemectl:")).unwrap_or("").trim().to_string();
        let _ = std::fs::remove_file(&log);
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((pack, ok, line));
        *JOB.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // the icon folder changed: the Settings loop re-reads its files and redraws
        queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(crate::settings::io::Ev::FilesChanged);
        wake();
    });
    Ok(())
}

/// Is an install running?
fn installing() -> bool {
    JOB.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

fn owner_detail(o: &Owner) -> &'static str {
    match o {
        Owner::User => "your choice: kept when you change themes",
        Owner::Theme(_) => "suggested by the theme: follows the next theme",
        Owner::Default => "not chosen: the toolkit default",
    }
}

pub fn items(m: &Model) -> Vec<Item> {
    let path = m.paths.shell_json();
    // while a change is still being saved, the shown document (with it) rather than the file without it
    let doc = if m.saving() { m.shell.clone() } else { crate::settings::io::json_load(&path) };
    let root = super::default_root();
    let roots = super::roots_with(&root);
    let mut out = Vec::new();

    out.push(Item::Header("Cursor theme".into()));
    let cur = policy::current(&doc, Provides::Cursors);
    let (themes, packs) = listing(&root, &roots);
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
    let running = installing();
    if let Some(r) = rec.as_ref().filter(|r| !r.missing.is_empty()) {
        let set = catalogs();
        let mut install_rows = Vec::new();
        for miss in &r.missing {
            match set.find(miss).filter(|e| e.source.archive_url.is_some()) {
                Some(e) => {
                    let mut c = Ctl::new(format!("ic:install:{}", e.pack.id), Kind::Button { label: "Install".into(), danger: false });
                    c.enabled = !running;
                    install_rows.push(
                        Row::new(format!("Install {}", e.pack.name))
                            .detail(format!(
                                "Suggested by {}: {} at {}, checked against its pinned tree, with its licence ({})",
                                r.theme,
                                e.source.repo,
                                &e.source.commit[..12.min(e.source.commit.len())],
                                e.license()
                            ))
                            .ctl(c),
                    );
                }
                None => out.push(Item::Note {
                    text: format!("The theme suggests the asset pack {miss}, which is not installed and has no download: decthemectl assets install {miss} --archive FILE (or --source-dir DIR)."),
                    warn: false,
                }),
            }
        }
        if !install_rows.is_empty() {
            out.push(Item::Card(install_rows));
        }
    }
    let job = JOB.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(p) = job {
        out.push(Item::Note { text: format!("Installing {p}: downloading and verifying, this can take a minute."), warn: false });
    } else if let Some((p, ok, line)) = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        out.push(Item::Note { text: if ok { format!("{p}: {line}") } else { format!("{p} was not installed: {}", line.trim_start_matches("decthemectl: ")) }, warn: !ok });
    }

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
                        if p.degraded {
                            ", DEGRADED: few icon directories"
                        } else if p.verified.complete {
                            ", verified"
                        } else if p.verified.tree || p.verified.archive_sha256 {
                            ", partly verified"
                        } else {
                            ", unpinned"
                        }
                    ))
                })
                .collect(),
        ));
    }
    out
}

/// `ic:cursor` and `ic:follow-theme`; `true` when handled.
pub fn apply(m: &mut Model, parts: &[&str], v: &CtlVal) -> bool {
    // A choice changes the value and who owns it (`asset_packs`) in one atomic write on the persistence worker.
    let choose = |m: &mut Model, slots: Vec<(Provides, Choice)>, said: String| {
        m.edit_shell_doc(
            "asset-choice",
            move |doc| {
                for (k, c) in &slots {
                    policy::choose(doc, *k, c);
                }
            },
            said,
            None,
        );
    };
    let res = match (parts, v) {
        (["ic", "cursor"], CtlVal::Str(s)) => {
            let c = if s == "auto" { Choice::Auto } else { Choice::Name(s.clone()) };
            choose(m, vec![(Provides::Cursors, c)], format!("Cursor theme: {}", if s == "auto" { "not set" } else { s }));
            return true;
        }
        (["ic", "cursor"], CtlVal::Reset) => {
            choose(m, vec![(Provides::Cursors, Choice::Auto)], "Cursor theme: not set".into());
            return true;
        }
        (["ic", "install", id], CtlVal::Click) => {
            let set = catalogs();
            match set.find(id).filter(|e| e.pack.id == *id && e.source.archive_url.is_some()) {
                None => Err(format!("no downloadable asset pack `{id}`")),
                Some(_) => start_install(id, m.queue.clone(), m.wake.clone()).map(|_| format!("Installing {id}")),
            }
        }
        (["ic", "follow-theme"], CtlVal::Click) => {
            choose(m, vec![(Provides::Icons, Choice::Theme), (Provides::Cursors, Choice::Theme)], "Icons and cursor follow the theme".into());
            return true;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;

    // the pane keeps its install job in process-wide slots: its tests take turns
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn ctl<'a>(items: &'a [Item], id: &str) -> Option<&'a Ctl> {
        items.iter().find_map(|i| if let Item::Card(rows) = i { rows.iter().flat_map(|r| r.ctls.iter()).find(|c| c.id == id) } else { None })
    }

    #[test]
    fn cursor_choice_and_following_the_theme_go_through_the_pane() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let (mut m, _) = model_with("");
        let path = m.paths.shell_json();
        let its = items(&m);
        let Some(Ctl { kind: Kind::Popup(opts, sel), .. }) = ctl(&its, "ic:cursor") else { panic!("no cursor popup") };
        assert_eq!((opts[0].1.as_str(), *sel), ("auto", Some(0)), "not set by default");
        assert!(ctl(&its, "ic:follow-theme").is_none(), "no suggestion recorded yet");

        m.apply("ic:cursor", CtlVal::Str("DMZ-White".into()));
        let doc = crate::settings::io::json_load(&path);
        assert_eq!(policy::current(&doc, Provides::Cursors).as_deref(), Some("DMZ-White"));
        assert_eq!(policy::owner(&doc, Provides::Cursors), Owner::User);
        let its = items(&m);
        let Some(Ctl { kind: Kind::Popup(opts, sel), .. }) = ctl(&its, "ic:cursor") else { panic!() };
        assert_eq!(sel.map(|i| opts[i].1.as_str()), Some("DMZ-White"), "an uninstalled choice is still shown");

        let mut doc = crate::settings::io::json_load(&path);
        policy::apply_theme(
            &mut doc,
            &policy::Recommendation { theme: "fixture-desktop".into(), icons: Some("bloom".into()), cursors: Some("bloom".into()), missing: vec!["fixture-bloom-dark".into()] },
        );
        crate::settings::io::json_set(&path, policy::KEY, doc.get(policy::KEY).cloned()).unwrap();
        crate::settings::io::json_set(&path, "icons", doc.get("icons").cloned()).unwrap();
        let its = items(&m);
        assert!(ctl(&its, "ic:follow-theme").is_some_and(|c| c.enabled), "the cursor is the user's: following the theme changes something");
        assert!(its.iter().any(|i| matches!(i, Item::Note { text, .. } if text.contains("assets install fixture-bloom-dark"))));

        m.apply("ic:follow-theme", CtlVal::Click);
        let doc = crate::settings::io::json_load(&path);
        assert_eq!((policy::current(&doc, Provides::Icons).as_deref(), policy::current(&doc, Provides::Cursors).as_deref()), (Some("bloom"), Some("bloom")));
        assert_eq!(policy::owner(&doc, Provides::Cursors), Owner::Theme("fixture-desktop".into()));
        assert!(!ctl(&items(&m), "ic:follow-theme").unwrap().enabled, "already following");

        m.apply("ic:cursor", CtlVal::Reset);
        assert_eq!(policy::current(&crate::settings::io::json_load(&path), Provides::Cursors), None);
    }

    #[test]
    fn a_suggested_pack_installs_in_the_background_with_its_outcome_shown() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        use std::os::unix::fs::PermissionsExt;
        let s = crate::desktop_assets::testkit::Scratch::new("pane-install");
        let fake = s.write(
            "fake-decthemectl",
            b"#!/bin/sh\n[ \"$1 $2 $3\" = \"assets install deepin-bloom\" ] || { echo \"decthemectl: bad args $*\"; exit 1; }\nsleep 1\necho \"installed: deepin-bloom -> /x/bloom\"\n",
        );
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut set = crate::desktop_assets::catalog::Set::default();
        set.push(crate::desktop_assets::catalog::parse(crate::desktop_assets::catalog::tests::GOOD, "t").unwrap());
        *CATALOGS.lock().unwrap() = Some(set);
        *PROGRAM.lock().unwrap() = Some(fake.display().to_string());
        let (mut m, _) = model_with("");
        let path = m.paths.shell_json();
        let mut doc = crate::settings::io::json_load(&path);
        policy::apply_theme(&mut doc, &policy::Recommendation { theme: "deepin".into(), missing: vec!["deepin-bloom".into()], ..Default::default() });
        crate::settings::io::json_set(&path, policy::KEY, doc.get(policy::KEY).cloned()).unwrap();
        assert!(ctl(&items(&m), "ic:install:deepin-bloom").is_some_and(|c| c.enabled));
        m.apply("ic:install:nope", CtlVal::Click);
        assert!(JOB.lock().unwrap().is_none(), "unknown ids start nothing");
        let woke = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w2 = woke.clone();
        m.wake = std::sync::Arc::new(move || {
            w2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        m.queue.lock().unwrap().clear();
        m.apply("ic:install:deepin-bloom", CtlVal::Click);
        assert!(installing(), "the child runs in the background");
        let its = items(&m);
        assert!(its.iter().any(|i| matches!(i, Item::Note { text, .. } if text.starts_with("Installing deepin-bloom"))));
        assert!(!ctl(&its, "ic:install:deepin-bloom").unwrap().enabled, "one install at a time");
        // nothing calls items() or sends input from here on: the worker alone must deliver the completion
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while m.queue.lock().unwrap().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(matches!(m.queue.lock().unwrap().pop_front(), Some(crate::settings::io::Ev::FilesChanged)), "a queue event for the Settings loop");
        assert_eq!(woke.load(std::sync::atomic::Ordering::SeqCst), 1, "and exactly one wake");
        assert!(!installing());
        let its = items(&m);
        assert!(its.iter().any(|i| matches!(i, Item::Note { text, warn: false } if text.contains("installed: deepin-bloom"))), "the outcome is shown");
        assert!(ctl(&its, "ic:install:deepin-bloom").unwrap().enabled);
        *PROGRAM.lock().unwrap() = None;
        *CATALOGS.lock().unwrap() = None;
    }
}
