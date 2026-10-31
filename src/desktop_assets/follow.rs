//! Applying a theme applies its matching packs: when a slot that follows the theme needs a pack that is not installed,
//! [`request`] starts one bounded background job, `decthemectl assets follow-theme --generation G --job TOKEN` (fixed
//! arguments, no shell, its own process group, output to a private per-job log). The job installs each missing pack
//! with the verified installer (pins, licences, nothing from the pack is run) and then applies them in one transaction
//! on `decebal.json`, **only if nothing changed meanwhile**: the same `asset_packs.generation`, the same selected
//! theme, the slot still following it with the value it had (`policy::complete`). A later theme or a manual choice
//! therefore always wins over a completion that arrives after it.
//!
//! Lifecycle, per configuration (`job_dir`: one folder per canonical `decebal.json` path, 0700):
//!
//! * `lock` (flock) serialises every claim, status update and stop of that configuration.
//! * **Claim**: [`request`] takes the lock, and while it holds it decides (a live job of the same generation, or one
//!   still inside its startup grace, is reused), writes a new claim with a unique job token, starts the worker and
//!   records the worker's PID and start time. A concurrent caller waits for the lock and then sees that claim: one
//!   worker per configuration and generation.
//! * **Compare-and-set**: every progress, terminal, watchdog and stop write goes through [`update`], which changes the
//!   status only while it still carries the writer's token. A stale worker, its watchdog or an old stop can never
//!   overwrite the status of the current job. Each job has its own log, `job-<token>.log`.
//! * **Stop** targets only the current job: its process is pinned with a pidfd, its start time checked against the
//!   pinned process, and SIGTERM sent through the pidfd to the worker itself (the installer runs in the worker and stops
//!   cooperatively, `super::cancel_on_sigterm`). Nothing is ever signalled by a bare PID or process group from a status.

use super::policy::{self, Recommendation};
use super::{catalog, install, Limits, Provides};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The whole job (all its downloads and the apply) is cancelled after this; ended a minute later if it does not stop.
pub const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30 * 60);
pub const DEADLINE_GRACE: std::time::Duration = std::time::Duration::from_secs(60);
/// A claim whose worker was never recorded (the requester died between claim and spawn) is stale after this.
pub const STARTUP_GRACE_SECS: u64 = 30;
/// Per-job logs kept in a configuration's folder.
const KEEP_LOGS: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// The job's unique token: only a writer holding it may change this status.
    #[serde(default)]
    pub token: String,
    pub generation: u64,
    pub theme: String,
    /// Pack ids to install, in order.
    pub packs: Vec<String>,
    /// `starting`, `installing`, `stopping` (non-terminal); `applied`, `superseded`, `failed`, `stopped`.
    pub phase: String,
    pub detail: String,
    pub pid: u32,
    /// The worker's start time (clock ticks since boot, /proc/PID/stat field 22): with the PID, its identity.
    pub start: u64,
    /// When the claim was made (seconds since the epoch).
    #[serde(default)]
    pub claimed_at: u64,
    pub log: String,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Status {
    pub fn terminal(&self) -> bool {
        !matches!(self.phase.as_str(), "starting" | "installing" | "stopping")
    }

    /// The current job is still at work: its worker is alive, or (not recorded yet) its claim is recent.
    pub fn running(&self) -> bool {
        if self.terminal() {
            return false;
        }
        if self.pid == 0 {
            return self.phase == "starting" && now().saturating_sub(self.claimed_at) < STARTUP_GRACE_SECS;
        }
        alive(self.pid, self.start)
    }

    /// The phase to show: a non-terminal job whose worker is gone was interrupted (killed, crashed, machine restarted).
    pub fn shown_phase(&self) -> &str {
        if !self.terminal() && !self.running() {
            "interrupted"
        } else {
            &self.phase
        }
    }
}

/// `$DECEBAL_ASSET_STATE` or `$XDG_STATE_HOME/decebal/assets`.
pub fn state_dir() -> PathBuf {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    var("DECEBAL_ASSET_STATE").unwrap_or_else(|| var("XDG_STATE_HOME").unwrap_or_else(|| var("HOME").unwrap_or_else(|| "/nonexistent".into()).join(".local/state")).join("decebal/assets"))
}

/// The folder of one configuration's jobs: `state_dir()/<first 16 hex of sha256(canonical decebal.json path)>`.
pub fn job_dir(shell_json: &Path) -> PathBuf {
    job_dir_in(&state_dir(), shell_json)
}

