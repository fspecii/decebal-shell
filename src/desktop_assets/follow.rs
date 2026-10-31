//! Applying a theme applies its matching packs: when a slot that follows the theme needs a pack that is not installed,
//! [`request`] starts one bounded background job, `decthemectl assets follow-theme --generation G` (fixed arguments,
//! no shell, its own process group, output to a private log). The job installs each missing pack with the verified
//! installer (pins, licences, nothing from the pack is run) and then applies them in one transaction on
//! `decebal.json`, **only if nothing changed meanwhile**: the same `asset_packs.generation`, the same theme, the slot
//! still following it with the value it had (`policy::complete`). A later theme or a manual choice therefore always
//! wins over a completion that arrives after it. Progress, failure and the stop request go through a status file.

use super::policy::{self, Recommendation};
use super::{catalog, install, Limits, Provides};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The whole job (all its downloads and the apply) is stopped after this.
pub const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub generation: u64,
    pub theme: String,
    /// Pack ids to install, in order.
    pub packs: Vec<String>,
    /// `starting`, `installing`, `applied`, `superseded`, `failed`, `stopped`.
    pub phase: String,
    pub detail: String,
    pub pid: u32,
    /// The job process's start time (clock ticks since boot, /proc/PID/stat field 22): a recycled PID is never signalled.
    pub start: u64,
    pub log: String,
}

impl Status {
    pub fn running(&self) -> bool {
        matches!(self.phase.as_str(), "starting" | "installing") && alive(self.pid, self.start)
    }
}

/// `$DECEBAL_ASSET_STATE` or `$XDG_STATE_HOME/decebal/assets` (0700).
pub fn state_dir() -> PathBuf {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    var("DECEBAL_ASSET_STATE").unwrap_or_else(|| var("XDG_STATE_HOME").unwrap_or_else(|| var("HOME").unwrap_or_else(|| "/nonexistent".into()).join(".local/state")).join("decebal/assets"))
}

fn ensure_dir(d: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    let m = std::fs::symlink_metadata(d).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(format!("{} is not a folder", d.display()));
    }
    std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())
}

pub fn status_at(dir: &Path) -> Option<Status> {
    let s = std::fs::read_to_string(dir.join("follow.json")).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn status() -> Option<Status> {
    status_at(&state_dir())
}

/// Written whole and renamed into place: a reader never sees half a status.
pub fn write_status(dir: &Path, st: &Status) -> Result<(), String> {
    ensure_dir(dir)?;
    let tmp = dir.join(format!(".follow.json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(st).unwrap_or_default()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join("follow.json")).map_err(|e| e.to_string())
}

/// Field 22 of /proc/PID/stat (after the parenthesised command name, which may itself contain spaces).
fn start_ticks(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &s[s.rfind(')')? + 2..];
    rest.split(' ').nth(19)?.parse().ok()
}

fn alive(pid: u32, start: u64) -> bool {
    pid > 1 && start_ticks(pid) == Some(start)
}

/// The slots the current recommendation still has to install: (slot, pack id) for each slot that follows the theme
/// and whose recommended pack is missing.
pub fn wanted(doc: &serde_json::Value) -> Vec<(Provides, String)> {
    let Some(rec) = Recommendation::from_doc(doc) else { return Vec::new() };
    let mut v = Vec::new();
    for k in [Provides::Icons, Provides::Cursors] {
        let st = policy::state(doc, k);
        let Some(p) = rec.pack(k) else { continue };
        if st.is_custom() || st.legacy || rec.get(k).is_some() || !rec.missing.iter().any(|m| m == p) || st.theme.as_deref() != Some(rec.theme.as_str()) {
            continue;
        }
        v.push((k, p.to_string()));
    }
    v
}

/// Called after a theme apply (and after "Follow theme" in Icons) has written `decebal.json`: starts the background
/// job when a following slot needs a pack that is missing and downloads are automatic. Returns a note for the report.
pub fn request(shell_json: &Path) -> Option<String> {
    let doc = crate::settings::io::json_load(shell_json);
    let want = wanted(&doc);
    if want.is_empty() || !policy::downloads_auto(&doc) {
        return None;
    }
    let g = policy::generation(&doc);
    let dir = state_dir();
    // an older job for another theme is stopped: its completion would be refused anyway, its download is wasted
    if let Some(old) = status_at(&dir).filter(Status::running) {
        if old.generation != g {
            let _ = stop_at(&dir);
        } else {
            return Some(format!("Downloading the matching packs for {} (already running)", old.theme));
        }
    }
    let theme = Recommendation::from_doc(&doc).map(|r| r.theme).unwrap_or_default();
    match spawn(&dir, g) {
        Ok(()) => Some(format!("Downloading and verifying {} for {theme} in the background; applied when done", want.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>().join(" and "))),
        Err(e) => {
            let _ = write_status(&dir, &Status { generation: g, theme, phase: "failed".into(), detail: format!("could not start the download: {e}"), ..Status::default() });
            Some(format!("The matching packs were not downloaded: {e}"))
        }
    }
}

fn program() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("decthemectl"))).filter(|p| p.is_file()).unwrap_or_else(|| "decthemectl".into())
}

