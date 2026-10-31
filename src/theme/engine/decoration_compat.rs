//! Mode-specific window-decoration styles, and the proof that the window manager can draw them.
//!
//! A theme names the style it wants for each mode, plus an optional fallback, with generic `[decoration]` keys (no theme or family id appears
//! anywhere in this module):
//!
//! | key | meaning |
//! |---|---|
//! | `style_light`, `style_dark` | the style wanted in that mode |
//! | `style` | the older single key: the wanted style when the mode has no `style_<mode>`, and the only key engines without this contract read |
//! | `fallback_style_light`, `fallback_style_dark`, `fallback_style` | the style to use when the wanted one cannot be proven supported (mode-specific key first) |
//!
//! Nothing here asks a theme author to know the user's window manager. The engine asks the window manager that will draw the style, and only
//! one of four answers counts:
//!
//! * [`Capability::Running`]: the running rdwm answered its own `schema` command: its `decoration.style` choices are the proof;
//! * [`Capability::Offline`]: no rdwm is running (the socket is missing or refuses), so the explicit local target `rdwmctl schema --offline`
//!   is the proof and the result says so ("takes effect when rdwm starts");
//! * [`Capability::Preview`]: the native preview worker draws with its own decoration helper and writes nothing to a window manager
//!   (explicit, set only by the preview worker through [`PREVIEW_ENV`]);
//! * [`Capability::Unknown`]: a socket is there but the answer is missing, malformed, from an older rdwm without `schema`, or timed out.
//!   This is never treated as proof, and never as "offline": the caller gets an error before anything is written.
//!
//! The installed command-line tool's offline table is not evidence about a running window manager; it is only used when none is running.
use crate::settings::io::Paths;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The `[decoration]` keys added by this contract (all optional, all typed as identifiers).
pub const KEYS: [&str; 5] = ["style_light", "style_dark", "fallback_style", "fallback_style_light", "fallback_style_dark"];
/// The first release that understands [`KEYS`]; a package using them must declare it as `min_shell_version`.
pub const SINCE: &str = "0.3.0";
/// Set to `preview` by the native preview worker (and nothing else) to draw the wanted style without asking a window manager.
pub const PREVIEW_ENV: &str = "DECEBAL_DECO_CAPS";
/// The key of `decebal.json` that records a fallback in use (shown by Settings until the wanted style applies).
pub const RECORD: &str = "decoration_compat";

/// Whether a theme's extra sections use this contract (see [`SINCE`]).
pub fn needs_framework(extra: &Map<String, Value>) -> bool {
    extra.get("decoration").is_some_and(|d| KEYS.iter().any(|k| d.get(*k).is_some()))
}

/// A style identifier: what rdwm accepts as a `decoration.style` name.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.as_bytes()[0].is_ascii_alphanumeric() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// What a theme wants in one mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub desired: String,
    pub fallback: Option<String>,
}

/// The style a theme wants in this mode, or `None` when it does not use the contract (the plain `style` key keeps its old behaviour).
pub fn wanted(extra: &Map<String, Value>, dark: bool) -> Option<Wanted> {
    let d = extra.get("decoration")?;
    if !KEYS.iter().any(|k| d.get(*k).is_some()) {
        return None;
    }
    let mode = if dark { "dark" } else { "light" };
    let get = |k: &str| d.get(k).and_then(Value::as_str).filter(|s| valid_id(s)).map(String::from);
    let desired = get(&format!("style_{mode}")).or_else(|| get("style"))?;
    let fallback = get(&format!("fallback_style_{mode}")).or_else(|| get("fallback_style"));
    Some(Wanted { desired, fallback })
}

/// The window manager's answer about the styles it can draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// The running rdwm listed these (its own `schema` reply).
    Running { styles: BTreeSet<String> },
    /// No rdwm is running; the target `rdwmctl schema --offline` listed these.
    Offline { styles: BTreeSet<String>, cli: PathBuf },
    /// The native preview worker: the wanted style is drawn by the preview's decoration helper.
    Preview,
    /// Not known: never proof, never "offline".
    Unknown { reason: String },
}

/// The `decoration.style` choices of an rdwm `schema` reply (`keys` entry, and `decoration_styles` records when present; conservative where they differ).
pub fn styles_of(reply: &Value) -> Option<BTreeSet<String>> {
    let from_keys: Option<BTreeSet<String>> = reply["keys"].as_array().and_then(|keys| {
        keys.iter()
            .find(|k| k["section"] == "decoration" && k["key"] == "style")
            .map(|k| k["choices"].as_array().map(|c| c.iter().filter_map(|s| s.as_str()).map(String::from).collect()).unwrap_or_default())
    });
    let from_records: Option<BTreeSet<String>> = reply["decoration_styles"].as_array().map(|a| a.iter().filter_map(|s| s["name"].as_str()).map(String::from).collect());
    let set = match (from_keys, from_records) {
        (Some(k), Some(r)) => k.intersection(&r).cloned().collect(),
        (Some(k), None) => k,
        (None, Some(r)) => r,
        (None, None) => return None,
    };
    (!set.is_empty()).then_some(set)
}