pub fn job_dir_in(base: &Path, shell_json: &Path) -> PathBuf {
    use crate::theme::engine::sha256::{self, Sha256};
    // the file may not exist yet: its folder is canonical, its name is kept
    let canon = std::fs::canonicalize(shell_json).ok().or_else(|| {
        let parent = shell_json.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        Some(std::fs::canonicalize(parent).ok()?.join(shell_json.file_name()?))
    });
    let p = canon.unwrap_or_else(|| shell_json.to_path_buf());
    let mut h = Sha256::default();
    h.update(std::os::unix::ffi::OsStrExt::as_bytes(p.as_os_str()));
    base.join(&sha256::hex(&h.finish())[..16])
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

/// The configuration's lock, held until dropped.
struct Lock(#[allow(dead_code)] std::fs::File);

fn lock(dir: &Path) -> Result<Lock, String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    ensure_dir(dir)?;
    let p = dir.join("lock");
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&p)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    // holders are short (a claim with its spawn, one status write, one stop): waiting is bounded by them
    loop {
        // SAFETY: flock on a descriptor we own.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(Lock(f));
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("{}: {e}", p.display()));
        }
    }
}

pub fn status_at(dir: &Path) -> Option<Status> {
    let s = std::fs::read_to_string(dir.join("follow.json")).ok()?;
    serde_json::from_str(&s).ok()
}

/// The current job of the configuration at `shell_json`.
pub fn status(shell_json: &Path) -> Option<Status> {
    status_at(&job_dir(shell_json))
}

/// Written whole and renamed into place (the caller holds the lock): a reader never sees half a status.
fn write_locked(dir: &Path, st: &Status) -> Result<(), String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let tmp = dir.join(format!(".follow.json.{}.{nanos}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(st).unwrap_or_default()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join("follow.json")).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Changes the status with `f` only while `token` is the current job's; `false` when another job owns it (nothing
/// written). Every write of a worker, its watchdog and a stop goes through here.
pub fn update(dir: &Path, token: &str, f: impl FnOnce(&mut Status)) -> Result<bool, String> {
    let _l = lock(dir)?;
    let Some(mut st) = status_at(dir).filter(|s| !token.is_empty() && s.token == token) else { return Ok(false) };
    f(&mut st);
    write_locked(dir, &st)?;
    Ok(true)
}

/// Field 22 of /proc/PID/stat (after the parenthesised command name, which may itself contain spaces).
fn start_ticks(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &s[s.rfind(')')? + 2..];
    rest.split(' ').nth(19)?.parse().ok()
}

fn alive(pid: u32, start: u64) -> bool {
    pid > 1 && start != 0 && start_ticks(pid) == Some(start)
}

/// A token unique to one claim: `<generation>-<pid>-<nanos>-<counter>` (digits and dashes only, a valid file name part).
fn new_token(g: u64) -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{g}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// What `--job` accepts: what [`new_token`] makes, nothing that could leave the folder as a log name.
pub fn token_ok(t: &str) -> bool {
    !t.is_empty() && t.len() <= 96 && t.bytes().all(|b| b.is_ascii_digit() || b == b'-') && !t.starts_with('-')
}

/// Removes all but the newest [`KEEP_LOGS`] `job-*.log` of the folder (the current job's log is the newest).
fn prune_logs(dir: &Path) {
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with("job-") && n.ends_with(".log")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    logs.sort();
    let n = logs.len().saturating_sub(KEEP_LOGS);
    for (_, p) in logs.into_iter().take(n) {
        let _ = std::fs::remove_file(p);
    }
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

/// Starts a worker with its log as output; returns its PID and start time (tests pass another).
pub trait Spawner {
    fn spawn(&self, token: &str, generation: u64, log: std::fs::File) -> Result<(u32, u64), String>;
}

/// The real worker: `decthemectl assets follow-theme --generation G --job TOKEN`, detached from the caller (it must
/// outlive a `decthemectl theme apply`) in its own process group, reaped by a thread when the caller lives on.
pub struct Worker;

fn program() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("decthemectl"))).filter(|p| p.is_file()).unwrap_or_else(|| "decthemectl".into())
}