/// Starts the job detached from the caller (it must outlive a `decthemectl theme apply`), in its own process group,
/// output to a fresh 0600 log in the state folder. Nothing is started in unit tests.
fn spawn(dir: &Path, g: u64) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    if cfg!(test) {
        return Ok(());
    }
    ensure_dir(dir)?;
    let log = dir.join("follow.log");
    let _ = std::fs::remove_file(&log);
    let f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let f2 = f.try_clone().map_err(|e| e.to_string())?;
    write_status(dir, &Status { generation: g, phase: "starting".into(), log: log.display().to_string(), ..Status::default() })?;
    std::process::Command::new(program())
        .args(["assets", "follow-theme", "--generation", &g.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(f))
        .stderr(std::process::Stdio::from(f2))
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start decthemectl: {e}"))
}

/// `decthemectl assets follow-theme --stop`: signals the running job's process group, after checking that the recorded
/// process is still that job (same start time).
pub fn stop_at(dir: &Path) -> Result<String, String> {
    let Some(mut st) = status_at(dir) else { return Ok("no background download".into()) };
    if !st.running() {
        return Ok(format!("no background download running (last: {} {})", st.phase, st.detail));
    }
    // SAFETY: kill on a process group whose leader we just verified by PID and start time.
    let r = unsafe { libc::kill(-(st.pid as libc::pid_t), libc::SIGTERM) };
    if r != 0 {
        return Err(format!("could not stop process {}: {}", st.pid, std::io::Error::last_os_error()));
    }
    st.phase = "stopped".into();
    st.detail = "stop requested; nothing is applied (an unfinished install removes its staging folder)".into();
    write_status(dir, &st)?;
    Ok(format!("stopped the download for {}", st.theme))
}

pub fn stop() -> Result<String, String> {
    stop_at(&state_dir())
}

/// Installs one catalogue pack (the real installer; tests pass another).
pub type Installer<'a> = &'a dyn Fn(&catalog::Entry) -> Result<String, String>;

pub fn install_pack<'a>(root: &'a Path, lim: &'a Limits) -> impl Fn(&catalog::Entry) -> Result<String, String> + 'a {
    move |e: &catalog::Entry| {
        let req = install::Request::from_catalog(*e, None, root.to_path_buf())?;
        let r = install::install(&req, lim)?;
        Ok(r.provenance.install_name.clone())
    }
}