/// Time budgets of one probe. Everything inside a probe happens on the calling thread under one absolute deadline, so a stalled window manager,
/// a full socket backlog or a misbehaving helper cannot hold theme planning longer than this, and no thread is left behind.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// Connect, send the request and read the whole answer from a running rdwm.
    pub running: Duration,
    /// Run the target `rdwmctl schema --offline`, read its whole output and see it exit.
    pub offline: Duration,
}

impl Budget {
    pub const DEFAULT: Budget = Budget { running: Duration::from_secs(3), offline: Duration::from_secs(8) };
}

/// The longest answer read from a running rdwm (the real `schema` reply is about 30 KiB).
const MAX_REPLY: usize = 2 << 20;
/// The longest output read from the offline helper.
const MAX_HELPER_OUTPUT: usize = 4 << 20;

/// Asks the window manager that will draw the style (see the module documentation for what counts as proof).
pub fn probe(paths: &Paths) -> Capability {
    if std::env::var(PREVIEW_ENV).as_deref() == Ok("preview") {
        return Capability::Preview;
    }
    probe_at(&paths.socket, &paths.rdwmctl)
}

/// [`probe`] for an explicit socket and an explicit target `rdwmctl`.
pub fn probe_at(sock: &Path, rdwmctl: &Path) -> Capability {
    probe_at_with(sock, rdwmctl, Budget::DEFAULT)
}

/// [`probe_at`] with explicit budgets.
pub fn probe_at_with(sock: &Path, rdwmctl: &Path, budget: Budget) -> Capability {
    match ask(sock, &json!({"cmd": "schema"}), budget.running) {
        // nothing there, or nothing listening behind a stale socket file: no window manager is running
        Asked::NotRunning => offline(rdwmctl, budget.offline),
        Asked::Failed(why) => Capability::Unknown { reason: format!("the running rdwm could not be asked ({why})") },
        Asked::Reply(reply) => match styles_of(&reply) {
            Some(styles) => Capability::Running { styles },
            None => Capability::Unknown { reason: "the running rdwm answered `schema` without a decoration.style list (an older rdwm?)".into() },
        },
    }
}

/// The result of one question to a window manager.
enum Asked {
    Reply(Value),
    /// The socket does not exist or nobody listens behind it.
    NotRunning,
    Failed(String),
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Waits until `events` are ready on `fd`; `false` when the deadline passes first.
fn wait_ready(fd: RawFd, events: i16, deadline: Instant) -> Result<bool, String> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        let mut p = libc::pollfd { fd, events, revents: 0 };
        let ms = left.as_millis().clamp(1, 1000) as i32;
        // SAFETY: `p` is a valid pollfd and the count is one.
        let r = unsafe { libc::poll(&mut p, 1, ms) };
        if r > 0 {
            return Ok(true);
        }
        if r < 0 && errno() != libc::EINTR {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
}

/// One request and its answer on ONE non-blocking connection under one absolute deadline: connect (retried while the backlog is full), write the line,
/// read until the newline. There is no probe connection before it, so nothing can change between "is it there" and "ask it".
fn ask(sock: &Path, request: &Value, budget: Duration) -> Asked {
    let deadline = Instant::now() + budget;
    let bytes = sock.as_os_str().as_bytes();
    // SAFETY: a zeroed sockaddr_un is a valid value to fill in.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.is_empty() || bytes.len() >= addr.sun_path.len() || bytes.contains(&0) {
        return Asked::Failed(format!("the socket path {} cannot be used", sock.display()));
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (i, b) in bytes.iter().enumerate() {
        addr.sun_path[i] = *b as libc::c_char;
    }
    // SAFETY: plain socket(2); the descriptor is owned immediately.
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, 0) };
    if raw < 0 {
        return Asked::Failed(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: `raw` is a fresh descriptor nobody else owns.
    let sock_fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let fd = sock_fd.as_raw_fd();
    let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    loop {
        // SAFETY: `addr` is a valid sockaddr_un of `len` bytes.
        if unsafe { libc::connect(fd, &addr as *const _ as *const libc::sockaddr, len) } == 0 {
            break;
        }
        match errno() {
            libc::ENOENT | libc::ENOTDIR | libc::ECONNREFUSED => return Asked::NotRunning,
            // a non-blocking connect to a full backlog is EAGAIN: try again until the deadline
            libc::EAGAIN | libc::EINTR | libc::EALREADY => {
                if Instant::now() >= deadline {
                    return Asked::Failed("the socket accepts no connection (its backlog is full?)".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            libc::EINPROGRESS => match wait_ready(fd, libc::POLLOUT, deadline) {
                Ok(true) => {
                    let mut err: libc::c_int = 0;
                    let mut l = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
                    // SAFETY: SO_ERROR into a c_int of the stated length.
                    unsafe { libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_ERROR, &mut err as *mut _ as *mut libc::c_void, &mut l) };
                    match err {
                        0 => break,
                        libc::ENOENT | libc::ECONNREFUSED => return Asked::NotRunning,
                        e => return Asked::Failed(std::io::Error::from_raw_os_error(e).to_string()),
                    }
                }
                Ok(false) => return Asked::Failed("the connection was not accepted in time".into()),
                Err(e) => return Asked::Failed(e),
            },
            e => return Asked::Failed(std::io::Error::from_raw_os_error(e).to_string()),
        }
    }
    let payload = format!("{request}\n").into_bytes();
    let mut sent = 0;
    while sent < payload.len() {
        // SAFETY: the pointer and length describe the unsent tail of `payload`.
        let n = unsafe { libc::send(fd, payload[sent..].as_ptr() as *const libc::c_void, payload.len() - sent, libc::MSG_NOSIGNAL) };
        if n > 0 {
            sent += n as usize;
            continue;
        }
        match errno() {
            libc::EAGAIN | libc::EINTR => match wait_ready(fd, libc::POLLOUT, deadline) {
                Ok(true) => {}
                Ok(false) => return Asked::Failed("the request could not be sent in time".into()),
                Err(e) => return Asked::Failed(e),
            },
            e => return Asked::Failed(std::io::Error::from_raw_os_error(e).to_string()),
        }
    }
    let mut reply: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        if let Some(end) = reply.iter().position(|b| *b == b'\n') {
            reply.truncate(end);
            break;
        }
        match wait_ready(fd, libc::POLLIN, deadline) {
            Ok(true) => {}
            Ok(false) => return Asked::Failed("no complete answer in time".into()),
            Err(e) => return Asked::Failed(e),
        }
        // SAFETY: `chunk` is a valid writable buffer of its own length.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
        if n > 0 {
            reply.extend_from_slice(&chunk[..n as usize]);
            if reply.len() > MAX_REPLY {
                return Asked::Failed(format!("the answer is larger than {MAX_REPLY} bytes"));
            }
        } else if n == 0 {
            if reply.is_empty() {
                return Asked::Failed("the connection closed without an answer".into());
            }
            break;
        } else if !matches!(errno(), libc::EAGAIN | libc::EINTR) {
            return Asked::Failed(std::io::Error::last_os_error().to_string());
        }
    }
    match serde_json::from_slice::<Value>(&reply) {
        Ok(v) if v["ok"] == Value::Bool(false) => Asked::Failed(v["error"].as_str().unwrap_or("error").to_string()),
        Ok(v) => Asked::Reply(v),
        Err(e) => Asked::Failed(format!("the answer is not JSON: {e}")),
    }
}

/// Owns the helper's whole process group: whatever the path out, the group is killed and the child reaped.
struct Helper {
    child: Option<Child>,
    pgid: i32,
}

impl Helper {
    /// Whether the helper has exited, without reaping it (the pid stays reserved, so `kill_group` cannot hit a stranger).
    fn exited(&self) -> bool {
        // SAFETY: waitid with WNOWAIT|WNOHANG only inspects the child; `info` is a zeroed siginfo_t.
        unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            let r = libc::waitid(libc::P_PID, self.pgid as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT);
            r == 0 && info.si_pid() != 0
        }
    }
    fn kill_group(&self) {
        // SAFETY: SIGKILL to the process group this helper leads (its leader is still unreaped or just exited, so the id is ours).
        unsafe { libc::kill(-self.pgid, libc::SIGKILL) };
    }
    /// Kills any survivor of the group, reaps the child and returns its status.
    fn finish(&mut self) -> Option<std::process::ExitStatus> {
        self.kill_group();
        self.child.take().and_then(|mut c| c.wait().ok())
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.finish();
        }
    }
}