impl Spawner for Worker {
    fn spawn(&self, token: &str, generation: u64, log: std::fs::File) -> Result<(u32, u64), String> {
        use std::os::unix::process::CommandExt;
        if cfg!(test) {
            return Err("no workers are started in unit tests".into());
        }
        let log2 = log.try_clone().map_err(|e| e.to_string())?;
        let mut child = std::process::Command::new(program())
            .args(["assets", "follow-theme", "--generation", &generation.to_string(), "--job", token])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(log))
            .stderr(std::process::Stdio::from(log2))
            .process_group(0)
            .spawn()
            .map_err(|e| format!("could not start decthemectl: {e}"))?;
        let pid = child.id();
        // a worker that already exited has no start time: recorded as 0, it is not running (the next request retries)
        let start = start_ticks(pid).unwrap_or(0);
        std::thread::Builder::new()
            .name("asset-follow-reap".into())
            .spawn(move || {
                let _ = child.wait();
            })
            .map_err(|e| format!("could not watch the worker: {e}"))?;
        Ok((pid, start))
    }
}

/// Called after a theme apply (and after "Follow theme" in Icons) has written `decebal.json`: starts the background
/// job when a following slot needs a pack that is missing and downloads are automatic. Returns a note for the report.
pub fn request(shell_json: &Path) -> Option<String> {
    request_in(shell_json, &job_dir(shell_json), &Worker)
}

pub fn request_in(shell_json: &Path, dir: &Path, spawner: &dyn Spawner) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let doc = crate::settings::io::json_load(shell_json);
    let want = wanted(&doc);
    if want.is_empty() || !policy::downloads_auto(&doc) {
        return None;
    }
    let g = policy::generation(&doc);
    let theme = Recommendation::from_doc(&doc).map(|r| r.theme).unwrap_or_default();
    let packs = want.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>();
    let l = match lock(dir) {
        Ok(l) => l,
        Err(e) => return Some(format!("The matching packs were not downloaded: {e}")),
    };
    if let Some(cur) = status_at(dir).filter(Status::running) {
        if cur.generation == g {
            return Some(format!("Downloading the matching packs for {} (already running)", cur.theme));
        }
        // an older job: its completion would be refused anyway, its download is wasted
        let _ = stop_locked(dir, &cur);
    }
    let token = new_token(g);
    let log = dir.join(format!("job-{token}.log"));
    let claim =
        Status { token: token.clone(), generation: g, theme: theme.clone(), packs: packs.clone(), phase: "starting".into(), claimed_at: now(), log: log.display().to_string(), ..Status::default() };
    let started =
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&log).map_err(|e| format!("{}: {e}", log.display())).and_then(|f| {
            // the claim is published before the worker exists; its identity is recorded before the lock is released
            write_locked(dir, &claim)?;
            hooks::before_spawn();
            spawner.spawn(&token, g, f)
        });
    let note = match started {
        Ok((pid, start)) => {
            let _ = write_locked(dir, &Status { pid, start, ..claim });
            Some(format!("Downloading and verifying {} for {theme} in the background; applied when done", packs.join(" and ")))
        }
        Err(e) => {
            let _ = write_locked(dir, &Status { phase: "failed".into(), detail: format!("could not start the download: {e}"), ..claim });
            Some(format!("The matching packs were not downloaded: {e}"))
        }
    };
    drop(l);
    prune_logs(dir);
    note
}

/// What a stop reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signalled {
    Sent,
    /// The job's process had already ended (or its PID now belongs to another process, which is left alone).
    Gone,
}

/// SIGTERM to the process `(pid, start)` through a pidfd: the pidfd pins one process, and the start time is checked
/// after it was opened, so the signal can only reach that process. No pidfd support: refused, never a bare kill.
pub fn signal_pinned(pid: u32, start: u64) -> Result<Signalled, String> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    if pid <= 1 || start == 0 {
        return Ok(Signalled::Gone);
    }
    // SAFETY: pidfd_open takes a pid and flags and returns a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) } as i32;
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ESRCH) => Ok(Signalled::Gone),
            _ => Err(format!("cannot pin process {pid} to stop it ({e}); nothing was signalled")),
        };
    }
    // SAFETY: a fresh descriptor from pidfd_open, owned once.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    if start_ticks(pid) != Some(start) {
        return Ok(Signalled::Gone);
    }
    if pid == std::process::id() {
        // a status naming the process that is stopping: never signal ourselves
        return Err("the recorded job is this process; nothing was signalled".into());
    }
    hooks::after_pin();
    // SAFETY: `fd` is a pidfd we own; a null siginfo and 0 flags are valid.
    let r = unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd.as_raw_fd(), libc::SIGTERM, std::ptr::null::<libc::siginfo_t>(), 0) };
    if r == 0 {
        return Ok(Signalled::Sent);
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => Ok(Signalled::Gone),
        _ => Err(format!("could not stop process {pid}: {e}")),
    }
}

