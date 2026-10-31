//! The `decwidgets` daemon: one small surface per desktop widget, kept below every application window.
//!
//! It runs natively on both display servers and picks one with the session policy every Decebal program shares
//! ([`crate::backend::session`]): a wlr-layer-shell surface per card on Wayland ([`super::host_wayland`]), an
//! override-redirect window per card on X11 ([`super::host_x11`]). The layout, painting, live data and click actions are
//! shared; only the surfaces and their input differ.
//!
//! One host runs per display server: X11 keeps the historical `decwidgets` lock, a Wayland host is scoped to its
//! compositor socket ([`HostKey`]). A host says what it made of the configuration in a readiness record ([`Record`]) that
//! names its pid, the start time of that pid, the server and the configuration it built; a reader only trusts it while
//! that very process still holds the lock, so a host killed with SIGKILL (or a pid reused since) is never "ready".
//!
//! A host from before per-server locks (one generic `decwidgets` lock on both servers) may still run after an update. On
//! X11 it holds the same lock, so nothing changes; on Wayland the keyed host would start next to it. When that older host is
//! proven to serve this compositor ([`legacy_for`]) no keyed host is started, the state is [`HostState::Legacy`] and callers
//! say so; it is never signalled or replaced automatically.

use super::config::{self, Instance, WidgetsConfig};
use super::data::Data;
use super::dsl::WidgetDef;
use super::layout::{self, Ctx, Item};
use super::paint::FontMetrics;
use crate::backend::session;
pub use crate::backend::session::Mode;
use crate::text::Fonts;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct Paths {
    pub shell_json: PathBuf,
    pub themes_dir: PathBuf,
}

const NO_DISPLAY: &str = "No desktop display is available for widgets (a Wayland compositor with layer-shell or an X11 session is needed)";

/// The display server of the current session (see [`crate::backend::session`]); the compositor is probed for
/// wlr-layer-shell only when the policy reaches Wayland, at most once per call.
pub fn mode() -> Option<Mode> {
    session::detect(session::layer_shell_supported)
}

// ---------------------------------------------------------------- one host per server

/// What one host instance is bound to: the lock that keeps it single and the display server its surfaces are on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKey {
    pub mode: Mode,
    /// Name of the single-instance lock ([`crate::single::hold`]).
    pub lock: String,
    /// The server as the readiness record names it: `x11:<DISPLAY>`, or the path of the Wayland socket.
    pub server: String,
}

impl HostKey {
    /// The key of the current session's host on `mode`; `None` when that server's variable is unset.
    pub fn current(mode: Mode) -> Option<HostKey> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        match mode {
            Mode::X11 => Some(HostKey::x11(&var("DISPLAY")?)),
            Mode::Wayland => HostKey::wayland(&var("WAYLAND_DISPLAY")?, std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).as_deref()),
        }
    }

    /// X11 keeps the lock the host has always used, so an installed host and this one never run side by side.
    fn x11(display: &str) -> HostKey {
        HostKey { mode: Mode::X11, lock: "decwidgets".into(), server: format!("x11:{display}") }
    }

    /// A Wayland host belongs to one compositor socket: a nested compositor or a second session gets a host of its own.
    fn wayland(display: &str, runtime_dir: Option<&Path>) -> Option<HostKey> {
        let socket = if Path::new(display).is_absolute() { PathBuf::from(display) } else { runtime_dir?.join(display) };
        Some(HostKey { mode: Mode::Wayland, lock: format!("decwidgets-wl-{}", slug(display)), server: socket.to_string_lossy().into_owned() })
    }

    fn record_path(&self) -> PathBuf {
        crate::single::runtime_file(&format!("{}.state", self.lock))
    }

    /// Identity of the server endpoint right now: device and inode of the Wayland socket (a restarted compositor that
    /// reuses the name has a new one); X11 hosts are tied to their display by the selection owner instead.
    fn endpoint(&self) -> String {
        match self.mode {
            Mode::Wayland => std::fs::metadata(&self.server).map(|m| format!("{}:{}", m.dev(), m.ino())).unwrap_or_default(),
            Mode::X11 => String::new(),
        }
    }
}