/// The job: install what generation `g` still wants, then apply under the guard. Status goes to `dir`.
pub fn run(shell_json: &Path, set: &catalog::Set, dir: &Path, g: u64, installer: Installer) -> Result<String, String> {
    let mut st = Status { generation: g, pid: std::process::id(), start: start_ticks(std::process::id()).unwrap_or(0), log: dir.join("follow.log").display().to_string(), ..Status::default() };
    let doc = crate::settings::io::json_load(shell_json);
    st.theme = Recommendation::from_doc(&doc).map(|r| r.theme).unwrap_or_default();
    let finish = |st: &mut Status, phase: &str, detail: String| {
        st.phase = phase.into();
        st.detail = detail.clone();
        let _ = write_status(dir, st);
        detail
    };
    if policy::generation(&doc) != g {
        return Ok(finish(&mut st, "superseded", format!("a later theme or choice replaced generation {g}; nothing to do")));
    }
    let want = wanted(&doc);
    st.packs = want.iter().map(|(_, p)| p.clone()).collect();
    let mut installed: Vec<(Provides, String)> = Vec::new();
    let mut failures = Vec::new();
    // one install per pack, also when it serves both slots (Bloom has icons and cursors)
    let mut packs: Vec<&String> = want.iter().map(|(_, p)| p).collect();
    packs.dedup();
    for (i, p) in packs.iter().enumerate() {
        st.phase = "installing".into();
        st.detail = format!("{p} ({} of {}): downloading and verifying", i + 1, packs.len());
        write_status(dir, &st)?;
        let Some(e) = set.find(p).filter(|e| e.pack.id == **p) else {
            failures.push(format!("{p}: not in any catalogue"));
            continue;
        };
        match installer(&e) {
            Ok(name) => installed.extend(want.iter().filter(|(_, q)| q == *p).map(|(k, _)| (*k, name.clone()))),
            Err(err) => failures.push(format!("{p}: {err}")),
        }
        if super::cancelled() {
            // a stop or a later theme: nothing is applied, and an unfinished install has removed its staging folder
            return Ok(finish(&mut st, "stopped", format!("stopped during {p}; nothing was applied")));
        }
    }
    let mut applied = Vec::new();
    let mut published_note: Option<String> = None;
    if !installed.is_empty() {
        // the transaction may run again on top of a save made meanwhile: its last outcome is the one committed
        let outcome = std::cell::RefCell::new(Ok(Vec::new()));
        crate::settings::io::json_update_doc(shell_json, |doc| {
            *outcome.borrow_mut() = policy::complete(doc, g, &st.theme, &installed);
            Ok(())
        })?;
        let why = match outcome.into_inner() {
            Ok(d) => {
                applied = d;
                None
            }
            Err(e) => Some(e),
        };
        if let Some(w) = why {
            return Ok(finish(&mut st, "superseded", format!("installed {}, not applied: {w}", installed.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", "))));
        }
        if !applied.is_empty() && !super::publish::selection_changed(shell_json) {
            // honest: the shell uses the new icons at once; GTK / Qt apps only once app integration publishes them
            published_note = Some("; apps (GTK, Qt) get it when \"Apply to all apps\" is on".to_string());
        }
    }
    let done: Vec<String> = applied.iter().map(|(k, n)| format!("{} {n}", if *k == Provides::Icons { "icons" } else { "cursors" })).collect();
    if failures.is_empty() {
        Ok(finish(&mut st, "applied", if done.is_empty() { "nothing left to apply".into() } else { format!("applied {}{}", done.join(" and "), published_note.unwrap_or_default()) }))
    } else {
        let msg = format!(
            "{}{} (the current icons stay; Settings > Icons can retry, or decthemectl assets install ID --archive FILE / --source-dir DIR)",
            if done.is_empty() { String::new() } else { format!("applied {}; ", done.join(" and ")) },
            failures.join("; ")
        );
        finish(&mut st, "failed", msg.clone());
        Err(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_assets::testkit::Scratch;

    fn set() -> catalog::Set {
        let mut s = catalog::Set::default();
        s.push(catalog::parse(catalog::tests::GOOD, "t").unwrap());
        s
    }

    /// A theme applied while its pack is missing: the slot follows it and keeps its value until the pack is installed.
    fn applied(f: &Path, theme: &str) {
        let s = set();
        crate::settings::io::json_update_doc(f, |d| {
            let rec = policy::recommendation(&s, theme, None, &|_, _| false);
            policy::apply_theme(d, &rec);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_missing_pack_is_installed_then_applied_only_while_nothing_changed() {
        let s = Scratch::new("follow");
        let f = s.path().join("decebal.json");
        let dir = s.path().join("state");
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}, "system": {"cursorTheme": "DMZ-White"}}"#).unwrap();
        applied(&f, "deepin");
        let d = crate::settings::io::json_load(&f);
        assert_eq!(policy::current(&d, Provides::Icons).as_deref(), Some("Adwaita"), "no flash while the pack is missing");
        assert_eq!(wanted(&d), vec![(Provides::Icons, "deepin-bloom".into()), (Provides::Cursors, "deepin-bloom".into())]);
        let g = policy::generation(&d);
        let calls = std::cell::RefCell::new(Vec::new());
        let inst = |e: &catalog::Entry| {
            calls.borrow_mut().push(e.pack.id.clone());
            Ok(e.pack.install_name.clone())
        };
        let out = run(&f, &set(), &dir, g, &inst).unwrap();
        assert!(out.contains("applied icons bloom and cursors bloom"), "{out}");
        let d = crate::settings::io::json_load(&f);
        assert_eq!((policy::current(&d, Provides::Icons).as_deref(), policy::current(&d, Provides::Cursors).as_deref()), (Some("bloom"), Some("bloom")));
        assert_eq!(policy::owner(&d, Provides::Icons), policy::Owner::Theme("deepin".into()));
        assert!(wanted(&d).is_empty(), "nothing left to install");
        assert_eq!(status_at(&dir).unwrap().phase, "applied");
        // a later theme while the download runs: the completion of the older one changes nothing
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g1 = policy::generation(&crate::settings::io::json_load(&f));
        let later = |e: &catalog::Entry| {
            applied(&f, "nord");
            Ok(e.pack.install_name.clone())
        };
        let out = run(&f, &set(), &dir, g1, &later).unwrap();
        assert!(out.contains("not applied: superseded"), "{out}");
        assert_eq!(policy::current(&crate::settings::io::json_load(&f), Provides::Icons).as_deref(), Some("Adwaita"));
        // a manual choice while the download runs wins too
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g2 = policy::generation(&crate::settings::io::json_load(&f));
        let manual = |e: &catalog::Entry| {
            policy::choose_in_file(&f, Provides::Icons, &policy::Choice::Name("Papirus".into())).unwrap();
            Ok(e.pack.install_name.clone())
        };
        assert!(run(&f, &set(), &dir, g2, &manual).unwrap().contains("superseded"));
        let d = crate::settings::io::json_load(&f);
        assert_eq!((policy::current(&d, Provides::Icons).as_deref(), policy::owner(&d, Provides::Icons)), (Some("Papirus"), policy::Owner::User));
        // a job started for an old generation does nothing at all
        assert!(run(&f, &set(), &dir, g2, &|_| panic!("not installed")).unwrap().contains("replaced generation"));
        // a failed download keeps the current icons and says what still works
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g3 = policy::generation(&crate::settings::io::json_load(&f));
        let e = run(&f, &set(), &dir, g3, &|_| Err("git fetch: HTTP 404".into())).unwrap_err();
        assert!(e.contains("HTTP 404") && e.contains("--source-dir"), "{e}");
        assert_eq!(policy::current(&crate::settings::io::json_load(&f), Provides::Icons).as_deref(), Some("Adwaita"));
        assert_eq!(status_at(&dir).unwrap().phase, "failed");
        assert_eq!(calls.borrow().len(), 1, "one pack for both slots (bloom provides icons and cursors) is installed once per job");
    }

    #[test]
    fn requests_respect_custom_slots_and_the_download_opt_out() {
        let s = Scratch::new("follow-req");
        let f = s.path().join("decebal.json");
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        policy::choose_in_file(&f, Provides::Icons, &policy::Choice::Name("Adwaita".into())).unwrap();
        policy::choose_in_file(&f, Provides::Cursors, &policy::Choice::Auto).unwrap();
        applied(&f, "deepin");
        assert!(wanted(&crate::settings::io::json_load(&f)).is_empty(), "custom slots are never fetched for");
        assert!(request(&f).is_none());
        std::fs::write(&f, "{}").unwrap();
        crate::settings::io::json_update_doc(&f, |d| {
            policy::set_downloads(d, false);
            Ok(())
        })
        .unwrap();
        applied(&f, "deepin");
        assert_eq!(wanted(&crate::settings::io::json_load(&f)).len(), 2);
        assert!(request(&f).is_none(), "downloads = ask: offered, not started");
    }

    #[test]
    fn stop_never_signals_a_recycled_pid() {
        let s = Scratch::new("follow-stop");
        let dir = s.path().join("state");
        // our own PID with a wrong start time: not the job, never signalled
        let st = Status { phase: "installing".into(), pid: std::process::id(), start: 1, theme: "deepin".into(), ..Status::default() };
        write_status(&dir, &st).unwrap();
        assert!(!status_at(&dir).unwrap().running());
        assert!(stop_at(&dir).unwrap().contains("no background download running"));
        // the real start time: recognised as running (the stop itself is not exercised on the test process)
        let st = Status { start: start_ticks(std::process::id()).unwrap(), ..st };
        write_status(&dir, &st).unwrap();
        assert!(status_at(&dir).unwrap().running());
    }
}