/// Stops the job `st` (the caller holds the lock and read `st` under it); writes `stopping` / `stopped` for its token.
fn stop_locked(dir: &Path, st: &Status) -> Result<String, String> {
    let (phase, detail) = match signal_pinned(st.pid, st.start)? {
        Signalled::Sent => ("stopping", "stop requested; nothing is applied (an unfinished install removes its staging folder)".to_string()),
        Signalled::Gone => ("stopped", "stopped: the job had already ended; nothing was applied".to_string()),
    };
    write_locked(dir, &Status { phase: phase.into(), detail, ..st.clone() })?;
    Ok(format!("stopped the download for {}", st.theme))
}

/// `decthemectl assets follow-theme --stop`: stops the configuration's current job, and only that one.
pub fn stop(shell_json: &Path) -> Result<String, String> {
    stop_at(&job_dir(shell_json))
}

pub fn stop_at(dir: &Path) -> Result<String, String> {
    let _l = lock(dir)?;
    let Some(st) = status_at(dir) else { return Ok("no background download".into()) };
    if !st.running() {
        return Ok(format!("no background download running (last: {} {})", st.shown_phase(), st.detail));
    }
    stop_locked(dir, &st)
}

/// The deadline of job `token`: past `limit` it is cancelled like a stop; if it has not ended `grace` later, its status
/// becomes failed (only while it is still the current job) and `exit` ends the process.
pub fn watchdog(dir: PathBuf, token: String, limit: std::time::Duration, grace: std::time::Duration, exit: fn()) {
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        super::cancel();
        std::thread::sleep(grace);
        let _ = expire(&dir, &token, limit);
        exit();
    });
}

/// The watchdog's write: refused when another job is current.
pub fn expire(dir: &Path, token: &str, limit: std::time::Duration) -> Result<bool, String> {
    update(dir, token, |s| {
        s.phase = "failed".into();
        s.detail = format!("did not finish within {} minutes and was ended; nothing was applied", limit.as_secs().div_ceil(60));
    })
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

/// The worker of job `token`: install what generation `g` still wants, then apply under the guard. Every status write
/// is conditional on `token` still being the current job; once it is not, the worker stops working and writes nothing.
pub fn run(shell_json: &Path, set: &catalog::Set, dir: &Path, token: &str, g: u64, installer: Installer) -> Result<String, String> {
    let me = (std::process::id(), start_ticks(std::process::id()).unwrap_or(0));
    // the claim is ours: acknowledge it (the requester records our identity too; a worker started by hand is not)
    let mine = update(dir, token, |s| {
        if s.pid == 0 {
            (s.pid, s.start) = me;
        }
    })?;
    if !mine {
        return Ok(format!("superseded: job {token} is not the current job; nothing to do"));
    }
    let finish = |phase: &str, detail: String| -> Result<String, String> {
        update(dir, token, |s| {
            s.phase = phase.into();
            s.detail = detail.clone();
        })?;
        Ok(detail)
    };
    let doc = crate::settings::io::json_load(shell_json);
    let theme = Recommendation::from_doc(&doc).map(|r| r.theme).unwrap_or_default();
    if policy::generation(&doc) != g {
        return finish("superseded", format!("a later theme or choice replaced generation {g}; nothing to do"));
    }
    let want = wanted(&doc);
    let mut installed: Vec<(Provides, String)> = Vec::new();
    let mut failures = Vec::new();
    // one install per pack, also when it serves both slots (Bloom has icons and cursors)
    let mut packs: Vec<&String> = want.iter().map(|(_, p)| p).collect();
    packs.dedup();
    for (i, p) in packs.iter().enumerate() {
        let current = update(dir, token, |s| {
            s.phase = "installing".into();
            s.detail = format!("{p} ({} of {}): downloading and verifying", i + 1, packs.len());
            s.packs = packs.iter().map(|p| p.to_string()).collect();
        })?;
        if !current {
            return Ok(format!("superseded during {p}: another job is current; nothing was applied"));
        }
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
            return finish("stopped", format!("stopped during {p}; nothing was applied"));
        }
    }
    let mut applied = Vec::new();
    let mut published_note: Option<String> = None;
    if !installed.is_empty() {
        // the transaction may run again on top of a save made meanwhile: its last outcome is the one committed
        let outcome = std::cell::RefCell::new(Ok(Vec::new()));
        crate::settings::io::json_update_doc(shell_json, |doc| {
            *outcome.borrow_mut() = policy::complete(doc, g, &theme, &installed);
            Ok(())
        })?;
        match outcome.into_inner() {
            Ok(d) => applied = d,
            Err(w) => return finish("superseded", format!("installed {}, not applied: {w}", installed.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", "))),
        }
        if !applied.is_empty() && !super::publish::selection_changed(shell_json) {
            // honest: the shell uses the new icons at once; GTK / Qt apps only once app integration publishes them
            published_note = Some("; apps (GTK, Qt) get it when \"Apply to all apps\" is on".to_string());
        }
    }
    let done: Vec<String> = applied.iter().map(|(k, n)| format!("{} {n}", if *k == Provides::Icons { "icons" } else { "cursors" })).collect();
    if failures.is_empty() {
        finish("applied", if done.is_empty() { "nothing left to apply".into() } else { format!("applied {}{}", done.join(" and "), published_note.unwrap_or_default()) })
    } else {
        let msg = format!(
            "{}{} (the current icons stay; Settings > Icons can retry, or decthemectl assets install ID --archive FILE / --source-dir DIR)",
            if done.is_empty() { String::new() } else { format!("applied {}; ", done.join(" and ")) },
            failures.join("; ")
        );
        finish("failed", msg.clone())?;
        Err(msg)
    }
}

