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

/// How long a background install may take before it is stopped (a 512 MiB download on a slow link fits).
const INSTALL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// After SIGTERM, how long the install's process group gets before SIGKILL.
const STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(3);
/// Output kept from an install: its first and its last bytes; the middle is counted, not kept.
const KEEP_HEAD: usize = 16 << 10;
const KEEP_TAIL: usize = 16 << 10;

/// After SIGKILL, how long the worker waits for the exit before it reports that the process has not exited yet. A
/// process in an uninterruptible kernel wait cannot be reaped before that wait ends: the worker keeps waiting for it
/// (the job stays pending), so the deadline bounds when the stop is requested, not when the process is gone.
const REAP_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(test)]
static DEADLINE: std::sync::Mutex<Option<std::time::Duration>> = std::sync::Mutex::new(None);
#[cfg(test)]
static GRACE: std::sync::Mutex<Option<std::time::Duration>> = std::sync::Mutex::new(None);
/// Test hooks: fail the worker's thread start; panic in the worker after supervising; run before the failure log is
/// written (with the job folder); pretend the killed child has not exited within the reap grace.
#[cfg(test)]
static FAIL_SPAWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static PANIC_IN_WORKER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
type DirHook = Option<Box<dyn Fn(&std::path::Path) + Send>>;
#[cfg(test)]
static BEFORE_LOG: std::sync::Mutex<DirHook> = std::sync::Mutex::new(None);
#[cfg(test)]
static SLOW_REAP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn stop_grace() -> std::time::Duration {
    #[cfg(test)]
    if let Some(d) = *GRACE.lock().unwrap_or_else(|e| e.into_inner()) {
        return d;
    }
    STOP_GRACE
}
#[cfg(test)]
static NEXT_ID: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
/// The log folder of the last failed install (kept for the user), removed when the next install starts.
static KEPT_LOG: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

fn deadline() -> std::time::Duration {
    #[cfg(test)]
    if let Some(d) = *DEADLINE.lock().unwrap_or_else(|e| e.into_inner()) {
        return d;
    }
    INSTALL_DEADLINE
}

/// Output of the install: bounded in memory whatever the child writes, with an honest count of what was dropped.
#[derive(Default)]
struct Capture {
    head: Vec<u8>,
    tail: std::collections::VecDeque<u8>,
    total: u64,
}

impl Capture {
    fn push(&mut self, data: &[u8]) {
        self.total += data.len() as u64;
        let room = KEEP_HEAD - self.head.len();
        let (h, t) = data.split_at(room.min(data.len()));
        self.head.extend_from_slice(h);
        for &c in t {
            if self.tail.len() == KEEP_TAIL {
                self.tail.pop_front();
            }
            self.tail.push_back(c);
        }
    }

    fn dropped(&self) -> u64 {
        self.total - (self.head.len() + self.tail.len()) as u64
    }

    fn text(&self) -> String {
        let mut s = String::from_utf8_lossy(&self.head).into_owned();
        if self.dropped() > 0 {
            s.push_str(&format!("\n[... {} bytes of output not kept ...]\n", self.dropped()));
        }
        s.push_str(&String::from_utf8_lossy(&self.tail.iter().copied().collect::<Vec<u8>>()));
        s
    }
}

/// A private folder for one install's log, created fresh (never an existing path, never through a link), mode 0700.
struct JobDirCleanup(std::path::PathBuf);

impl Drop for JobDirCleanup {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir(&self.0) {
            if e.kind() != std::io::ErrorKind::NotFound {
                *KEPT_LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(self.0.clone());
            }
        }
    }
}

fn job_dir() -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // SAFETY: getuid has no failure mode and touches no memory.
    let uid = unsafe { libc::getuid() };
    let base = std::env::temp_dir();
    for _ in 0..64 {
        #[allow(unused_mut)]
        let mut id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        if let Some(n) = NEXT_ID.lock().unwrap_or_else(|e| e.into_inner()).take() {
            id = n;
        }
        let d = base.join(format!("decebal-asset-install-{uid}-{}-{id}", std::process::id()));
        // create_dir fails on anything already there, a link included: it never reuses or follows one
        match std::fs::DirBuilder::new().mode(0o700).create(&d) {
            Ok(()) => return Ok(d),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", d.display())),
        }
    }
    Err("could not create a private folder for the install log".into())
}