/// A file-name-safe form of a display name (long or odd ones become a hash).
fn slug(s: &str) -> String {
    let out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if out.is_empty() || out.len() > 64 {
        format!("{:016x}", fnv(s.as_bytes()))
    } else {
        out
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// Cheap fingerprint of the configuration file a build used (modification time and length).
pub(super) fn config_stamp(shell_json: &Path) -> u64 {
    match std::fs::metadata(shell_json) {
        Ok(m) => fnv(format!("{}.{}:{}", m.mtime(), m.mtime_nsec(), m.len()).as_bytes()),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------- readiness

/// What a host made of the configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    /// Enabled desktop cards (0 when widgets are hidden or none is enabled: an idle host is ready on purpose).
    pub wanted: u32,
    /// Cards on screen with their first frame.
    pub shown: u32,
    /// Cards whose definition failed (or past the limit of cards a host shows).
    pub broken: u32,
    /// Cards with nowhere to go: no output yet, or the compositor closed their surface.
    pub unplaced: u32,
}

impl Tally {
    /// Why some cards are missing, for the user.
    pub fn problem(&self) -> String {
        let mut why = Vec::new();
        if self.broken > 0 {
            why.push(format!("{} could not be loaded", self.broken));
        }
        if self.unplaced > 0 {
            why.push(format!("{} found no display to go on", self.unplaced));
        }
        if why.is_empty() {
            why.push("the rest did not appear".into());
        }
        format!("{} of {} desktop widgets are shown: {}; see `decwidgets status`", self.shown, self.wanted, why.join(", "))
    }
}

/// The readiness record a host writes once a build has settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub pid: i32,
    /// Start time of `pid` (field 22 of `/proc/<pid>/stat`), so a reused pid is not mistaken for the host.
    pub start: u64,
    pub server: String,
    pub endpoint: String,
    /// [`config_stamp`] of the configuration this outcome is for.
    pub config: u64,
    pub tally: Tally,
}

const RECORD_HEADER: &str = "decwidgets-ready 1";

impl Record {
    fn encode(&self) -> String {
        let t = &self.tally;
        format!(
            "{RECORD_HEADER}\npid={}\nstart={}\nserver={}\nendpoint={}\nconfig={}\nwanted={}\nshown={}\nbroken={}\nunplaced={}\n",
            self.pid, self.start, self.server, self.endpoint, self.config, t.wanted, t.shown, t.broken, t.unplaced
        )
    }

    fn parse(text: &str) -> Option<Record> {
        let mut lines = text.lines();
        if lines.next()? != RECORD_HEADER {
            return None;
        }
        let fields: std::collections::HashMap<&str, &str> = lines.filter_map(|l| l.split_once('=')).collect();
        let num = |k: &str| fields.get(k)?.parse::<u64>().ok();
        let count = |k: &str| fields.get(k)?.parse::<u32>().ok();
        Some(Record {
            pid: fields.get("pid")?.parse().ok().filter(|p: &i32| *p > 0)?,
            start: num("start")?,
            server: fields.get("server")?.to_string(),
            endpoint: fields.get("endpoint")?.to_string(),
            config: num("config")?,
            tally: Tally { wanted: count("wanted")?, shown: count("shown")?, broken: count("broken")?, unplaced: count("unplaced")? },
        })
    }
}

/// What a reader checks a record against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Facts {
    /// Pid of the process holding the host lock now, if any.
    pub holder: Option<i32>,
    /// Start time of the record's pid now (`None`: no such process).
    pub start: Option<u64>,
    pub server: String,
    pub endpoint: String,
}

/// The record describes the live host of this server: its process still holds the lock, is the same process (not a
/// reused pid) and serves the same server endpoint.
pub(super) fn trusted(rec: &Record, facts: &Facts) -> bool {
    facts.holder == Some(rec.pid) && facts.start == Some(rec.start) && rec.server == facts.server && rec.endpoint == facts.endpoint
}

/// Where the host of a session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostState {
    /// No host holds this server's lock.
    Down,
    /// A host runs but has not settled the current configuration yet.
    Starting,
    /// Every enabled card is on screen, or none is enabled (`wanted == 0`).
    Ready(Tally),
    /// Running, but some enabled cards could not be shown.
    Degraded(Tally),
    /// A host from before per-server locks (this pid) serves this Wayland server: it shows widgets but cannot confirm a
    /// configuration, and the updated host is not started next to it until it ends (see [`legacy_for`]).
    Legacy(i32),
}

impl HostState {
    fn of_tally(t: Tally) -> HostState {
        if t.shown >= t.wanted {
            HostState::Ready(t)
        } else {
            HostState::Degraded(t)
        }
    }

    pub fn running(&self) -> bool {
        matches!(self, HostState::Ready(_) | HostState::Degraded(_) | HostState::Legacy(_))
    }
}

/// The state of a Wayland host from its lock, its record and the configuration the caller expects it to show.
pub(super) fn classify(holder: Option<i32>, rec: Option<&Record>, trusted: bool, config: Option<u64>) -> HostState {
    if holder.is_none() {
        return HostState::Down;
    }
    match rec {
        Some(r) if trusted && config.is_none_or(|c| c == r.config) => HostState::of_tally(r.tally),
        _ => HostState::Starting,
    }
}

/// The state of an X11 host: the selection owner says it is up (as it always has); a trusted record refines it.
fn classify_x11(owned: bool, holder: Option<i32>, rec: Option<&Record>, trusted: bool, config: Option<u64>) -> HostState {
    if !owned {
        return if holder.is_some() { HostState::Starting } else { HostState::Down };
    }
    match rec {
        Some(r) if trusted && config.is_some_and(|c| c != r.config) => HostState::Starting,
        Some(r) if trusted => HostState::of_tally(r.tally),
        // a host from before readiness records: up is all it can say
        _ => HostState::Ready(Tally::default()),
    }
}