fn spawn_group(rdwmctl: &Path) -> std::io::Result<Child> {
    let mut tries = 0;
    loop {
        let r = Command::new(rdwmctl).args(["schema", "--offline"]).env_remove("RDWM_SOCK").process_group(0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        // a script that was written a moment ago can be "text file busy" while another thread's fork still holds its writer; that clears at once
        match r {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && tries < 5 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

fn offline(rdwmctl: &Path, budget: Duration) -> Capability {
    let unknown = |why: String| Capability::Unknown { reason: format!("rdwm is not running and the target rdwmctl `{}` could not list its styles ({why})", rdwmctl.display()) };
    let deadline = Instant::now() + budget;
    let mut child = match spawn_group(rdwmctl) {
        Ok(c) => c,
        Err(e) => return unknown(e.to_string()),
    };
    let Some(stdout) = child.stdout.take() else { return unknown("no output pipe".into()) };
    let pgid = child.id() as i32;
    let mut helper = Helper { child: Some(child), pgid };
    let fd = stdout.as_raw_fd();
    // SAFETY: fcntl on a descriptor this function owns, setting O_NONBLOCK so a quiet pipe can never block the read.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    // drain the output while the helper runs (a full pipe must not block it), up to the cap and the absolute deadline
    let mut out: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match wait_ready(fd, libc::POLLIN, deadline) {
            Ok(true) => {}
            Ok(false) => {
                helper.finish();
                return unknown("it did not finish in time (a descendant may be holding its output open)".into());
            }
            Err(e) => {
                helper.finish();
                return unknown(e);
            }
        }
        // SAFETY: `chunk` is a valid writable buffer of its own length.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
        if n > 0 {
            out.extend_from_slice(&chunk[..n as usize]);
            if out.len() > MAX_HELPER_OUTPUT {
                helper.finish();
                return unknown(format!("its output is larger than {MAX_HELPER_OUTPUT} bytes"));
            }
        } else if n == 0 {
            break;
        } else if !matches!(errno(), libc::EAGAIN | libc::EINTR) {
            let e = std::io::Error::last_os_error().to_string();
            helper.finish();
            return unknown(e);
        }
    }
    // the output is complete (EOF): the helper must still exit within the deadline, then every survivor of its group is killed
    while !helper.exited() {
        if Instant::now() >= deadline {
            helper.finish();
            return unknown("it closed its output but did not exit in time".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let status = helper.finish();
    if !status.is_some_and(|s| s.success()) {
        return unknown(format!("exit status {}", status.map_or("unknown".into(), |s| s.to_string())));
    }
    match serde_json::from_slice::<Value>(&out).ok().as_ref().and_then(styles_of) {
        Some(styles) => Capability::Offline { styles, cli: rdwmctl.to_path_buf() },
        None => unknown("its reply has no decoration.style list".into()),
    }
}

/// Where a resolved style was proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proof {
    Running,
    Offline,
    Preview,
}

/// The style to write, and the honest account of how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub style: String,
    pub desired: String,
    pub fallback_used: bool,
    pub proof: Proof,
    /// Set when a fallback is in use: persists in `decebal.json` and Settings until the wanted style applies.
    pub warning: Option<String>,
    /// A one-time remark for the apply report (not persistent).
    pub note: Option<String>,
}

/// Picks the style to write from what the theme wants and what the window manager can draw.
///
/// `Err` is a complete, user-readable reason and means nothing may be written: the wanted style cannot be proven, and no fallback can be.
pub fn resolve(w: &Wanted, cap: &Capability) -> Result<Resolution, String> {
    let (styles, proof, who) = match cap {
        Capability::Preview => return Ok(Resolution { style: w.desired.clone(), desired: w.desired.clone(), fallback_used: false, proof: Proof::Preview, warning: None, note: None }),
        Capability::Unknown { reason } => {
            return Err(format!("Window style `{}` could not be checked: {reason}. Nothing was changed. Try again when rdwm answers, or turn off `Windows follow theme`.", w.desired))
        }
        Capability::Running { styles } => (styles, Proof::Running, "the running rdwm".to_string()),
        Capability::Offline { styles, cli } => (styles, Proof::Offline, format!("rdwm is not running; the target rdwmctl `{}`", cli.display())),
    };
    let tail = if proof == Proof::Offline { " It takes effect when rdwm starts." } else { "" };
    if styles.contains(&w.desired) {
        let note = (proof == Proof::Offline).then(|| format!("Window style `{}` was checked against the target rdwmctl because rdwm is not running.{tail}", w.desired));
        return Ok(Resolution { style: w.desired.clone(), desired: w.desired.clone(), fallback_used: false, proof, warning: None, note });
    }
    let missing = format!("{who} does not list the window style `{}` ({} styles)", w.desired, styles.len());
    match &w.fallback {
        Some(f) if styles.contains(f) => Ok(Resolution {
            style: f.clone(),
            desired: w.desired.clone(),
            fallback_used: true,
            proof,
            warning: Some(format!(
                "Window style `{}` needs a newer rdwm: {who} does not list it, so `{f}` is used for now. Upgrade rdwm and log in again (or restart rdwm), then apply the theme again.{tail}",
                w.desired
            )),
            note: None,
        }),
        Some(f) => Err(format!("{missing}, and the fallback `{f}` is not listed either. Nothing was changed. Upgrade rdwm and log in again, or choose another theme.")),
        None => Err(format!("{missing}, and the theme declares no fallback. Nothing was changed. Upgrade rdwm and log in again, or choose another theme.")),
    }
}

/// The whole decision for one apply: `Ok(None)` when the theme does not use the contract.
pub fn prepare(extra: &Map<String, Value>, dark: bool, paths: &Paths) -> Result<Option<Resolution>, String> {
    let Some(w) = wanted(extra, dark) else { return Ok(None) };
    resolve(&w, &probe(paths)).map(Some)
}

/// Writes the resolved style where the existing `decoration.style` path reads it (and removes the contract keys from the working copy).
pub fn rewrite(extra: &mut Map<String, Value>, res: &Resolution) {
    if let Some(Value::Object(d)) = extra.get_mut("decoration") {
        for k in KEYS {
            d.shift_remove(k);
        }
        d.insert("style".into(), json!(res.style));
    }
}

/// Keeps `decebal.json` honest: a record exists exactly while a fallback is in use for the applied theme and mode.
pub fn record(shell: &mut Value, theme: &str, dark: bool, res: Option<&Resolution>) {
    let Some(obj) = shell.as_object_mut() else { return };
    match res.filter(|r| r.fallback_used) {
        Some(r) => {
            obj.insert(
                RECORD.into(),
                json!({"theme": theme, "mode": if dark {"dark"} else {"light"}, "desired": r.desired, "applied": r.style,
                       "proof": match r.proof { Proof::Running => "running", Proof::Offline => "offline", Proof::Preview => "preview" },
                       "message": r.warning.clone().unwrap_or_default()}),
            );
        }
        None => {
            obj.shift_remove(RECORD);
        }
    }
}

/// The persistent warning to show in Settings, if a fallback is in use.
pub fn warning(shell: &Value) -> Option<String> {
    let r = shell.get(RECORD)?;
    let (message, theme) = (r["message"].as_str()?, r["theme"].as_str()?);
    (!message.is_empty() && !theme.is_empty()).then(|| message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn extra(decoration: Value) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("decoration".into(), decoration);
        m
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deco-compat-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn schema_reply(styles: &[&str]) -> Value {
        json!({"ok": true, "keys": [{"section": "decoration", "key": "style", "choices": styles}],
               "decoration_styles": styles.iter().map(|s| json!({"name": s})).collect::<Vec<_>>()})
    }

    /// A fake rdwm that answers every request with `reply(cmd)` until dropped.
    fn fake_wm(sock: &Path, reply: impl Fn(&str) -> Option<String> + Send + 'static) -> std::thread::JoinHandle<()> {
        let listener = UnixListener::bind(sock).unwrap();
        std::thread::spawn(move || {
            listener.set_nonblocking(false).unwrap();
            // exactly one connection: the request itself (there is no separate probe connection)
            for stream in listener.incoming().take(1) {
                let Ok(mut s) = stream else { continue };
                let mut line = String::new();
                if BufReader::new(&s).read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let cmd = serde_json::from_str::<Value>(&line).ok().and_then(|v| v["cmd"].as_str().map(String::from)).unwrap_or_default();
                if let Some(text) = reply(&cmd) {
                    let _ = writeln!(s, "{text}");
                }
            }
        })
    }

    /// A fake rdwmctl: a script that prints `json` for `schema --offline` (or exits non-zero when `json` is `None`).
    fn fake_ctl(dir: &Path, json: Option<&str>) -> PathBuf {
        let p = dir.join("rdwmctl");
        let body = match json {
            Some(j) => format!("#!/bin/sh\nif [ \"$1 $2\" = \"schema --offline\" ]; then cat <<'EOF'\n{j}\nEOF\nelse exit 2; fi\n"),
            None => "#!/bin/sh\nexit 3\n".to_string(),
        };
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    const OLD: &[&str] = &["macos", "classic", "windows", "windows-light", "adwaita", "adwaita-light"];
    const NEW: &[&str] = &["macos", "classic", "windows", "windows-light", "adwaita", "adwaita-light", "lingmo", "lingmo-dark"];

    #[test]
    fn wanted_is_generic_mode_specific_and_absent_for_old_style_themes() {
        let d = extra(json!({"style": "adwaita", "style_light": "fam", "style_dark": "fam-dark", "fallback_style_light": "adwaita-light", "fallback_style_dark": "adwaita"}));
        assert_eq!(wanted(&d, false), Some(Wanted { desired: "fam".into(), fallback: Some("adwaita-light".into()) }));
        assert_eq!(wanted(&d, true), Some(Wanted { desired: "fam-dark".into(), fallback: Some("adwaita".into()) }));
        // the single `style` key is the wanted style of a mode that has none, and `fallback_style` serves both modes
        let d = extra(json!({"style": "breeze", "style_dark": "x-dark", "fallback_style": "breeze"}));
        assert_eq!(wanted(&d, false), Some(Wanted { desired: "breeze".into(), fallback: Some("breeze".into()) }));
        assert_eq!(wanted(&d, true), Some(Wanted { desired: "x-dark".into(), fallback: Some("breeze".into()) }));
        // a theme that only has the old key keeps the old path; nothing about a family id matters
        assert_eq!(wanted(&extra(json!({"style": "macos"})), true), None);
        assert_eq!(wanted(&Map::new(), true), None);
        // invalid identifiers are ignored, not passed to rdwm
        assert_eq!(wanted(&extra(json!({"style_dark": "../x", "style": "macos"})), true), Some(Wanted { desired: "macos".into(), fallback: None }));
        assert!(needs_framework(&extra(json!({"fallback_style": "macos"}))) && !needs_framework(&extra(json!({"style": "macos"}))));
    }

    #[test]
    fn running_wm_lists_decide_and_the_fallback_is_used_only_when_proven() {
        let w = Wanted { desired: "lingmo-dark".into(), fallback: Some("adwaita".into()) };
        let new = Capability::Running { styles: set(NEW) };
        let r = resolve(&w, &new).unwrap();
        assert_eq!((r.style.as_str(), r.fallback_used, r.proof, r.warning.is_none()), ("lingmo-dark", false, Proof::Running, true));
        let old = Capability::Running { styles: set(OLD) };
        let r = resolve(&w, &old).unwrap();
        assert_eq!((r.style.as_str(), r.fallback_used), ("adwaita", true));
        let warning = r.warning.unwrap();
        assert!(warning.contains("lingmo-dark") && warning.contains("running rdwm") && warning.contains("log in again"), "{warning}");
        // a fallback that the same window manager does not list is not "proven": error, nothing written
        let w2 = Wanted { desired: "lingmo-dark".into(), fallback: Some("deepin".into()) };
        assert!(resolve(&w2, &old).unwrap_err().contains("fallback `deepin` is not listed"));
        // no fallback declared: error
        let w3 = Wanted { desired: "lingmo-dark".into(), fallback: None };
        assert!(resolve(&w3, &old).unwrap_err().contains("declares no fallback"));
    }

    #[test]
    fn offline_proof_is_labelled_and_unknown_is_never_proof() {
        let w = Wanted { desired: "lingmo".into(), fallback: Some("adwaita-light".into()) };
        let off = Capability::Offline { styles: set(NEW), cli: "/x/rdwmctl".into() };
        let r = resolve(&w, &off).unwrap();
        assert_eq!((r.style.as_str(), r.proof, r.fallback_used), ("lingmo", Proof::Offline, false));
        assert!(r.note.unwrap().contains("not running"), "offline use says so");
        let off_old = Capability::Offline { styles: set(OLD), cli: "/x/rdwmctl".into() };
        let r = resolve(&w, &off_old).unwrap();
        assert!(r.fallback_used && r.warning.unwrap().contains("takes effect when rdwm starts"));
        // unknown: error even though a fallback exists and every style might "probably" work
        let err = resolve(&w, &Capability::Unknown { reason: "timed out".into() }).unwrap_err();
        assert!(err.contains("could not be checked") && err.contains("timed out") && err.contains("Nothing was changed"), "{err}");
        // preview: the wanted style, no gate, no warning
        let r = resolve(&w, &Capability::Preview).unwrap();
        assert_eq!((r.style.as_str(), r.proof, r.warning), ("lingmo", Proof::Preview, None));
    }

    #[test]
    fn probe_asks_the_running_wm_and_ignores_the_installed_cli_table() {
        let d = tmpdir("probe-running");
        // the installed (target) CLI knows the new styles, the RUNNING wm does not: the running answer wins
        let ctl = fake_ctl(&d, Some(&schema_reply(NEW).to_string()));
        let sock = d.join("old.sock");
        let _ = std::fs::remove_file(&sock);
        let h = fake_wm(&sock, |cmd| (cmd == "schema").then(|| schema_reply(OLD).to_string()));
        assert_eq!(probe_at(&sock, &ctl), Capability::Running { styles: set(OLD) });
        h.join().ok();
        let sock = d.join("new.sock");
        let h = fake_wm(&sock, |cmd| (cmd == "schema").then(|| schema_reply(NEW).to_string()));
        assert_eq!(probe_at(&sock, &Path::new("/nonexistent/rdwmctl")), Capability::Running { styles: set(NEW) });
        h.join().ok();
    }

    #[test]
    fn probe_without_a_wm_uses_only_the_explicit_target_cli_and_otherwise_is_unknown() {
        let d = tmpdir("probe-offline");
        let none = d.join("missing.sock");
        let ctl = fake_ctl(&d, Some(&schema_reply(NEW).to_string()));
        match probe_at(&none, &ctl) {
            Capability::Offline { styles, cli } => assert_eq!((styles, cli), (set(NEW), ctl.clone())),
            other => panic!("{other:?}"),
        }
        // the target CLI fails, or does not exist, or prints something without styles: unknown, not an empty offline proof
        for bad in [fake_ctl(&tmpdir("probe-offline-fail"), None), PathBuf::from("/nonexistent/rdwmctl"), fake_ctl(&tmpdir("probe-offline-empty"), Some("{\"ok\":true}"))] {
            assert!(matches!(probe_at(&none, &bad), Capability::Unknown { .. }), "{}", bad.display());
        }
        // a stale socket file nobody listens on counts as "not running"
        let stale = d.join("stale.sock");
        drop(UnixListener::bind(&stale).unwrap());
        assert!(matches!(probe_at(&stale, &ctl), Capability::Offline { .. }));
    }

    #[test]
    fn transient_and_old_running_answers_are_unknown_not_offline() {
        let d = tmpdir("probe-transient");
        // the target CLI would say "supported"; a running but unhelpful wm must still be Unknown
        let ctl = fake_ctl(&d, Some(&schema_reply(NEW).to_string()));
        let cases: Vec<(&str, Box<dyn Fn(&str) -> Option<String> + Send>)> = vec![
            ("error", Box::new(|_| Some(json!({"ok": false, "error": "unknown command"}).to_string()))),
            ("garbage", Box::new(|_| Some("not json".to_string()))),
            ("silent", Box::new(|_| None)),
            ("no-styles", Box::new(|_| Some(json!({"ok": true, "keys": [], "actions": []}).to_string()))),
        ];
        for (name, reply) in cases {
            let sock = d.join(format!("{name}.sock"));
            let h = fake_wm(&sock, reply);
            let got = probe_at(&sock, &ctl);
            assert!(matches!(got, Capability::Unknown { .. }), "{name}: {got:?}");
            h.join().ok();
        }
    }

    #[test]
    fn the_preview_worker_is_the_only_explicit_gate_bypass() {
        // not set: a probe against nothing is the offline path (and with no CLI, unknown)
        let paths = Paths { rdwm_config: "/x/c.toml".into(), shell_dir: "/x".into(), rdwmctl: "/nonexistent/rdwmctl".into(), socket: "/nonexistent/rdwm.sock".into() };
        if std::env::var(PREVIEW_ENV).is_err() {
            assert!(matches!(probe(&paths), Capability::Unknown { .. }));
        }
    }

    #[test]
    fn styles_of_is_conservative_when_keys_and_records_differ() {
        let both = json!({"keys": [{"section": "decoration", "key": "style", "choices": ["a", "b", "c"]}], "decoration_styles": [{"name": "a"}, {"name": "b"}]});
        assert_eq!(styles_of(&both), Some(set(&["a", "b"])));
        assert_eq!(styles_of(&json!({"keys": [{"section": "general", "key": "x"}]})), None);
        assert_eq!(styles_of(&json!({"keys": [{"section": "decoration", "key": "style", "choices": []}]})), None);
    }

    #[test]
    fn record_and_warning_exist_only_while_a_fallback_is_in_use() {
        let mut shell = json!({"theme": {"name": "x"}});
        let w = Wanted { desired: "lingmo-dark".into(), fallback: Some("adwaita".into()) };
        let fallback = resolve(&w, &Capability::Running { styles: set(OLD) }).unwrap();
        record(&mut shell, "lingmo", true, Some(&fallback));
        assert_eq!(shell[RECORD]["desired"], "lingmo-dark");
        assert_eq!((shell[RECORD]["applied"].as_str(), shell[RECORD]["mode"].as_str(), shell[RECORD]["proof"].as_str()), (Some("adwaita"), Some("dark"), Some("running")));
        assert!(warning(&shell).unwrap().contains("needs a newer rdwm"));
        // the wanted style applies later: the record goes away, and so does it for a theme without the contract
        let ok = resolve(&w, &Capability::Running { styles: set(NEW) }).unwrap();
        record(&mut shell, "lingmo", true, Some(&ok));
        assert!(shell.get(RECORD).is_none() && warning(&shell).is_none());
        record(&mut shell, "lingmo", true, Some(&fallback));
        record(&mut shell, "other", true, None);
        assert!(shell.get(RECORD).is_none());
        assert!(warning(&json!({RECORD: {"theme": "x", "message": ""}})).is_none());
    }

    #[test]
    fn rewrite_leaves_one_plain_style_for_the_existing_path() {
        let mut e = extra(json!({"style": "adwaita", "style_dark": "lingmo-dark", "fallback_style": "adwaita", "height": 30}));
        let r = resolve(&Wanted { desired: "lingmo-dark".into(), fallback: None }, &Capability::Running { styles: set(NEW) }).unwrap();
        rewrite(&mut e, &r);
        assert_eq!(e["decoration"], json!({"style": "lingmo-dark", "height": 30}));
    }

    // ------------------------------------------------------------ bounded I/O: no thread, one deadline, no leftovers

    fn budget(ms: u64) -> Budget {
        Budget { running: Duration::from_millis(ms), offline: Duration::from_millis(ms) }
    }

    fn sockaddr(path: &Path) -> (libc::sockaddr_un, libc::socklen_t) {
        let mut a: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        a.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (i, b) in path.as_os_str().as_bytes().iter().enumerate() {
            a.sun_path[i] = *b as libc::c_char;
        }
        (a, std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t)
    }

    /// A listener nobody accepts from, with backlog 0, and its accept queue filled by clients that stay connected.
    fn full_backlog(path: &Path) -> (OwnedFd, Vec<OwnedFd>) {
        let (addr, len) = sockaddr(path);
        unsafe {
            let l = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
            assert!(l >= 0);
            assert_eq!(libc::bind(l, &addr as *const _ as *const libc::sockaddr, len), 0);
            assert_eq!(libc::listen(l, 0), 0);
            let mut clients = vec![];
            for _ in 0..16 {
                let c = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, 0);
                let r = libc::connect(c, &addr as *const _ as *const libc::sockaddr, len);
                let full = r != 0 && errno() == libc::EAGAIN;
                clients.push(OwnedFd::from_raw_fd(c));
                if full {
                    return (OwnedFd::from_raw_fd(l), clients);
                }
            }
            panic!("the accept queue never filled");
        }
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn gone(pid: i32) -> bool {
        for _ in 0..80 {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(s) if s.rsplit(") ").next().is_some_and(|r| r.starts_with('Z')) => return true,
                Ok(_) => std::thread::sleep(Duration::from_millis(25)),
            }
        }
        false
    }

    fn good_json() -> String {
        schema_reply(NEW).to_string()
    }

    #[test]
    fn a_full_backlog_cannot_hang_planning() {
        let d = tmpdir("backlog");
        let sock = d.join("full.sock");
        let (_listener, _clients) = full_backlog(&sock);
        let ctl = fake_ctl(&d, Some(&good_json()));
        let t = Instant::now();
        let got = probe_at_with(&sock, &ctl, budget(400));
        assert!(t.elapsed() < Duration::from_millis(1500), "bounded: {:?}", t.elapsed());
        match got {
            Capability::Unknown { reason } => assert!(reason.contains("backlog"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_stalled_or_partial_answer_is_bounded_and_is_unknown() {
        let d = tmpdir("stalled");
        let ctl = fake_ctl(&d, Some(&good_json()));
        for (name, partial) in [("silent", None), ("partial", Some(&b"{\"ok\":tr"[..]))] {
            let sock = d.join(format!("{name}.sock"));
            let listener = UnixListener::bind(&sock).unwrap();
            let partial = partial.map(|p| p.to_vec());
            let h = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut line = String::new();
                let _ = BufReader::new(&s).read_line(&mut line);
                if let Some(p) = partial {
                    let _ = s.write_all(&p);
                }
                std::thread::sleep(Duration::from_millis(1500));
            });
            let t = Instant::now();
            let got = probe_at_with(&sock, &ctl, budget(400));
            assert!(t.elapsed() < Duration::from_millis(1200), "{name}: {:?}", t.elapsed());
            assert!(matches!(got, Capability::Unknown { .. }), "{name}: {got:?}");
            h.join().ok();
        }
    }

    #[test]
    fn an_oversized_answer_is_refused_not_buffered_forever() {
        let d = tmpdir("oversized");
        let sock = d.join("big.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut line = String::new();
            let _ = BufReader::new(&s).read_line(&mut line);
            let junk = vec![b'x'; 64 * 1024];
            for _ in 0..(3 << 20) / junk.len() {
                if s.write_all(&junk).is_err() {
                    break;
                }
            }
        });
        let got = probe_at_with(&sock, Path::new("/nonexistent/rdwmctl"), budget(3000));
        match got {
            Capability::Unknown { reason } => assert!(reason.contains("larger than"), "{reason}"),
            other => panic!("{other:?}"),
        }
        h.join().ok();
    }

    #[test]
    fn the_probe_uses_exactly_one_connection() {
        let d = tmpdir("one-connection");
        let sock = d.join("one.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        listener.set_nonblocking(true).unwrap();
        let h = std::thread::spawn(move || {
            let mut accepted = 0;
            let until = Instant::now() + Duration::from_millis(900);
            while Instant::now() < until {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        accepted += 1;
                        s.set_nonblocking(false).unwrap();
                        let mut line = String::new();
                        let _ = BufReader::new(&s).read_line(&mut line);
                        let _ = writeln!(s, "{}", schema_reply(NEW));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
            accepted
        });
        let got = probe_at_with(&sock, Path::new("/nonexistent/rdwmctl"), budget(2000));
        assert!(matches!(got, Capability::Running { .. }), "{got:?}");
        assert_eq!(h.join().unwrap(), 1, "no probe connection before the question");
    }

    #[test]
    fn a_large_helper_output_is_drained_while_it_runs() {
        // more than a pipe holds: a parent that waits for exit before reading would deadlock the helper
        let d = tmpdir("helper-large");
        let body = format!(
            "if [ \"$1 $2\" = \"schema --offline\" ]; then printf '{{\"ok\":true,\"keys\":[{{\"section\":\"decoration\",\"key\":\"style\",\"choices\":[\"a\",\"b\"]}}],\"pad\":\"'; head -c 2000000 /dev/zero | tr '\\0' 'x'; printf '\"}}\\n'; else exit 2; fi"
        );
        let ctl = script(&d, "ctl-large", &body);
        let none = d.join("none.sock");
        let t = Instant::now();
        let got = probe_at_with(&none, &ctl, budget(6000));
        assert!(matches!(got, Capability::Offline { ref styles, .. } if *styles == set(&["a", "b"])), "{got:?}");
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn an_oversized_helper_output_is_refused_and_the_helper_killed() {
        let d = tmpdir("helper-oversized");
        let body = format!("echo $$ > '{}/pid'; head -c 6000000 /dev/zero | tr '\\0' 'x'; sleep 30", d.display());
        let ctl = script(&d, "ctl-oversized", &body);
        let got = probe_at_with(&d.join("none.sock"), &ctl, budget(6000));
        match got {
            Capability::Unknown { reason } => assert!(reason.contains("larger than"), "{reason}"),
            other => panic!("{other:?}"),
        }
        let pid: i32 = std::fs::read_to_string(d.join("pid")).unwrap().trim().parse().unwrap();
        assert!(gone(pid), "the helper must not be left running");
    }

    #[test]
    fn a_descendant_holding_stdout_cannot_hang_and_is_cleaned_up() {
        let d = tmpdir("helper-descendant");
        // the helper prints a valid answer and exits at once, but leaves a background process that keeps its stdout open for a long time
        let body = format!("if [ \"$1 $2\" = \"schema --offline\" ]; then sleep 30 & echo $! > '{}/pid'; cat <<'EOF'\n{}\nEOF\nelse exit 2; fi", d.display(), good_json());
        let ctl = script(&d, "ctl-descendant", &body);
        let t = Instant::now();
        let got = probe_at_with(&d.join("none.sock"), &ctl, budget(1000));
        assert!(t.elapsed() < Duration::from_millis(3000), "bounded: {:?}", t.elapsed());
        match got {
            Capability::Unknown { reason } => assert!(reason.contains("did not finish"), "{reason}"),
            other => panic!("{other:?}"),
        }
        let pid: i32 = std::fs::read_to_string(d.join("pid")).unwrap().trim().parse().unwrap();
        assert!(gone(pid), "the descendant must be killed with the helper's process group");
    }

    #[test]
    fn a_helper_that_never_answers_is_killed_at_the_deadline() {
        let d = tmpdir("helper-hang");
        let ctl = script(&d, "ctl-hang", &format!("echo $$ > '{}/pid'; sleep 30", d.display()));
        let t = Instant::now();
        let got = probe_at_with(&d.join("none.sock"), &ctl, budget(500));
        assert!(t.elapsed() < Duration::from_millis(2500), "{:?}", t.elapsed());
        assert!(matches!(got, Capability::Unknown { .. }), "{got:?}");
        let pid: i32 = std::fs::read_to_string(d.join("pid")).unwrap().trim().parse().unwrap();
        assert!(gone(pid));
    }
}