/// Writes the kept output to `<dir>/install.log` (created new, 0600, without following links).
fn write_log(dir: &std::path::Path, text: &str) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let p = dir.join("install.log");
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Has the child exited? Asked without reaping it (`WNOWAIT`), so its process group id stays reserved for the group
/// kill that follows.
fn exited(pid: u32) -> bool {
    // SAFETY: siginfo_t is plain data; waitid writes into it and keeps no reference.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let r = libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT);
        r == 0 && info.si_pid() == pid as libc::pid_t
    }
}

/// Signals the install's own process group (it was started with `process_group(0)`, so the group id is the child's
/// pid). Only called while the child is not reaped yet: the id cannot belong to anyone else.
fn signal_group(pid: u32, sig: libc::c_int) {
    // SAFETY: a negative pid addresses exactly this process group; never our own group, never a name.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
    }
}

/// What the supervising thread reports when it ends, however it ends (a panic included): `LAST` is set, the pending
/// job is cleared, and the Settings loop is woken through its queue, exactly once.
struct Finish {
    pack: String,
    queue: crate::settings::io::Queue,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    outcome: Option<(bool, String)>,
}

impl Drop for Finish {
    fn drop(&mut self) {
        let (ok, line) = self.outcome.take().unwrap_or((false, "the install supervisor stopped unexpectedly".into()));
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((self.pack.clone(), ok, line));
        *JOB.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // the icon folder may have changed: the Settings loop re-reads its files and redraws
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(crate::settings::io::Ev::FilesChanged);
        (self.wake)();
    }
}