/// The pid holding the flock on lock `name`. Read from `/proc/locks`, so the check never takes the lock itself (a host
/// starting at that moment would otherwise be refused). A descriptor-level check handles filesystems whose
/// stat device number differs from the lock table's superblock device number.
fn lock_holder(name: &str) -> Option<i32> {
    let path = crate::single::runtime_file(&format!("{name}.lock"));
    let meta = std::fs::metadata(&path).ok()?;
    if let Some(pid) = std::fs::read_to_string("/proc/locks").ok().and_then(|locks| flock_holder(&locks, meta.dev(), meta.ino())) {
        return Some(pid);
    }
    let pid = std::fs::read_to_string(&path).ok()?.trim().parse().ok()?;
    descriptor_holds_lock(pid, &meta).then_some(pid)
}

fn descriptor_holds_lock(pid: i32, target: &std::fs::Metadata) -> bool {
    if pid <= 0 {
        return false;
    }
    let process = PathBuf::from(format!("/proc/{pid}"));
    if !std::fs::metadata(&process).is_ok_and(|m| m.uid() == target.uid()) {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(process.join("fd")) else { return false };
    entries.filter_map(Result::ok).any(|entry| {
        // Compare the descriptor's real file identity before inspecting its kernel lock metadata.
        if !std::fs::metadata(entry.path()).is_ok_and(|m| m.dev() == target.dev() && m.ino() == target.ino()) {
            return false;
        }
        std::fs::read_to_string(process.join("fdinfo").join(entry.file_name())).is_ok_and(|info| fd_has_flock(&info, pid))
    })
}

fn fd_has_flock(info: &str, pid: i32) -> bool {
    info.lines().filter_map(|line| line.strip_prefix("lock:")).any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.get(1) == Some(&"FLOCK") && fields.get(3) == Some(&"WRITE") && fields.get(4).and_then(|v| v.parse::<i32>().ok()) == Some(pid)
    })
}

/// The holder of an active (not waiting) FLOCK on the file `dev`/`ino` in the text of `/proc/locks`.
fn flock_holder(locks: &str, dev: u64, ino: u64) -> Option<i32> {
    // glibc's major()/minor() of st_dev, which /proc/locks prints as `%02x:%02x:%lu`
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    let want = format!("{major:02x}:{minor:02x}:{ino}");
    locks.lines().find_map(|line| {
        // `1: FLOCK  ADVISORY  WRITE 1234 00:1a:5678 0 EOF`; a waiter has `->` after the index and is skipped
        let f: Vec<&str> = line.split_whitespace().collect();
        (f.get(1) == Some(&"FLOCK") && f.get(5) == Some(&want.as_str())).then(|| f.get(4)?.parse().ok()).flatten()
    })
}