/// Test injection points (no-ops in normal builds).
mod hooks {
    #[cfg(test)]
    thread_local! {
        pub static BEFORE_SPAWN: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
        pub static AFTER_PIN: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
    }

    pub fn before_spawn() {
        #[cfg(test)]
        BEFORE_SPAWN.with(|h| {
            if let Some(f) = h.borrow().as_ref() {
                f()
            }
        });
    }

    pub fn after_pin() {
        #[cfg(test)]
        AFTER_PIN.with(|h| {
            if let Some(f) = h.borrow().as_ref() {
                f()
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_assets::testkit::Scratch;
    use std::sync::{Arc, Barrier, Mutex};

    fn set() -> catalog::Set {
        let mut s = catalog::Set::default();
        s.push(catalog::parse(catalog::tests::GOOD, "t").unwrap());
        s
    }

    /// A theme applied while its pack is missing (as `theme apply` writes it: the selection and the slots together).
    fn applied(f: &Path, theme: &str) {
        let s = set();
        crate::settings::io::json_update_doc(f, |d| {
            d["theme"] = serde_json::json!({"name": theme});
            let rec = policy::recommendation(&s, theme, None, &|_, _| false);
            policy::apply_theme(d, &rec);
            Ok(())
        })
        .unwrap();
    }

    fn me() -> (u32, u64) {
        (std::process::id(), start_ticks(std::process::id()).unwrap())
    }

    /// Records every spawn; reports the test process itself as the (live) worker.
    #[derive(Default)]
    struct Fake {
        spawns: Mutex<Vec<(String, u64)>>,
        delay: Option<std::time::Duration>,
    }

    impl Spawner for Fake {
        fn spawn(&self, token: &str, generation: u64, _log: std::fs::File) -> Result<(u32, u64), String> {
            if let Some(d) = self.delay {
                std::thread::sleep(d);
            }
            self.spawns.lock().unwrap().push((token.to_string(), generation));
            Ok(me())
        }
    }

    /// A claimed job for `g` whose worker is this test process.
    fn claimed(f: &Path, dir: &Path) -> String {
        let fake = Fake::default();
        request_in(f, dir, &fake).expect("a job is started");
        let st = status_at(dir).unwrap();
        assert_eq!((st.pid, st.start), me(), "the worker's identity is recorded with the claim");
        st.token
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
        let t = claimed(&f, &dir);
        let calls = std::cell::RefCell::new(Vec::new());
        let inst = |e: &catalog::Entry| {
            calls.borrow_mut().push(e.pack.id.clone());
            Ok(e.pack.install_name.clone())
        };
        let out = run(&f, &set(), &dir, &t, g, &inst).unwrap();
        assert!(out.contains("applied icons bloom and cursors bloom"), "{out}");
        let d = crate::settings::io::json_load(&f);
        assert_eq!((policy::current(&d, Provides::Icons).as_deref(), policy::current(&d, Provides::Cursors).as_deref()), (Some("bloom"), Some("bloom")));
        assert_eq!(policy::owner(&d, Provides::Icons), policy::Owner::Theme("deepin".into()));
        assert!(wanted(&d).is_empty(), "nothing left to install");
        assert_eq!(status_at(&dir).unwrap().phase, "applied");
        assert_eq!(calls.borrow().len(), 1, "one install for both slots (bloom provides icons and cursors)");
        // a later theme while the download runs: the completion of the older one changes nothing
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g1 = policy::generation(&crate::settings::io::json_load(&f));
        let t1 = claimed(&f, &dir);
        let later = |e: &catalog::Entry| {
            applied(&f, "nord");
            Ok(e.pack.install_name.clone())
        };
        assert!(run(&f, &set(), &dir, &t1, g1, &later).unwrap().contains("not applied: superseded"));
        assert_eq!(policy::current(&crate::settings::io::json_load(&f), Provides::Icons).as_deref(), Some("Adwaita"));
        // a manual choice while the download runs wins too
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g2 = policy::generation(&crate::settings::io::json_load(&f));
        let t2 = claimed(&f, &dir);
        let manual = |e: &catalog::Entry| {
            policy::choose_in_file(&f, Provides::Icons, &policy::Choice::Name("Papirus".into())).unwrap();
            Ok(e.pack.install_name.clone())
        };
        assert!(run(&f, &set(), &dir, &t2, g2, &manual).unwrap().contains("superseded"));
        let d = crate::settings::io::json_load(&f);
        assert_eq!((policy::current(&d, Provides::Icons).as_deref(), policy::owner(&d, Provides::Icons)), (Some("Papirus"), policy::Owner::User));
        // a failed download keeps the current icons and says what still works
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g3 = policy::generation(&crate::settings::io::json_load(&f));
        let t3 = claimed(&f, &dir);
        let e = run(&f, &set(), &dir, &t3, g3, &|_| Err("git fetch: HTTP 404".into())).unwrap_err();
        assert!(e.contains("HTTP 404") && e.contains("--source-dir"), "{e}");
        assert_eq!(policy::current(&crate::settings::io::json_load(&f), Provides::Icons).as_deref(), Some("Adwaita"));
        assert_eq!(status_at(&dir).unwrap().phase, "failed");
    }

    #[test]
    fn a_direct_theme_edit_during_the_download_is_never_given_the_old_themes_pack() {
        let s = Scratch::new("follow-edit");
        let f = s.path().join("decebal.json");
        let dir = s.path().join("state");
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g = policy::generation(&crate::settings::io::json_load(&f));
        let t = claimed(&f, &dir);
        // only the selected theme changes: no generation bump, the recommendation metadata still says deepin
        let edit = |e: &catalog::Entry| {
            crate::settings::io::json_update_doc(&f, |d| {
                d["theme"]["name"] = serde_json::json!("nord");
                Ok(())
            })
            .unwrap();
            Ok(e.pack.install_name.clone())
        };
        let out = run(&f, &set(), &dir, &t, g, &edit).unwrap();
        assert!(out.contains("not applied") && out.contains("nord"), "{out}");
        let d = crate::settings::io::json_load(&f);
        assert_eq!((policy::current(&d, Provides::Icons).as_deref(), policy::current(&d, Provides::Cursors)), (Some("Adwaita"), None));
    }

    #[test]
    fn requests_respect_custom_slots_and_the_download_opt_out() {
        let s = Scratch::new("follow-req");
        let f = s.path().join("decebal.json");
        let dir = s.path().join("state");
        let fake = Fake::default();
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        policy::choose_in_file(&f, Provides::Icons, &policy::Choice::Name("Adwaita".into())).unwrap();
        policy::choose_in_file(&f, Provides::Cursors, &policy::Choice::Auto).unwrap();
        applied(&f, "deepin");
        assert!(wanted(&crate::settings::io::json_load(&f)).is_empty(), "custom slots are never fetched for");
        assert!(request_in(&f, &dir, &fake).is_none());
        std::fs::write(&f, "{}").unwrap();
        crate::settings::io::json_update_doc(&f, |d| {
            policy::set_downloads(d, false);
            Ok(())
        })
        .unwrap();
        applied(&f, "deepin");
        assert_eq!(wanted(&crate::settings::io::json_load(&f)).len(), 2);
        assert!(request_in(&f, &dir, &fake).is_none(), "downloads = ask: offered, not started");
        assert!(fake.spawns.lock().unwrap().is_empty());
    }

    #[test]
    fn concurrent_and_delayed_requests_start_exactly_one_worker_per_generation() {
        let s = Scratch::new("follow-claim");
        let f = s.path().join("decebal.json");
        let dir = s.path().join("state");
        std::fs::write(&f, "{}").unwrap();
        applied(&f, "deepin");
        // a slow worker start: the first request holds the claim (and the lock) while it spawns
        let fake = Arc::new(Fake { delay: Some(std::time::Duration::from_millis(300)), ..Fake::default() });
        let (tx, rx) = std::sync::mpsc::channel();
        let (f1, d1, k1) = (f.clone(), dir.clone(), fake.clone());
        let first = std::thread::spawn(move || {
            hooks::BEFORE_SPAWN.with(|h| *h.borrow_mut() = Some(Box::new(move || tx.send(()).unwrap())));
            request_in(&f1, &d1, &*k1)
        });
        rx.recv().unwrap(); // the claim is published and the worker is being started
        let n = 8;
        let barrier = Arc::new(Barrier::new(n));
        let others: Vec<_> = (0..n)
            .map(|_| {
                let (f2, d2, k2, b) = (f.clone(), dir.clone(), fake.clone(), barrier.clone());
                std::thread::spawn(move || {
                    b.wait();
                    request_in(&f2, &d2, &*k2).unwrap()
                })
            })
            .collect();
        assert!(first.join().unwrap().unwrap().contains("in the background"));
        for o in others {
            assert!(o.join().unwrap().contains("already running"), "every later caller sees the claim");
        }
        let spawns = fake.spawns.lock().unwrap().clone();
        assert_eq!(spawns.len(), 1, "exactly one worker: {spawns:?}");
        let st = status_at(&dir).unwrap();
        assert_eq!((st.token.as_str(), (st.pid, st.start)), (spawns[0].0.as_str(), me()));
        assert!(std::path::Path::new(&st.log).is_file() && st.log.ends_with(&format!("job-{}.log", st.token)), "a log per job");
        // a claim whose requester died before recording a worker expires after the startup grace
        let stale = Status { pid: 0, start: 0, phase: "starting".into(), claimed_at: now() - STARTUP_GRACE_SECS - 1, ..st.clone() };
        assert!(!stale.running() && stale.shown_phase() == "interrupted");
        assert!(Status { pid: 0, start: 0, phase: "starting".into(), claimed_at: now(), ..st }.running(), "a fresh claim counts as running");
    }

    #[test]
    fn an_old_job_its_watchdog_and_an_old_stop_never_touch_the_current_job() {
        let s = Scratch::new("follow-cas");
        let f = s.path().join("decebal.json");
        let dir = s.path().join("state");
        std::fs::write(&f, r#"{"icons": {"theme": "Adwaita"}}"#).unwrap();
        applied(&f, "deepin");
        let g7 = policy::generation(&crate::settings::io::json_load(&f));
        let t7 = claimed(&f, &dir);
        // g7's install is held while a later theme claims g8 (the stop of g7 reaches no process: its "worker" is the
        // test process, so the stop is made to find it gone)
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let (f7, d7, t7c) = (f.clone(), dir.clone(), t7.clone());
        let held = std::thread::spawn(move || {
            let inst = |e: &catalog::Entry| {
                ready_tx.send(()).unwrap();
                go_rx.recv().unwrap();
                Ok(e.pack.install_name.clone())
            };
            run(&f7, &set(), &d7, &t7c, g7, &inst)
        });
        ready_rx.recv().unwrap(); // g7 is inside its install, its status says so
        assert_eq!(status_at(&dir).unwrap().phase, "installing");
        // the g8 claim: the old job is not alive any more for this test, so it is simply replaced
        update(&dir, &t7, |s| s.start = 1).unwrap();
        applied(&f, "lingmo-like");
        applied(&f, "deepin");
        let g8 = policy::generation(&crate::settings::io::json_load(&f));
        let t8 = claimed(&f, &dir);
        assert_ne!(t7, t8);
        go_tx.send(()).unwrap();
        let out = held.join().unwrap().unwrap();
        assert!(out.contains("superseded") || out.contains("not the current job"), "{out}");
        let st = status_at(&dir).unwrap();
        assert_eq!((st.token.as_str(), st.generation, st.phase.as_str(), (st.pid, st.start)), (t8.as_str(), g8, "starting", me()), "g8's status survives g7's completion");
        // g7's watchdog and any write with g7's token are refused
        assert!(!expire(&dir, &t7, DEADLINE).unwrap());
        assert!(!update(&dir, &t7, |s| s.phase = "stopped".into()).unwrap());
        assert_eq!(status_at(&dir).unwrap().token, t8);
        // the watchdog of the current job is honoured
        assert!(expire(&dir, &t8, DEADLINE).unwrap());
        assert_eq!(status_at(&dir).unwrap().phase, "failed");
    }

    #[test]
    fn stop_reaches_only_the_pinned_current_worker() {
        let s = Scratch::new("follow-stop");
        let dir = s.path().join("state");
        // a status naming a live process with another start time (a recycled PID): never signalled
        let ours = Status { token: "1-1-1-1".into(), phase: "installing".into(), pid: std::process::id(), start: 1, theme: "deepin".into(), ..Status::default() };
        {
            let _l = lock(&dir).unwrap();
            write_locked(&dir, &ours).unwrap();
        }
        assert!(!status_at(&dir).unwrap().running());
        assert!(stop_at(&dir).unwrap().contains("no background download running (last: interrupted"));
        assert_eq!(signal_pinned(std::process::id(), 1).unwrap(), Signalled::Gone, "the pinned process's start time is checked");
        // a real current worker: a child process, stopped through its pidfd
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let (pid, start) = (child.id(), start_ticks(child.id()).unwrap());
        {
            let _l = lock(&dir).unwrap();
            write_locked(&dir, &Status { pid, start, ..ours.clone() }).unwrap();
        }
        assert!(stop_at(&dir).unwrap().contains("stopped the download"));
        assert_eq!(status_at(&dir).unwrap().phase, "stopping");
        let code = child.wait().unwrap();
        assert_eq!(std::os::unix::process::ExitStatusExt::signal(&code), Some(libc::SIGTERM), "the worker itself got SIGTERM");
        // the target ends (and is reaped, so its PID may be reused) between the check and the signal: the pidfd still
        // names the ended process, so the signal reaches nobody
        let mut gone = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let (gpid, gstart) = (gone.id(), start_ticks(gone.id()).unwrap());
        hooks::AFTER_PIN.with(|h| {
            *h.borrow_mut() = Some(Box::new(move || {
                // SAFETY: SIGKILL to our own child, then reaped with waitpid (nothing else waits for it here)
                unsafe {
                    libc::kill(gpid as libc::pid_t, libc::SIGKILL);
                    libc::waitpid(gpid as libc::pid_t, std::ptr::null_mut(), 0);
                }
            }))
        });
        let r = signal_pinned(gpid, gstart);
        hooks::AFTER_PIN.with(|h| *h.borrow_mut() = None);
        assert_eq!(r, Ok(Signalled::Gone), "the ended, reaped target is not signalled again, nor anything with its PID");
        let _ = gone.wait(); // already reaped by the hook
                             // and a status naming this very process is refused outright
        assert!(signal_pinned(me().0, me().1).unwrap_err().contains("this process"));
    }

    #[test]
    fn tokens_are_unique_and_plain() {
        let a = new_token(7);
        let b = new_token(7);
        assert!(a != b && token_ok(&a) && token_ok(&b));
        let long = "1".repeat(97);
        for bad in ["", "-1", "1/../x", "1 2", "abc", long.as_str()] {
            assert!(!token_ok(bad), "{bad}");
        }
        let s = Scratch::new("follow-dir");
        let f = s.path().join("decebal.json");
        assert_eq!(job_dir_in(s.path(), &f), job_dir_in(s.path(), &s.path().join("./decebal.json")), "one folder per configuration");
        assert_ne!(job_dir_in(s.path(), &f), job_dir_in(s.path(), &s.path().join("other.json")));
    }
}