/// Runs the child to its end or to the deadline, draining its output (bounded) as it goes; returns its success,
/// whether it was stopped, and the kept output. The child's process group is cleaned up before the child is reaped.
fn supervise(mut child: std::process::Child, out: std::fs::File, limit: std::time::Duration, slow_reap: &dyn Fn(u32)) -> (bool, bool, Capture) {
    use std::io::Read;
    let pid = child.id();
    let mut out = out;
    let mut cap = Capture::default();
    let start = std::time::Instant::now();
    let mut eof = false;
    let drain = |out: &mut std::fs::File, cap: &mut Capture, eof: &mut bool| {
        let mut buf = [0u8; 8192];
        // bounded per round: a child writing without pause cannot starve the exit and deadline checks
        for _ in 0..64 {
            match out.read(&mut buf) {
                Ok(0) => {
                    *eof = true;
                    return;
                }
                Ok(n) => cap.push(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return, // WouldBlock: nothing more right now
            }
        }
    };
    let mut stopped = false;
    loop {
        drain(&mut out, &mut cap, &mut eof);
        if exited(pid) {
            break;
        }
        if start.elapsed() >= limit {
            stopped = true;
            signal_group(pid, libc::SIGTERM);
            let grace = std::time::Instant::now();
            while !exited(pid) && grace.elapsed() < stop_grace() {
                drain(&mut out, &mut cap, &mut eof);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(if eof { 100 } else { 20 }));
    }
    // whatever the child left behind in its group goes too, while the child (zombie or alive) still holds the group id
    signal_group(pid, libc::SIGKILL);
    // a killed process normally exits at once; one in an uninterruptible kernel wait exits only when that wait ends.
    // Wait a bounded time, then say so (the caller keeps the job pending) and keep waiting: reaping cannot be forced
    let reap_from = std::time::Instant::now();
    #[allow(unused_mut)]
    let mut gone = exited(pid);
    while !gone && reap_from.elapsed() < REAP_GRACE {
        std::thread::sleep(std::time::Duration::from_millis(20));
        gone = exited(pid);
    }
    #[cfg(test)]
    if SLOW_REAP.load(std::sync::atomic::Ordering::SeqCst) {
        gone = false;
    }
    if !gone {
        slow_reap(pid);
    }
    let status = child.wait().ok();
    // the writers are gone: read what is left, briefly
    let tail_until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while !eof && std::time::Instant::now() < tail_until {
        drain(&mut out, &mut cap, &mut eof);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    (!stopped && status.is_some_and(|s| s.success()), stopped, cap)
}

/// The line of the output that says how it ended.
fn outcome_line(text: &str, ok: bool) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pick = if ok { lines.iter().find(|l| l.starts_with("installed") || l.starts_with("already") || l.starts_with("replaced")) } else { lines.iter().rev().find(|l| l.starts_with("decthemectl:")) };
    pick.map(|l| l.chars().take(400).collect()).unwrap_or_default()
}

/// Starts `decthemectl assets install ID` (fixed arguments, no shell, its own process group) on a worker thread and
/// supervises it there: output through a pipe into a bounded buffer, a deadline, the group cleaned up at the end, the
/// outcome recorded, and the Settings loop woken through its queue (like its other workers), without any input.
/// The calling (UI) thread only claims the job and starts the worker: it never creates the child, waits or reaps, and
/// a worker that cannot start leaves no child behind (none was created).
fn start_install(pack: &str, queue: crate::settings::io::Queue, wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<(), String> {
    let mut g = JOB.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return Err("another pack is being installed".into());
    }
    // published before the worker exists and the lock released: the worker's guard clears it on every path, a failed
    // thread start included (which drops the guard, unrun, inside `spawn`)
    *g = Some(pack.to_string());
    drop(g);
    let finish = Finish { pack: pack.to_string(), queue, wake, outcome: None };
    let (pack_s, limit) = (pack.to_string(), deadline());
    let work = move || run_install(&pack_s, limit, finish);
    #[cfg(test)]
    let spawned = if FAIL_SPAWN.load(std::sync::atomic::Ordering::SeqCst) {
        drop(work);
        Err(std::io::Error::other("injected thread start failure"))
    } else {
        std::thread::Builder::new().name("asset-install".into()).spawn(work).map(|_| ())
    };
    #[cfg(not(test))]
    let spawned = std::thread::Builder::new().name("asset-install".into()).spawn(work).map(|_| ());
    spawned.map_err(|e| {
        let msg = format!("could not start the install supervisor: {e}");
        // the guard has cleared the job and reported; make its outcome precise
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((pack.to_string(), false, msg.clone()));
        msg
    })
}

/// The worker: creates the job folder, the pipe and the child, supervises it and fills in the outcome. Every early
/// return still goes through `finish` (dropped at the end), so exactly one outcome and one wake are reported.
fn run_install(pack: &str, limit: std::time::Duration, mut finish: Finish) {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    if let Some(old) = KEPT_LOG.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = std::fs::remove_file(old.join("install.log"));
        let _ = std::fs::remove_dir(&old);
    }
    let fail = |finish: &mut Finish, msg: String| finish.outcome = Some((false, msg));
    let dir = match job_dir() {
        Ok(d) => d,
        Err(e) => return fail(&mut finish, e),
    };
    let _cleanup_on_unwind = JobDirCleanup(dir.clone());
    let cleanup = |e: String| {
        let _ = std::fs::remove_dir(&dir);
        e
    };
    // one pipe for stdout and stderr; our end is non-blocking, both ends close on exec in anything else we start
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: pipe2 fills two descriptors that we then own.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return fail(&mut finish, cleanup(format!("pipe: {}", std::io::Error::last_os_error())));
    }
    // SAFETY: fresh descriptors from pipe2, owned exactly once each.
    let (read_end, write_end) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    // SAFETY: fcntl on a descriptor we own.
    let nonblocking = unsafe {
        use std::os::fd::AsRawFd;
        let fd = read_end.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL);
        flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0
    };
    if !nonblocking {
        return fail(&mut finish, cleanup("pipe: cannot make it non-blocking".into()));
    }
    let write2 = match write_end.try_clone() {
        Ok(w) => w,
        Err(e) => return fail(&mut finish, cleanup(e.to_string())),
    };
    let child = {
        let mut cmd = std::process::Command::new(program());
        cmd.args(["assets", "install", pack]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::from(write_end)).stderr(std::process::Stdio::from(write2)).process_group(0);
        // dies with this worker thread, which outlives it in every path below
        match crate::controlcenter::sys::die_with_parent(&mut cmd).spawn() {
            Ok(c) => c,
            Err(e) => return fail(&mut finish, cleanup(format!("could not start decthemectl: {e}"))),
        }
        // `cmd` and with it our copies of the write end are dropped here: EOF comes when the child's group is gone
    };
    let out = std::fs::File::from(read_end);
    // reported while the job stays pending: the process was killed but has not exited yet
    let (queue, wake, pack_s) = (finish.queue.clone(), finish.wake.clone(), finish.pack.clone());
    let slow = move |pid: u32| {
        let msg = format!("stop requested; process {pid} was killed but has not exited yet (an uninterruptible wait?); the install stays pending until it does");
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((pack_s.clone(), false, msg));
        queue.lock().unwrap_or_else(|e| e.into_inner()).push_back(crate::settings::io::Ev::FilesChanged);
        wake();
    };
    let (ok, stopped, cap) = supervise(child, out, limit, &slow);
    #[cfg(test)]
    if PANIC_IN_WORKER.load(std::sync::atomic::Ordering::SeqCst) {
        panic!("injected worker panic");
    }
    let text = cap.text();
    let mut line = outcome_line(&text, ok);
    if stopped {
        line = format!("did not finish within {} minutes and was stopped", limit.as_secs().div_ceil(60));
    } else if line.is_empty() {
        line = if ok { "installed".into() } else { "decthemectl failed without a message".into() };
    }
    if cap.dropped() > 0 {
        line.push_str(&format!(" (output truncated: {} bytes not kept)", cap.dropped()));
    }
    if ok {
        let _ = std::fs::remove_dir(&dir);
    } else {
        #[cfg(test)]
        if let Some(h) = BEFORE_LOG.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            h(&dir);
        }
        match write_log(&dir, &text) {
            Ok(p) => {
                line.push_str(&format!("; log: {}", p.display()));
                *KEPT_LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.clone());
            }
            Err(e) => {
                line.push_str(&format!("; the log could not be written ({e})"));
                // Remove an empty owned job folder. Preserve and track a partial log
                // instead of leaving an untracked directory when publication fails.
                if let Err(cleanup) = std::fs::remove_dir(&dir) {
                    if cleanup.kind() != std::io::ErrorKind::NotFound {
                        *KEPT_LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.clone());
                        line.push_str(&format!("; diagnostic folder: {}", dir.display()));
                    }
                }
            }
        }
    }
    finish.outcome = Some((ok, line));
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

    /// Runs a fake installer through the real supervisor and waits (without input) for its completion event.
    fn run_fake(tag: &str, script: &str, limit: Option<std::time::Duration>) -> (Option<(String, bool, String)>, usize, crate::desktop_assets::testkit::Scratch) {
        use std::os::unix::fs::PermissionsExt;
        let s = crate::desktop_assets::testkit::Scratch::new(tag);
        let fake = s.write("fake-decthemectl", format!("#!/bin/sh\n{script}\n").as_bytes());
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (last, woke, _) = run_fake_program(&format!("{tag}-run"), &fake.display().to_string(), limit);
        (last, woke, s)
    }

    /// Runs `program` as the installer through the real supervisor; waits (without input) until the job is no longer
    /// pending and returns the outcome, the number of wakes and a scratch folder.
    fn run_fake_program(tag: &str, program: &str, limit: Option<std::time::Duration>) -> (Option<(String, bool, String)>, usize, crate::desktop_assets::testkit::Scratch) {
        let s = crate::desktop_assets::testkit::Scratch::new(tag);
        *PROGRAM.lock().unwrap() = Some(program.to_string());
        *DEADLINE.lock().unwrap() = limit;
        let q: crate::settings::io::Queue = std::sync::Arc::new(std::sync::Mutex::new(Default::default()));
        let woke = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w2 = woke.clone();
        *LAST.lock().unwrap() = None;
        start_install(
            "deepin-bloom",
            q.clone(),
            std::sync::Arc::new(move || {
                w2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }),
        )
        .unwrap();
        assert!(installing());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while (installing() || q.lock().unwrap().is_empty()) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(q.lock().unwrap().len(), woke.load(std::sync::atomic::Ordering::SeqCst), "one queue event per wake");
        assert!(!installing(), "the pending job is cleared");
        *PROGRAM.lock().unwrap() = None;
        *DEADLINE.lock().unwrap() = None;
        let last = LAST.lock().unwrap().clone();
        (last, woke.load(std::sync::atomic::Ordering::SeqCst), s)
    }

    fn gone(pid: i32) -> bool {
        // gone, or a zombie waiting for init: no longer running either way
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(st) if st.rsplit(')').next().is_some_and(|r| r.trim_start().starts_with('Z')) => return true,
                Ok(_) if std::time::Instant::now() > until => return false,
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
    }

    #[test]
    fn huge_output_is_kept_bounded_and_said_to_be_truncated() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let (last, woke, _s) = run_fake("pane-huge", "head -c 3000000 /dev/zero | tr '\\000' x\necho\necho 'installed: deepin-bloom -> /x/bloom'", None);
        let (pack, ok, line) = last.unwrap();
        assert_eq!((pack.as_str(), ok, woke), ("deepin-bloom", true, 1));
        assert!(line.starts_with("installed: deepin-bloom") && line.contains("output truncated"), "{line}");
        let mut c = Capture::default();
        c.push(&vec![b'x'; 3_000_000]);
        assert_eq!((c.head.len(), c.tail.len(), c.dropped()), (KEEP_HEAD, KEEP_TAIL, 3_000_000 - (KEEP_HEAD + KEEP_TAIL) as u64));
        assert!(c.text().contains("bytes of output not kept"));
    }

    #[test]
    fn a_stalled_install_is_stopped_with_its_whole_process_group() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let pids = crate::desktop_assets::testkit::Scratch::new("pane-pids");
        let pf = pids.path().join("pids");
        let script = format!("sleep 300 &\necho $! > '{0}'\necho $$ >> '{0}'\necho 'downloading...'\nsleep 300", pf.display());
        let started = std::time::Instant::now();
        let (last, woke, _s) = run_fake("pane-stall", &script, Some(std::time::Duration::from_secs(1)));
        assert!(started.elapsed() < std::time::Duration::from_secs(15), "the deadline holds");
        let (_, ok, line) = last.unwrap();
        assert!(!ok && woke == 1 && line.starts_with("did not finish within 1 minutes and was stopped"), "{line}");
        let ids: Vec<i32> = std::fs::read_to_string(&pf).unwrap().lines().map(|l| l.trim().parse().unwrap()).collect();
        assert_eq!(ids.len(), 2);
        for pid in ids {
            assert!(gone(pid), "pid {pid} of the install's group still runs");
        }
        // the kept log: a private file in a private folder, with what the install said
        let log = std::path::PathBuf::from(line.rsplit("log: ").next().unwrap());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&log).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(log.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(std::fs::read_to_string(&log).unwrap().contains("downloading..."));
    }

    #[test]
    fn a_failed_install_reports_its_message_and_keeps_its_log() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let (last, woke, _s) = run_fake("pane-fail", "echo 'reading archive'\necho 'decthemectl: tree mismatch for `bloom`' >&2\nexit 3", None);
        let (_, ok, line) = last.unwrap();
        assert!(!ok && woke == 1 && line.starts_with("decthemectl: tree mismatch for `bloom`; log: "), "{line}");
    }

    #[test]
    fn job_folders_are_created_fresh_and_never_through_an_existing_path() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let victim = crate::desktop_assets::testkit::Scratch::new("pane-victim");
        // SAFETY: getuid has no failure mode.
        let uid = unsafe { libc::getuid() };
        let id = 900_000_000 + u64::from(std::process::id());
        let planted = std::env::temp_dir().join(format!("decebal-asset-install-{uid}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_file(&planted);
        std::os::unix::fs::symlink(victim.path(), &planted).unwrap();
        *NEXT_ID.lock().unwrap() = Some(id);
        let (last, _, _s) = run_fake("pane-collide", "echo 'decthemectl: refused'\nexit 1", None);
        let line = last.unwrap().2;
        let log = std::path::PathBuf::from(line.rsplit("log: ").next().unwrap());
        assert_ne!(log.parent().unwrap(), planted.as_path(), "the planted name was skipped");
        assert_eq!(std::fs::read_dir(victim.path()).unwrap().count(), 0, "nothing was written through the link");
        assert_eq!(std::fs::read_link(&planted).unwrap(), victim.path(), "the planted link is untouched");
        std::fs::remove_file(&planted).unwrap();
        // a program that cannot start: reported as the outcome (the worker creates the child), nothing pending, no folder
        *NEXT_ID.lock().unwrap() = Some(id + 1);
        let (last, woke, _s2) = run_fake_program("pane-nostart", "/nonexistent/decthemectl", None);
        let (_, ok, line) = last.unwrap();
        assert!(!ok && woke == 1 && line.contains("could not start decthemectl"), "{line}");
        assert!(!installing());
        assert!(!std::env::temp_dir().join(format!("decebal-asset-install-{uid}-{}-{}", std::process::id(), id + 1)).exists());
    }

    #[test]
    fn a_worker_that_cannot_start_launches_no_child() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        use std::os::unix::fs::PermissionsExt;
        let s = crate::desktop_assets::testkit::Scratch::new("pane-nothread");
        let marker = s.path().join("ran");
        let fake = s.write("fake", format!("#!/bin/sh\ntouch '{}'\n", marker.display()).as_bytes());
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        *PROGRAM.lock().unwrap() = Some(fake.display().to_string());
        FAIL_SPAWN.store(true, std::sync::atomic::Ordering::SeqCst);
        let q: crate::settings::io::Queue = std::sync::Arc::new(std::sync::Mutex::new(Default::default()));
        let woke = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w2 = woke.clone();
        let started = std::time::Instant::now();
        let e = start_install(
            "deepin-bloom",
            q.clone(),
            std::sync::Arc::new(move || {
                w2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }),
        )
        .unwrap_err();
        FAIL_SPAWN.store(false, std::sync::atomic::Ordering::SeqCst);
        *PROGRAM.lock().unwrap() = None;
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "nothing waited on the calling thread");
        assert!(e.contains("could not start the install supervisor"), "{e}");
        assert!(!installing());
        assert_eq!((q.lock().unwrap().len(), woke.load(std::sync::atomic::Ordering::SeqCst)), (1, 1), "reported once");
        assert!(LAST.lock().unwrap().as_ref().is_some_and(|l| !l.1 && l.2.contains("could not start the install supervisor")));
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!marker.exists(), "no child was ever started");
    }

    #[test]
    fn a_panicking_worker_still_reports_once_and_leaves_no_process() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let pids = crate::desktop_assets::testkit::Scratch::new("pane-panic-pids");
        let pf = pids.path().join("pid");
        let id = u64::MAX - 15;
        let dir = std::env::temp_dir().join(format!("decebal-asset-install-{}-{}-{id}", unsafe { libc::getuid() }, std::process::id()));
        assert!(!dir.exists(), "the cleanup test must own a fresh path");
        *NEXT_ID.lock().unwrap() = Some(id);
        PANIC_IN_WORKER.store(true, std::sync::atomic::Ordering::SeqCst);
        let (last, woke, _s) = run_fake("pane-panic", &format!("sleep 300 &\necho $! > '{}'\nexit 1", pf.display()), None);
        PANIC_IN_WORKER.store(false, std::sync::atomic::Ordering::SeqCst);
        let (_, ok, line) = last.unwrap();
        assert!(!ok && woke == 1 && line.contains("stopped unexpectedly"), "{line}");
        let pid: i32 = std::fs::read_to_string(&pf).unwrap().trim().parse().unwrap();
        assert!(gone(pid), "the install's group was cleaned up before the panic");
        assert!(!dir.exists(), "the empty owned job directory was cleaned up during unwind");
    }

    #[test]
    fn a_log_that_cannot_be_written_is_reported_and_its_folder_handled() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        // the log cannot be created and the folder is empty: the folder is removed, nothing is tracked
        *BEFORE_LOG.lock().unwrap() = Some(Box::new(|d: &std::path::Path| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o500)).unwrap();
        }));
        let (last, _, _s) = run_fake("pane-nolog", "echo 'decthemectl: refused'\nexit 1", None);
        let line = last.unwrap().2;
        assert!(line.starts_with("decthemectl: refused; the log could not be written") && !line.contains("diagnostic folder"), "{line}");
        assert!(KEPT_LOG.lock().unwrap().is_none());
        // a partial log is already there: the folder is kept, tracked and named
        *BEFORE_LOG.lock().unwrap() = Some(Box::new(|d: &std::path::Path| std::fs::write(d.join("install.log"), "partial").unwrap()));
        let (last, _, _s) = run_fake("pane-partial", "echo 'decthemectl: refused'\nexit 1", None);
        *BEFORE_LOG.lock().unwrap() = None;
        let line = last.unwrap().2;
        let kept = KEPT_LOG.lock().unwrap().clone().expect("tracked");
        assert!(line.contains("the log could not be written") && line.contains(&format!("diagnostic folder: {}", kept.display())), "{line}");
        assert_eq!(std::fs::read_to_string(kept.join("install.log")).unwrap(), "partial", "the partial log is preserved");
    }

    #[test]
    fn a_child_that_ignores_sigterm_is_killed_after_the_grace() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let pids = crate::desktop_assets::testkit::Scratch::new("pane-term-pids");
        let pf = pids.path().join("pid");
        *GRACE.lock().unwrap() = Some(std::time::Duration::from_millis(500));
        let started = std::time::Instant::now();
        let (last, woke, _s) = run_fake("pane-noterm", &format!("trap '' TERM\necho $$ > '{}'\nwhile :; do sleep 1; done", pf.display()), Some(std::time::Duration::from_millis(500)));
        *GRACE.lock().unwrap() = None;
        let (_, ok, line) = last.unwrap();
        assert!(!ok && woke == 1 && line.starts_with("did not finish"), "{line}");
        assert!(started.elapsed() < std::time::Duration::from_secs(8), "SIGKILL after the grace: {:?}", started.elapsed());
        let pid: i32 = std::fs::read_to_string(&pf).unwrap().trim().parse().unwrap();
        assert!(gone(pid));
    }

    #[test]
    fn a_leader_that_exits_early_does_not_leave_its_descendants_holding_the_job() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let pids = crate::desktop_assets::testkit::Scratch::new("pane-early-pids");
        let pf = pids.path().join("pid");
        let started = std::time::Instant::now();
        let (last, woke, _s) = run_fake("pane-early", &format!("sleep 300 &\necho $! > '{}'\necho 'installed: deepin-bloom -> /x/bloom'\nexit 0", pf.display()), None);
        let (_, ok, line) = last.unwrap();
        assert!(ok && woke == 1 && line.starts_with("installed: deepin-bloom"), "{line}");
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "the descendant holding the pipe did not delay completion");
        let pid: i32 = std::fs::read_to_string(&pf).unwrap().trim().parse().unwrap();
        assert!(gone(pid), "the descendant was killed with the group");
    }

    #[test]
    fn a_process_that_has_not_exited_after_the_kill_keeps_the_job_pending_and_says_so() {
        let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
        use std::os::unix::fs::PermissionsExt;
        // a process in an uninterruptible wait cannot be made here; the hook makes the reap look slow instead
        let s = crate::desktop_assets::testkit::Scratch::new("pane-slowreap");
        let fake = s.write("fake", b"#!/bin/sh\necho 'installed: deepin-bloom -> /x/bloom'\n");
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        *PROGRAM.lock().unwrap() = Some(fake.display().to_string());
        SLOW_REAP.store(true, std::sync::atomic::Ordering::SeqCst);
        // what the pane would see at each wake: still pending?, and the message
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(bool, String)>::new()));
        let s2 = seen.clone();
        let q: crate::settings::io::Queue = std::sync::Arc::new(std::sync::Mutex::new(Default::default()));
        start_install(
            "deepin-bloom",
            q.clone(),
            std::sync::Arc::new(move || {
                let msg = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone().map(|l| l.2).unwrap_or_default();
                s2.lock().unwrap().push((installing(), msg));
            }),
        )
        .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while installing() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        SLOW_REAP.store(false, std::sync::atomic::Ordering::SeqCst);
        *PROGRAM.lock().unwrap() = None;
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen[0].0 && seen[0].1.contains("has not exited yet") && seen[0].1.contains("stays pending"), "first wake: still pending, said honestly: {seen:?}");
        assert!(!seen[1].0 && seen[1].1.starts_with("installed"), "second wake: finished: {seen:?}");
        assert_eq!(q.lock().unwrap().len(), 2);
    }
}