fn start_time(pid: i32) -> Option<u64> {
    parse_start_time(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// Field 22 (starttime) of a `/proc/<pid>/stat` line; the command name may contain spaces and parentheses.
fn parse_start_time(stat: &str) -> Option<u64> {
    stat[stat.rfind(')')? + 1..].split_whitespace().nth(19)?.parse().ok()
}

/// Watches the host of one server. Opened once per wait: the mode is decided (and the compositor probed) by the caller,
/// and on X11 one connection serves every check.
pub struct Probe {
    key: HostKey,
    config: Option<u64>,
    #[cfg(feature = "x11")]
    x11: Option<super::host_x11::ReadyProbe>,
}

impl Probe {
    /// `shell_json`: the configuration the host is expected to show; `None` accepts whatever it built last.
    pub fn new(key: HostKey, shell_json: Option<&Path>) -> Probe {
        Probe {
            config: shell_json.map(config_stamp),
            #[cfg(feature = "x11")]
            x11: if key.mode == Mode::X11 { super::host_x11::ReadyProbe::connect() } else { None },
            key,
        }
    }

    pub fn state(&mut self) -> HostState {
        let holder = lock_holder(&self.key.lock);
        let rec = std::fs::read_to_string(self.key.record_path()).ok().and_then(|t| Record::parse(&t));
        let ok = rec.as_ref().is_some_and(|r| trusted(r, &Facts { holder, start: start_time(r.pid), server: self.key.server.clone(), endpoint: self.key.endpoint() }));
        match self.key.mode {
            Mode::Wayland => match classify(holder, rec.as_ref(), ok, self.config) {
                HostState::Down => legacy_host(&self.key).map_or(HostState::Down, HostState::Legacy),
                state => state,
            },
            Mode::X11 => {
                #[cfg(feature = "x11")]
                let owned = self.x11.as_mut().is_some_and(|p| p.owned());
                #[cfg(not(feature = "x11"))]
                let owned = false;
                classify_x11(owned, holder, rec.as_ref(), ok, self.config)
            }
        }
    }
}

/// The state of this session's host, checked against the configuration in `shell_json` when given.
pub fn state(shell_json: Option<&Path>) -> HostState {
    match mode().and_then(HostKey::current) {
        Some(key) => Probe::new(key, shell_json).state(),
        None => HostState::Down,
    }
}

/// True once the host of this session has shown the configuration it last read (some cards may be missing).
pub fn is_running() -> bool {
    state(None).running()
}

/// The host's side of the readiness record: written atomically once a build has settled, rewritten when the outcome
/// changes, removed when the host ends (only while it is still ours).
pub(super) struct Announcer {
    path: PathBuf,
    base: Record,
    last: Option<(u64, Tally)>,
}

impl Announcer {
    /// Call while holding the host lock: any record already there is from a host that died without cleaning up.
    pub(super) fn new(key: &HostKey) -> Announcer {
        let path = key.record_path();
        let _ = std::fs::remove_file(&path);
        let pid = std::process::id() as i32;
        let base = Record { pid, start: start_time(pid).unwrap_or(0), server: key.server.clone(), endpoint: key.endpoint(), config: 0, tally: Tally::default() };
        Announcer { path, base, last: None }
    }

    /// No prior frame tally is valid while surfaces are missing or being replaced.
    /// Fail closed if the old record cannot be removed: a live host must not advertise stale Ready state.
    pub(super) fn invalidate(&mut self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("could not invalidate desktop widget readiness: {error}")),
        }
        self.last = None;
        Ok(())
    }

    /// The build for `config` has settled with `tally`.
    pub(super) fn settle(&mut self, config: u64, tally: Tally) {
        if self.last == Some((config, tally)) {
            return;
        }
        let rec = Record { config, tally, ..self.base.clone() };
        let tmp = self.path.with_extension(format!("state.tmp{}", rec.pid));
        if std::fs::write(&tmp, rec.encode()).and_then(|()| std::fs::rename(&tmp, &self.path)).is_ok() {
            self.last = Some((config, tally));
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    #[cfg(test)]
    pub(super) fn settled(&self) -> bool {
        self.last.is_some()
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        let ours = std::fs::read_to_string(&self.path).ok().and_then(|t| Record::parse(&t)).is_some_and(|r| r.pid == self.base.pid);
        if ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

// ---------------------------------------------------------------- hosts from before per-server locks

/// The lock every host used before per-server locks, on both servers (the X11 host still uses it).
const LEGACY_LOCK: &str = "decwidgets";
/// Written by an older Wayland host once its first frame was up: its pid (removed when it ended normally). Older X11 hosts
/// never wrote it and current hosts do not.
const LEGACY_READY: &str = "decwidgets.ready";

/// The part of a process's starting environment (`/proc/<pid>/environ`) that names its Wayland server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct TargetEnv {
    pub wayland_display: Option<String>,
    pub runtime_dir: Option<PathBuf>,
}

pub(super) fn parse_target_env(raw: &[u8]) -> TargetEnv {
    let mut env = TargetEnv::default();
    for kv in raw.split(|b| *b == 0).map(String::from_utf8_lossy) {
        if let Some(v) = kv.strip_prefix("WAYLAND_DISPLAY=").filter(|v| !v.is_empty()) {
            env.wayland_display = Some(v.to_string());
        } else if let Some(v) = kv.strip_prefix("XDG_RUNTIME_DIR=").filter(|v| !v.is_empty()) {
            env.runtime_dir = Some(PathBuf::from(v));
        }
    }
    env
}

/// What is known about the holder of the generic lock.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct LegacyFacts {
    /// Pid holding the `decwidgets` flock now (from `/proc/locks`).
    pub holder: Option<i32>,
    /// Pid in `decwidgets.ready`.
    pub ready_pid: Option<i32>,
    /// The holder is a current X11 host: its trusted readiness record names it (same pid and start time) and an X11 server.
    pub current_x11: bool,
    /// The holder's starting environment; `None` when it could not be read.
    pub env: Option<TargetEnv>,
}

/// The pid of a host from before per-server locks that serves the Wayland server of `key`, so a keyed host must not start
/// next to it. Proof required: the pid in `decwidgets.ready` (only older Wayland hosts write it) is the live holder of the
/// generic lock, that holder is not a current X11 host, and its environment names this compositor's socket. A holder whose
/// environment names another socket (a nested compositor, a second session) is left alone, and so is any X11 host. An
/// environment that cannot be read counts as this compositor: the older host is a Wayland host of this user and runtime
/// directory, and two hosts drawing the same cards is the failure being prevented.
pub(super) fn legacy_for(key: &HostKey, f: &LegacyFacts) -> Option<i32> {
    if key.mode != Mode::Wayland || f.current_x11 {
        return None;
    }
    let pid = f.holder.filter(|h| f.ready_pid == Some(*h))?;
    match &f.env {
        None => Some(pid),
        Some(env) => {
            let theirs = HostKey::wayland(env.wayland_display.as_deref()?, env.runtime_dir.as_deref())?;
            (theirs.server == key.server).then_some(pid)
        }
    }
}

fn legacy_facts() -> LegacyFacts {
    let holder = lock_holder(LEGACY_LOCK);
    let ready_pid = std::fs::read_to_string(crate::single::runtime_file(LEGACY_READY)).ok().and_then(|s| s.trim().parse().ok()).filter(|p: &i32| *p > 0);
    let current_x11 = holder.is_some_and(|h| {
        let rec = std::fs::read_to_string(crate::single::runtime_file(&format!("{LEGACY_LOCK}.state"))).ok().and_then(|t| Record::parse(&t));
        rec.is_some_and(|r| r.pid == h && start_time(h) == Some(r.start) && r.server.starts_with("x11:"))
    });
    let env = holder.filter(|h| !current_x11 && ready_pid == Some(*h)).and_then(|h| std::fs::read(format!("/proc/{h}/environ")).ok()).map(|raw| parse_target_env(&raw));
    LegacyFacts { holder, ready_pid, current_x11, env }
}

/// [`legacy_for`] with the facts of this machine now; `None` for X11 keys without looking.
pub fn legacy_host(key: &HostKey) -> Option<i32> {
    if key.mode != Mode::Wayland {
        return None;
    }
    legacy_for(key, &legacy_facts())
}

fn legacy_message(pid: i32) -> String {
    format!(
        "desktop widgets from before the update (process {pid}) still run on this display, so the updated host was not started next to them; \
         they keep showing widgets but cannot confirm new choices. Log out and back in (or end that process) to switch to the updated host"
    )
}

// ---------------------------------------------------------------- starting the host

fn spawn_host() -> bool {
    crate::launch::spawn_argv(&[crate::launch::sibling_or_path("decwidgets"), "daemon".into()])
}

/// Starts this session's host when none holds its lock, without waiting for its first frame and without probing the
/// compositor: for the bar's startup and theme apply, which must not stall on a new Wayland host. The daemon decides
/// the server itself; when the guess here differs (a compositor without layer-shell), the extra copy finds the real
/// host's lock and exits.
pub fn start() -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    let Some(key) = session::Env::current().decide(|| true).and_then(HostKey::current) else { return Err(NO_DISPLAY.into()) };
    if lock_holder(&key.lock).is_none() {
        if let Some(pid) = legacy_host(&key) {
            // served, by a host the updated one must not run next to; settings and `decwidgets status` say what to do
            crate::log_info!("decwidgets: not starting: {}", legacy_message(pid));
            return Ok(());
        }
    }
    if lock_holder(&key.lock).is_some() || spawn_host() {
        Ok(())
    } else {
        Err("Could not start desktop widgets".into())
    }
}

/// Starts this session's host if needed and waits (bounded) until it has shown the saved configuration. An error says
/// when cards are enabled but could not all be shown, so the caller can tell the user instead of claiming success.
pub fn ensure_running() -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    let key = mode().and_then(HostKey::current).ok_or(NO_DISPLAY)?;
    let shell_json = crate::theme::live::Source::discover().shell_json;
    let mut probe = Probe::new(key, Some(shell_json.as_path()));
    if probe.state() == HostState::Down && !spawn_host() {
        return Err("Could not start desktop widgets".into());
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match probe.state() {
            HostState::Ready(_) => return Ok(()),
            HostState::Degraded(t) => return Err(t.problem()),
            HostState::Legacy(pid) => return Err(legacy_message(pid)),
            HostState::Down | HostState::Starting if Instant::now() >= deadline => return Err("Desktop widgets did not start; see the decwidgets log".into()),
            HostState::Down | HostState::Starting => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

/// Runs this session's host until it is stopped. Returns at once when another host already serves this server; with
/// `autostart`, a session without a usable display is not an error.
pub fn run(paths: Paths, autostart: bool) -> Result<(), String> {
    let Some(key) = mode().and_then(HostKey::current) else { return if autostart { Ok(()) } else { Err(NO_DISPLAY.into()) } };
    // An older host already draws this compositor's cards: never a second set next to it, never a signal to it.
    if let Some(pid) = legacy_host(&key) {
        return if autostart {
            crate::log_info!("decwidgets: {}", legacy_message(pid));
            Ok(())
        } else {
            Err(legacy_message(pid))
        };
    }
    // One host per server: a second copy (a supervisor restart, a racing start) leaves the first alone.
    let Some(_lock) = crate::single::hold(&key.lock) else { return Ok(()) };
    let announcer = Announcer::new(&key);
    match key.mode {
        #[cfg(feature = "wayland")]
        Mode::Wayland => super::host_wayland::run(paths, announcer),
        #[cfg(feature = "x11")]
        Mode::X11 => super::host_x11::run(paths, announcer),
        #[allow(unreachable_patterns)]
        _ => {
            let _ = (paths, announcer);
            Err(NO_DISPLAY.into())
        }
    }
}

// ---------------------------------------------------------------- shared by both backends

/// How many desktop cards one host shows at most; compositors limit layer surfaces, and a configuration past this is a
/// mistake, not a desktop.
pub(super) const MAX_CARDS: usize = 64;

/// The enabled desktop cards of `cfg`, in order (none when widgets are hidden).
pub(super) fn wanted(cfg: &WidgetsConfig) -> impl Iterator<Item = &Instance> {
    cfg.items.iter().filter(move |i| cfg.enabled && i.enabled && i.place == config::Place::Desktop)
}

/// Size of the card for the current data and fonts.
pub(super) fn lay(def: &WidgetDef, data: &Data, fonts: &Fonts) -> Item {
    let th = crate::theme::th();
    layout::layout(def, &Ctx { data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, None)
}

pub(super) fn data_with_options(base: &Data, inst: &Instance) -> Data {
    let mut d = base.clone();
    for (k, v) in &inst.options {
        let val = match v {
            serde_json::Value::String(s) => super::data::Val::Text(s.clone()),
            serde_json::Value::Number(n) => super::data::Val::Num(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::Bool(b) => super::data::Val::Bool(*b),
            _ => continue,
        };
        d.set(&format!("opt.{k}"), val);
    }
    d
}

/// Mtimes of the widget definition files in use, so editing a widget redraws it without a restart.
pub(super) fn theme_stamp(paths: &Paths) -> u64 {
    let cfg = WidgetsConfig::load(&paths.shell_json);
    let mut sum = 0u64;
    for i in &cfg.items {
        let file = match &i.source {
            Some(s) => config::package_dir(&paths.themes_dir, s).map(|d| d.join("widgets").join(format!("{}.toml", i.use_))),
            None => Some(config::user_dir().join(format!("{}.toml", i.use_))),
        };
        if let Some(m) = file.and_then(|f| std::fs::metadata(f).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
            sum = sum.wrapping_add(m.as_nanos() as u64);
        }
    }
    sum
}

pub(super) fn dots_under(item: &Item, x: f32, y: f32) -> Option<u32> {
    if !item.rect.contains(x, y) {
        return None;
    }
    item.children.iter().find_map(|c| dots_under(c, x, y)).or_else(|| layout::dot_at(item, x))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pid: i32, tally: Tally) -> Record {
        Record { pid, start: 4242, server: "/run/user/1000/wayland-1".into(), endpoint: "27:991".into(), config: 7, tally }
    }

    fn facts(holder: Option<i32>, start: Option<u64>) -> Facts {
        Facts { holder, start, server: "/run/user/1000/wayland-1".into(), endpoint: "27:991".into() }
    }

    const SHOWN: Tally = Tally { wanted: 2, shown: 2, broken: 0, unplaced: 0 };

    #[test]
    fn a_record_round_trips_and_garbage_or_a_bare_pid_is_not_a_record() {
        let r = record(1234, SHOWN);
        assert_eq!(Record::parse(&r.encode()), Some(r));
        assert_eq!(Record::parse("1234"), None, "the plain pid file of an older host");
        assert_eq!(Record::parse(""), None);
        assert_eq!(Record::parse(&record(1234, SHOWN).encode().replace("shown=2\n", "")), None, "a truncated record");
        assert_eq!(Record::parse(&record(1234, SHOWN).encode().replace("pid=1234", "pid=0")), None);
    }

    #[test]
    fn only_the_live_lock_holder_with_the_same_start_time_and_server_is_trusted() {
        let r = record(1234, SHOWN);
        assert!(trusted(&r, &facts(Some(1234), Some(4242))));
        assert!(!trusted(&r, &facts(None, Some(4242))), "SIGKILLed host: the kernel released its lock, the record stayed");
        assert!(!trusted(&r, &facts(Some(999), Some(4242))), "another host holds the lock now");
        assert!(!trusted(&r, &facts(Some(1234), Some(5555))), "the pid was reused by a later process");
        assert!(!trusted(&r, &facts(Some(1234), None)), "no such process");
        let foreign = Facts { server: "/run/user/1000/wayland-2".into(), ..facts(Some(1234), Some(4242)) };
        assert!(!trusted(&r, &foreign), "a host of another compositor");
        let restarted = Facts { endpoint: "27:1002".into(), ..facts(Some(1234), Some(4242)) };
        assert!(!trusted(&r, &restarted), "a restarted compositor that reuses the socket name");
    }

    #[test]
    fn a_stale_record_is_starting_or_down_never_ready() {
        let r = record(1234, SHOWN);
        assert_eq!(classify(None, Some(&r), false, None), HostState::Down, "lock free: the host is gone whatever the file says");
        assert_eq!(classify(Some(77), Some(&r), false, None), HostState::Starting, "a new host has not settled yet");
        assert_eq!(classify(Some(1234), None, false, None), HostState::Starting);
        assert_eq!(classify(Some(1234), Some(&r), true, None), HostState::Ready(SHOWN));
        assert_eq!(classify(Some(1234), Some(&r), true, Some(8)), HostState::Starting, "still building a newer configuration");
        assert_eq!(classify(Some(1234), Some(&r), true, Some(7)), HostState::Ready(SHOWN));
    }

    #[test]
    fn cards_without_an_output_or_with_errors_are_degraded_but_zero_enabled_is_ready() {
        let none = Tally { wanted: 2, shown: 0, broken: 0, unplaced: 2 };
        assert_eq!(classify(Some(1), Some(&record(1, none)), true, None), HostState::Degraded(none));
        assert!(none.problem().contains("0 of 2") && none.problem().contains("no display"));
        let broken = Tally { wanted: 3, shown: 2, broken: 1, unplaced: 0 };
        assert_eq!(classify(Some(1), Some(&record(1, broken)), true, None), HostState::Degraded(broken));
        assert!(broken.problem().contains("could not be loaded"));
        let idle = Tally::default();
        assert_eq!(classify(Some(1), Some(&record(1, idle)), true, None), HostState::Ready(idle), "nothing enabled is ready on purpose");
        assert!(HostState::Degraded(none).running() && !HostState::Starting.running() && !HostState::Down.running());
    }

    #[test]
    fn an_x11_host_is_up_by_its_selection_and_refined_by_a_trusted_record() {
        let r = record(1, Tally { wanted: 1, shown: 0, broken: 1, unplaced: 0 });
        assert_eq!(classify_x11(false, None, Some(&r), true, None), HostState::Down);
        assert_eq!(classify_x11(false, Some(1), None, false, None), HostState::Starting);
        assert_eq!(classify_x11(true, Some(1), None, false, Some(3)), HostState::Ready(Tally::default()), "an older host without records");
        assert_eq!(classify_x11(true, Some(1), Some(&r), false, Some(3)), HostState::Ready(Tally::default()), "an untrusted record is ignored");
        assert_eq!(classify_x11(true, Some(1), Some(&r), true, Some(7)), HostState::Degraded(r.tally));
        assert_eq!(classify_x11(true, Some(1), Some(&r), true, Some(8)), HostState::Starting);
    }

    #[test]
    fn the_lock_holder_comes_from_proc_locks_and_waiters_are_skipped() {
        // st_dev 0x1a (major 0, minor 0x1a) and 0x10302 (major 0x103, minor 2)
        let locks = "1: FLOCK  ADVISORY  WRITE 555 00:1a:900 0 EOF\n1: -> FLOCK  ADVISORY  WRITE 556 00:1a:901 0 EOF\n2: POSIX  ADVISORY  WRITE 557 00:1a:902 0 EOF\n3: FLOCK  ADVISORY  WRITE 558 103:02:77 0 EOF\n";
        assert_eq!(flock_holder(locks, 0x1a, 900), Some(555));
        assert_eq!(flock_holder(locks, 0x1a, 901), None, "only a waiter");
        assert_eq!(flock_holder(locks, 0x1a, 902), None, "not a flock");
        assert_eq!(flock_holder(locks, 0x1b, 900), None, "another device");
        assert_eq!(flock_holder(locks, 0x10302, 77), Some(558));
    }

    #[test]
    fn descriptor_lock_identity_requires_a_real_exclusive_flock() {
        use std::os::fd::AsRawFd;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("widget-lock-{}-{nonce}", std::process::id()));
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        // The open descriptor remains usable; no temporary path can be left by a failed assertion.
        std::fs::remove_file(&path).unwrap();
        let meta = file.metadata().unwrap();
        let pid = std::process::id() as i32;
        assert!(!descriptor_holds_lock(pid, &meta), "an open file alone is not readiness");
        // SAFETY: operate only on the private descriptor created by this test.
        assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        assert!(descriptor_holds_lock(pid, &meta));
        assert!(!descriptor_holds_lock(0, &meta));
        // SAFETY: release that same private descriptor's lock.
        assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) }, 0);
        assert!(!descriptor_holds_lock(pid, &meta), "a stale PID cannot establish ownership");
    }

    #[test]
    fn descriptor_lock_parser_rejects_waiters_shared_and_foreign_locks() {
        assert!(fd_has_flock("lock:\t1: FLOCK ADVISORY WRITE 555 00:23:900 0 EOF", 555));
        assert!(!fd_has_flock("lock:\t1: -> FLOCK ADVISORY WRITE 555 00:23:900 0 EOF", 555));
        assert!(!fd_has_flock("lock:\t1: FLOCK ADVISORY READ 555 00:23:900 0 EOF", 555));
        assert!(!fd_has_flock("lock:\t1: FLOCK ADVISORY WRITE 556 00:23:900 0 EOF", 555));
        assert!(!fd_has_flock("lock:\t1: POSIX ADVISORY WRITE 555 00:23:900 0 EOF", 555));
    }

    #[test]
    fn start_time_survives_odd_command_names_and_matches_this_process() {
        let stat = "4321 (a (b) c) S 1 4321 4321 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10";
        assert_eq!(parse_start_time(stat), Some(987654));
        assert_eq!(parse_start_time("garbage"), None);
        let me = std::process::id() as i32;
        assert!(start_time(me).is_some() && start_time(me) == start_time(me));
    }

    #[test]
    fn a_wayland_host_is_scoped_to_its_socket_and_x11_keeps_its_lock() {
        let rt = Path::new("/run/user/1000");
        let a = HostKey::wayland("wayland-1", Some(rt)).unwrap();
        let b = HostKey::wayland("wayland-2", Some(rt)).unwrap();
        assert_eq!(a.server, "/run/user/1000/wayland-1");
        assert_ne!(a.lock, b.lock);
        assert_ne!(a.record_path(), b.record_path());
        assert_eq!(HostKey::wayland("/tmp/nested/wl-0", None).unwrap().server, "/tmp/nested/wl-0");
        assert!(HostKey::wayland("/tmp/nested/wl-0", None).unwrap().lock.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(HostKey::wayland("wayland-1", None), None, "a relative socket needs XDG_RUNTIME_DIR");
        assert_eq!(HostKey::x11(":0").lock, "decwidgets", "the X11 host keeps the lock it always had");
        assert_ne!(HostKey::x11(":0").lock, a.lock);
    }

    #[test]
    fn the_announcer_writes_atomically_and_removes_only_its_own_record() {
        let key = HostKey { mode: Mode::Wayland, lock: format!("decebal-test-widgets-{}", std::process::id()), server: "/nonexistent/wl".into() };
        let path = key.record_path();
        std::fs::write(&path, record(1, SHOWN).encode()).unwrap();
        let mut a = Announcer::new(&key);
        assert!(!path.exists(), "a record left by a dead host is cleared before the first build");
        assert!(!a.settled());
        a.settle(9, SHOWN);
        let rec = Record::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!((rec.pid, rec.config, rec.tally, rec.start), (std::process::id() as i32, 9, SHOWN, start_time(std::process::id() as i32).unwrap()));
        a.invalidate().unwrap();
        assert!(!path.exists(), "missing surfaces must not leave a Ready announcement");
        assert!(!a.settled());
        a.invalidate().unwrap();
        a.settle(9, SHOWN);
        assert!(path.exists(), "the same configuration is announced again only after replacement frames settle");
        drop(a);
        assert!(!path.exists());
        let mut a = Announcer::new(&key);
        a.settle(1, SHOWN);
        std::fs::write(&path, record(1, SHOWN).encode()).unwrap();
        drop(a);
        assert!(path.exists(), "a successor's record is left alone");
        let _ = std::fs::remove_file(&path);
    }

    fn legacy(holder: Option<i32>, ready: Option<i32>, current_x11: bool, env: Option<(&str, &str)>) -> LegacyFacts {
        LegacyFacts { holder, ready_pid: ready, current_x11, env: env.map(|(d, r)| TargetEnv { wayland_display: Some(d.into()), runtime_dir: Some(r.into()) }) }
    }

    #[test]
    fn an_older_wayland_host_of_this_compositor_blocks_a_keyed_host_and_nothing_else_does() {
        let rt = Path::new("/run/user/1000");
        let key = HostKey::wayland("wayland-1", Some(rt)).unwrap();
        let same = Some(("wayland-1", "/run/user/1000"));
        assert_eq!(legacy_for(&key, &legacy(Some(700), Some(700), false, same)), Some(700), "proven: ready pid = lock holder, same socket");
        assert_eq!(legacy_for(&key, &legacy(Some(700), Some(700), false, Some(("/run/user/1000/wayland-1", "/elsewhere")))), Some(700), "an absolute socket path");
        assert_eq!(legacy_for(&key, &legacy(Some(700), Some(700), false, None)), Some(700), "unreadable environment: counted, never two sets of cards");
        assert_eq!(legacy_for(&key, &legacy(Some(700), Some(700), false, Some(("wayland-2", "/run/user/1000")))), None, "a nested or second compositor");
        assert_eq!(legacy_for(&key, &legacy(Some(700), Some(700), false, Some(("wayland-1", "/run/user/1001")))), None, "another runtime directory");
        let no_wayland = LegacyFacts { env: Some(TargetEnv { wayland_display: None, runtime_dir: Some(rt.into()) }), ..legacy(Some(700), Some(700), false, None) };
        assert_eq!(legacy_for(&key, &no_wayland), None, "a holder without a Wayland display draws on X11");
        assert_eq!(legacy_for(&key, &legacy(Some(700), Some(700), true, same)), None, "a current X11 host holds the generic lock");
        assert_eq!(legacy_for(&key, &legacy(Some(700), None, false, same)), None, "an older X11 host never wrote the ready file");
        assert_eq!(legacy_for(&key, &legacy(None, Some(700), false, same)), None, "a ready file left by a host that is gone");
        assert_eq!(legacy_for(&key, &legacy(Some(701), Some(700), false, same)), None, "the ready pid is not the holder (a stale file)");
        assert_eq!(legacy_for(&HostKey::x11(":0"), &legacy(Some(700), Some(700), false, same)), None, "X11 keeps its one lock as before");
    }

    #[test]
    fn the_target_environment_is_read_from_a_nul_separated_block() {
        let raw = b"HOME=/home/u\0WAYLAND_DISPLAY=wayland-1\0XDG_RUNTIME_DIR=/run/user/1000\0DISPLAY=:0\0";
        assert_eq!(parse_target_env(raw), TargetEnv { wayland_display: Some("wayland-1".into()), runtime_dir: Some("/run/user/1000".into()) });
        assert_eq!(parse_target_env(b"WAYLAND_DISPLAY=\0DISPLAY=:1"), TargetEnv::default(), "empty values are unset");
        assert!(HostState::Legacy(7).running() && legacy_message(7).contains("process 7"));
    }

    #[test]
    fn an_exited_host_is_not_alive() {
        assert!(start_time(std::process::id() as i32).is_some());
        assert_eq!(start_time(-1), None);
    }
}
