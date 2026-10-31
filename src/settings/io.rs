//! Files, validation, atomic writes, rdwm IPC and the background watcher.

use super::tomlcfg::Cfg;
use serde_json::Value;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime};

pub const BACKUP_SUFFIX: &str = "bak-decsettings";

// ---------------------------------------------------------------- paths

#[derive(Debug, Clone)]
pub struct Paths {
    pub rdwm_config: PathBuf,
    /// `~/.config/omarchy` (or `$DECEBAL_CONFIG`): dock-settings.json etc.
    pub shell_dir: PathBuf,
    pub rdwmctl: PathBuf,
    /// rdwm's IPC socket (`RDWM_SOCK`, else `$XDG_RUNTIME_DIR/rdwm.sock`).
    pub socket: PathBuf,
}

impl Paths {
    pub fn discover() -> Paths {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let rdwm_config = std::env::var_os("DECSETTINGS_CONFIG").or_else(|| std::env::var_os("RDWM_CONFIG")).map(PathBuf::from).unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(|x| PathBuf::from(x).join("rdwm/config.toml")).unwrap_or_else(|| home.join(".config/rdwm/config.toml"))
        });
        let socket = match std::env::var_os("RDWM_SOCK").filter(|s| !s.is_empty()) {
            Some(p) => PathBuf::from(p),
            None => std::env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("rdwm.sock")).unwrap_or_else(|| PathBuf::from("/nonexistent/rdwm.sock")),
        };
        Paths {
            rdwm_config,
            shell_dir: crate::config::config_dir(),
            rdwmctl: std::env::var_os("RDWMCTL").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(crate::launch::sibling_or_path("rdwmctl"))),
            socket,
        }
    }

    pub fn dock_settings(&self) -> PathBuf {
        self.shell_dir.join("dock-settings.json")
    }
    pub fn dock_pinned(&self) -> PathBuf {
        self.shell_dir.join("dock-pinned-macos.json")
    }
    pub fn shell_json(&self) -> PathBuf {
        self.shell_dir.join("decebal.json")
    }
    pub fn backgrounds(&self) -> PathBuf {
        self.shell_dir.join("backgrounds")
    }
}

// ---------------------------------------------------------------- atomic write

/// File identity used to notice external edits: modification time, size and inode (a rename over
/// the file changes the inode even when mtime and size coincide).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stamp(pub Option<(SystemTime, u64, u64)>);

impl Stamp {
    /// For task results that cross the worker boundary.
    pub fn to_value(&self) -> Value {
        match self.0 {
            Some((t, len, ino)) => {
                let d = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
                serde_json::json!([d.as_secs(), d.subsec_nanos(), len, ino])
            }
            None => Value::Null,
        }
    }

    pub fn from_value(v: &Value) -> Stamp {
        let n = |i: usize| v.get(i).and_then(Value::as_u64);
        match (n(0), n(1), n(2), n(3)) {
            (Some(s), Some(ns), Some(len), Some(ino)) => Stamp(Some((SystemTime::UNIX_EPOCH + Duration::new(s, ns as u32), len, ino))),
            _ => Stamp(None),
        }
    }
}

pub fn stamp(path: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    Stamp(std::fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len(), m.ino()))))
}

// ---------------------------------------------------------------- one write protocol for every writer

/// The text every write that found its file changed since it was read fails with (nothing was written then).
pub const CHANGED_ON_DISK: &str = "changed on disk while saving; nothing was written";

pub fn is_changed_on_disk(e: &str) -> bool {
    e.ends_with(CHANGED_ON_DISK)
}

fn file_lock(target: &Path) -> Arc<Mutex<()>> {
    static LOCKS: std::sync::OnceLock<Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>> = std::sync::OnceLock::new();
    LOCKS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(target.to_path_buf()).or_default().clone()
}

/// The file a write of `path` really replaces (a symlinked config is written through).
fn target_of(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Runs `f` (with the real target) while no other writer in this process touches the file: every writer in `io`
/// (the persistence worker, the theme engine, the remaining synchronous panes) takes this lock from its read to its
/// rename. Writers in other processes are caught by the identity check right before the rename instead.
/// Re-entrant on one thread, so a transaction holding the lock can call helpers that write the same file.
pub fn with_file_lock<T>(path: &Path, f: impl FnOnce(&Path) -> T) -> T {
    thread_local! {
        static HELD: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let target = target_of(path);
    if HELD.with(|h| h.borrow().contains(&target)) {
        return f(&target);
    }
    let lock = file_lock(&target);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    HELD.with(|h| h.borrow_mut().push(target.clone()));
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            HELD.with(|h| h.borrow_mut().retain(|p| p != &self.0));
        }
    }
    let _release = Release(target.clone());
    f(&target)
}

/// Writes `text` next to `path` and renames it over the target (following a symlinked target so
/// dotfile managers keep working). Keeps the permissions of an existing file, makes a one-time
/// `<name>.bak-decsettings` of the original, and fsyncs before the rename. A file that changes while it is
/// being written (an editor saving during a slow fsync) is not replaced: the write fails with [`CHANGED_ON_DISK`].
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    with_file_lock(path, |t| write_locked(t, text, stamp(t)))
}

/// [`write_atomic`], only while the file still has the identity `expect` it had when the caller read it.
pub fn write_if_unchanged(path: &Path, text: &str, expect: Stamp) -> Result<(), String> {
    with_file_lock(path, |t| write_locked(t, text, expect))
}

/// A private file (a secret, a cache): the same protocol, created with `mode` from the start (never readable by
/// others, not even briefly), and no `.bak-decsettings` copy.
pub fn write_private(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    with_file_lock(path, |t| write_with(t, bytes, stamp(t), Some(mode), false))
}

/// The caller holds the file lock of `target`.
fn write_locked(target: &Path, text: &str, expect: Stamp) -> Result<(), String> {
    write_with(target, text.as_bytes(), expect, None, true)
}

fn write_with(target: &Path, text: &[u8], expect: Stamp, mode: Option<u32>, backup: bool) -> Result<(), String> {
    static TMP: AtomicU64 = AtomicU64::new(0);
    let changed = || {
        crate::log_warn!("{} changed on disk while saving; not overwriting", target.display());
        Err(format!("{}: {CHANGED_ON_DISK}", target.display()))
    };
    if stamp(target) != expect {
        return changed();
    }
    let dir = target.parent().ok_or("config path has no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if backup && target.exists() {
        let bak = dir.join(format!("{name}.{BACKUP_SUFFIX}"));
        if !bak.exists() {
            std::fs::copy(target, &bak).map_err(|e| format!("backup {}: {e}", bak.display()))?;
        }
    }
    // Unique per write: two writers never share (truncate or rename) one another's temp file.
    let tmp = dir.join(format!(".{name}.decsettings-{}-{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    let res = (|| -> std::io::Result<bool> {
        let mut f = match mode {
            Some(m) => std::fs::OpenOptions::new().write(true).create_new(true).mode(m).open(&tmp)?,
            None => std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?,
        };
        f.write_all(text)?;
        fault::before_sync(target)?;
        f.sync_all()?;
        if mode.is_none() {
            if let Ok(m) = std::fs::metadata(target) {
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(m.permissions().mode() & 0o7777));
            }
        }
        // At commit: the file must still be what the caller read (the fsync above may have taken seconds).
        if stamp(target) != expect {
            return Ok(false);
        }
        std::fs::rename(&tmp, target)?;
        Ok(true)
    })();
    match res {
        Ok(true) => {
            crate::log_debug!("wrote {} ({} bytes)", target.display(), text.len());
            Ok(())
        }
        Ok(false) => {
            let _ = std::fs::remove_file(&tmp);
            changed()
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            crate::log_error!("write {} failed: {e}", target.display());
            Err(format!("{}: {e}", target.display()))
        }
    }
}

/// Deterministic slow or failing `fsync` for tests and private evidence builds. Production builds compile this to
/// nothing: the `settings-io-fault` feature is off by default and never part of a package.
pub mod fault {
    use std::path::Path;

    /// Runs before an fsync; an error fails that write.
    #[cfg(test)]
    pub type SyncHook = std::sync::Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>;

    #[cfg(test)]
    thread_local! {
        static HOOK: std::cell::RefCell<Option<SyncHook>> = const { std::cell::RefCell::new(None) };
    }

    /// Unit tests: runs `hook` before every `fsync` that `write_atomic` makes on the current thread.
    #[cfg(test)]
    pub fn set_thread_hook(hook: Option<SyncHook>) {
        HOOK.with(|h| *h.borrow_mut() = hook);
    }

    #[allow(unused_variables)]
    pub(super) fn before_sync(target: &Path) -> std::io::Result<()> {
        #[cfg(test)]
        if let Some(hook) = HOOK.with(|h| h.borrow().clone()) {
            hook(target)?;
        }
        #[cfg(feature = "settings-io-fault")]
        env_fault(target)?;
        Ok(())
    }

    /// `DECSETTINGS_FAULT_SYNC_GATE=<file>`: every fsync waits while that file exists (at most two minutes).
    /// `DECSETTINGS_FAULT_SYNC_MS=<n>`: every fsync takes `n` ms longer.
    /// `DECSETTINGS_FAULT_SYNC_FAIL=<text>`: an fsync of a file whose name contains `text` fails with EIO.
    #[cfg(feature = "settings-io-fault")]
    fn env_fault(target: &Path) -> std::io::Result<()> {
        use std::time::{Duration, Instant};
        let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(gate) = std::env::var_os("DECSETTINGS_FAULT_SYNC_GATE").filter(|g| !g.is_empty()) {
            let start = Instant::now();
            if Path::new(&gate).exists() {
                crate::log_info!("fault: fsync of {name} held by the gate file");
            }
            while Path::new(&gate).exists() && start.elapsed() < Duration::from_secs(120) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        if let Some(ms) = std::env::var("DECSETTINGS_FAULT_SYNC_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
            std::thread::sleep(Duration::from_millis(ms));
        }
        if std::env::var("DECSETTINGS_FAULT_SYNC_FAIL").ok().filter(|f| !f.is_empty()).is_some_and(|f| name.contains(&f)) {
            crate::log_info!("fault: failing fsync of {name}");
            return Err(std::io::Error::from_raw_os_error(5));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- rdwm config store

#[derive(Debug, PartialEq, Eq)]
pub enum External {
    Unchanged,
    /// The file changed and parsed; the document was replaced.
    Reloaded,
    /// The file changed but is not valid TOML right now (mid-edit): the old document is kept.
    Invalid(String),
}

/// `cfg` is what the window shows: the saved document plus edits the persistence worker has not committed yet
/// (see `persist.rs`). `text` and the stamp are always the saved file as this process last knew it.
pub struct Store {
    pub path: PathBuf,
    pub cfg: Cfg,
    pub text: String,
    stamp: Stamp,
    pub rdwmctl: PathBuf,
    /// Increases whenever the document was (re)read from disk rather than derived from this process's own writes.
    pub epoch: u64,
}

fn next_epoch() -> u64 {
    static EPOCH: AtomicU64 = AtomicU64::new(1);
    EPOCH.fetch_add(1, Ordering::Relaxed)
}

impl Store {
    pub fn load(path: &Path, rdwmctl: &Path) -> Store {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let cfg = Cfg::parse(&text).unwrap_or_default();
        Store { path: path.to_path_buf(), cfg, text, stamp: stamp(path), rdwmctl: rdwmctl.to_path_buf(), epoch: next_epoch() }
    }

    /// The saved file's identity as this process last knew it.
    pub fn saved_stamp(&self) -> Stamp {
        self.stamp
    }

    /// The persistence worker committed `text` (whose file identity is now `stamp`). `cfg` is left to the caller,
    /// which re-applies the edits still in flight on top.
    pub fn adopt(&mut self, text: String, stamp: Stamp) {
        self.text = text;
        self.stamp = stamp;
    }

    /// Re-reads the file when its stamp changed.
    pub fn check_external(&mut self) -> External {
        let now = stamp(&self.path);
        if now == self.stamp {
            return External::Unchanged;
        }
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            // deleted: behave like an empty file (rdwm uses built-in defaults)
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return External::Invalid(e.to_string()),
        };
        self.stamp = now;
        if text == self.text {
            return External::Unchanged;
        }
        match Cfg::parse(&text) {
            Ok(cfg) => {
                self.cfg = cfg;
                self.text = text;
                self.epoch = next_epoch();
                External::Reloaded
            }
            Err(e) => External::Invalid(e),
        }
    }

    /// Validates `new` exactly like `rdwm --check-config`, then replaces the file atomically.
    /// On `Err` nothing was written and `self` is unchanged.
    pub fn commit(&mut self, new: &Cfg) -> Result<bool, String> {
        let text = new.text();
        if text == self.text {
            return Ok(false);
        }
        if stamp(&self.path) != self.stamp {
            crate::log_warn!("{} changed on disk since it was read; not overwriting", self.path.display());
            // someone edited the file since we last read it: do not clobber, pick up their version
            return Err("config.toml changed on disk; reloaded it, please repeat the change".into());
        }
        if let Err(e) = validate(&self.rdwmctl, &self.path, &text) {
            crate::log_warn!("rdwm config rejected, nothing written: {e}");
            return Err(e);
        }
        write_if_unchanged(&self.path, &text, self.stamp)?;
        crate::log_info!("rdwm config saved: {}", self.path.display());
        self.cfg = new.clone();
        self.text = text;
        self.stamp = stamp(&self.path);
        Ok(true)
    }

    pub fn restore_backup(&mut self) -> Result<(), String> {
        let text = restore_backup(&self.path, &self.rdwmctl)?;
        self.cfg = Cfg::parse(&text)?;
        self.text = text;
        self.stamp = stamp(&self.path);
        Ok(())
    }
}

/// Puts the one-time `config.toml.bak-decsettings` back (validated like any other change); returns its text.
pub fn restore_backup(path: &Path, rdwmctl: &Path) -> Result<String, String> {
    let bak = path.with_file_name(format!("{}.{BACKUP_SUFFIX}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    let text = std::fs::read_to_string(&bak).map_err(|e| format!("{}: {e}", bak.display()))?;
    Cfg::parse(&text)?;
    validate(rdwmctl, path, &text)?;
    write_atomic(path, &text)?;
    Ok(text)
}

/// Runs `rdwmctl check-config` on a scratch copy; without rdwmctl only the TOML syntax is checked.
pub fn validate(rdwmctl: &Path, target: &Path, text: &str) -> Result<(), String> {
    Cfg::parse(text).map_err(|e| format!("invalid TOML: {e}"))?;
    let dir = std::fs::canonicalize(target).ok().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(|| target.parent().map(Path::to_path_buf)).unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    // One owned scratch file per check (create_new, private): two validators never read each other's text.
    static CHECK: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(".decsettings-check-{}-{}.toml", std::process::id(), CHECK.fetch_add(1, Ordering::Relaxed)));
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", tmp.display())
    })?;
    let out = Command::new(rdwmctl).arg("check-config").arg(&tmp).output();
    let _ = std::fs::remove_file(&tmp);
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let msg = msg.trim_start_matches("rdwmctl: ").trim_start_matches("config error: ");
            Err(msg.replace(&tmp.display().to_string(), "config.toml"))
        }
        // rdwmctl missing: syntax-only validation (rdwm itself still rejects bad values on reload)
        Err(_) => Ok(()),
    }
}

// ---------------------------------------------------------------- JSON files (decebal)

pub fn json_load(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(Value::is_object).unwrap_or_else(|| Value::Object(Default::default()))
}

/// Sets (or with `None` removes) a top-level key, keeping every other key and the key order.
pub fn json_set(path: &Path, key: &str, v: Option<Value>) -> Result<(), String> {
    json_update(path, key, v, |_, _| Ok(())).map(|_| ())
}

/// [`json_set`] as a compare-and-swap under the file lock: when the file changes between the read and the rename
/// (someone saved it during a slow fsync), it is read again and the one key applied again, so their keys and ours
/// both survive. `guard` sees the document and its identity before every attempt and may refuse (a conflict on the
/// same key). A file that is not a JSON object right now is never overwritten. Returns the committed document and
/// its identity.
pub fn json_update(path: &Path, key: &str, v: Option<Value>, guard: impl Fn(&Value, Stamp) -> Result<(), String>) -> Result<(Value, Stamp), String> {
    with_file_lock(path, |t| {
        let name = t.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut attempt = 0;
        loop {
            let before = stamp(t);
            let mut doc = match std::fs::read_to_string(t) {
                Ok(s) if s.trim().is_empty() => Value::Object(Default::default()),
                Ok(s) => serde_json::from_str::<Value>(&s).ok().filter(Value::is_object).ok_or_else(|| format!("{name} is not a JSON object right now; not overwriting it"))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(Default::default()),
                Err(e) => return Err(format!("{}: {e}", t.display())),
            };
            guard(&doc, before)?;
            let obj = doc.as_object_mut().expect("object");
            match &v {
                Some(v) => {
                    obj.insert(key.to_string(), v.clone());
                }
                None => {
                    obj.shift_remove(key);
                }
            }
            let mut text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
            text.push('\n');
            crate::log_debug!("json set {name} key `{key}`");
            match write_locked(t, &text, before) {
                Ok(()) => return Ok((doc, stamp(t))),
                Err(e) if is_changed_on_disk(&e) && attempt < 3 => {
                    attempt += 1;
                    crate::log_info!("{name} changed on disk while saving `{key}`; applying it again on top (attempt {attempt})");
                }
                Err(e) => return Err(e),
            }
        }
    })
}

// ---------------------------------------------------------------- IPC

/// One request/reply round trip on rdwm's socket. Short timeouts: the UI must never hang on it.
pub fn ipc_call(sock: &Path, req: &Value) -> Result<Value, String> {
    ipc_call_inner(sock, req, true)
}

fn ipc_call_inner(sock: &Path, req: &Value, warn: bool) -> Result<Value, String> {
    let mut s = UnixStream::connect(sock).map_err(|e| {
        crate::log_trace!("rdwm socket {}: {e}", sock.display());
        e.to_string()
    })?;
    crate::log_trace!("rdwm ipc: {req}");
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(1500)));
    writeln!(s, "{req}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if v["ok"] == Value::Bool(false) {
        if warn {
            crate::log_warn!("rdwm ipc {req} failed: {}", v["error"].as_str().unwrap_or("error"));
        }
        return Err(v["error"].as_str().unwrap_or("error").to_string());
    }
    Ok(v)
}

// ---------------------------------------------------------------- watcher thread

/// What the background thread reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    /// One of the watched files changed (stamp differs from the last report).
    FilesChanged,
    /// rdwm's runtime view changed (or appeared / vanished).
    Runtime(Box<Runtime>),
    /// Result of a `reload` request to rdwm.
    Reload(Result<(), String>),
    /// The persistence worker finished a job (in submission order).
    Persisted(Box<super::persist::Done>),
}

/// Explicit support advertised by the live compositor, never inferred from schema keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effects {
    pub protocol: u32,
    pub backend: String,
    pub instance: String,
    pub ssd_shadows: bool,
}

impl Effects {
    fn from_reply(reply: &Value) -> Option<Self> {
        let instance = reply["instance"].as_str()?;
        let effects = &reply["effects"];
        if reply["ok"] != true
            || reply["protocol"] != 1
            || !matches!(reply["backend"].as_str()?, "nested-pixman" | "nested-gles" | "drm-pixman" | "drm-gles")
            || instance.is_empty()
            || instance.len() > 128
            || instance.chars().any(char::is_control)
            || effects["protocol"] != 1
            || effects["backend"] != "wayland"
        {
            return None;
        }
        Some(Self { protocol: 1, backend: "wayland".into(), instance: instance.into(), ssd_shadows: effects["ssd_shadows"].as_bool()? })
    }
    pub fn supports_ssd_shadows(&self) -> bool {
        self.protocol == 1 && self.backend == "wayland" && !self.instance.is_empty() && self.ssd_shadows
    }
}

#[derive(Debug, Clone, Default)]
pub struct Runtime {
    pub running: bool,
    pub state: Value,
    pub config: Value,
    pub binds: Value,
    /// Error text of the last failed reload, if rdwm still holds one.
    pub error: Option<String>,
    pub version: Option<String>,
    pub effects: Option<Effects>,
    /// The `decoration.style` names the running rdwm itself listed in its `schema` reply; `None` when it is not running or could not say (an older rdwm).
    pub decoration_styles: Option<Vec<String>>,
    /// Monotonic request start. Excluded from equality so idle polls do not repaint.
    pub observed: u64,
}

impl PartialEq for Runtime {
    fn eq(&self, other: &Self) -> bool {
        self.running == other.running
            && self.state == other.state
            && self.config == other.config
            && self.binds == other.binds
            && self.error == other.error
            && self.version == other.version
            && self.effects == other.effects
            && self.decoration_styles == other.decoration_styles
    }
}

pub type Queue = Arc<Mutex<VecDeque<Ev>>>;

pub fn poll_runtime(sock: &Path) -> Runtime {
    poll_runtime_for(sock, std::env::var_os("WAYLAND_DISPLAY").is_some_and(|s| !s.is_empty()))
}

fn poll_runtime_for(sock: &Path, wayland: bool) -> Runtime {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let observed = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let offline = || Runtime { observed, ..Runtime::default() };
    let before = stamp(sock);
    let Ok(state) = ipc_call(sock, &serde_json::json!({"cmd":"get_state"})) else { return offline() };
    let config = ipc_call(sock, &serde_json::json!({"cmd":"get_config"})).unwrap_or(Value::Null);
    let binds = ipc_call(sock, &serde_json::json!({"cmd":"bind","op":"list"})).unwrap_or(Value::Null);
    // This executes on the existing watcher/reload worker (or before startup),
    // never from a paint or input handler. Older compositors failing this probe
    // are normal; do not emit a warning every second.
    let effects = if wayland { ipc_call_inner(sock, &serde_json::json!({"cmd":"output_query"}), false).ok().as_ref().and_then(Effects::from_reply) } else { None };
    // What the RUNNING rdwm can draw, from its own answer (the installed rdwmctl's offline table can describe a different build).
    let decoration_styles =
        ipc_call_inner(sock, &serde_json::json!({"cmd":"schema"}), false).ok().as_ref().and_then(crate::theme::engine::decoration_compat::styles_of).map(|set| set.into_iter().collect::<Vec<_>>());
    // A replacement socket cannot combine one compositor's config with another's effects.
    if before.0.is_none() || stamp(sock) != before {
        return offline();
    }
    Runtime {
        running: true,
        version: config["version"].as_str().map(String::from),
        error: config["error"].as_str().map(String::from),
        state: state["state"].clone(),
        config: config["config"].clone(),
        binds: binds["binds"].clone(),
        effects,
        decoration_styles,
        observed,
    }
}

/// Coalesce runtime heartbeats without discarding file/reload events. An older
/// reload worker must not overwrite a newer watcher result or disconnect.
pub fn queue_runtime(queue: &Queue, rt: Runtime) {
    let mut queue = queue.lock().unwrap();
    if queue.iter().any(|ev| matches!(ev, Ev::Runtime(newer) if newer.observed > rt.observed)) {
        return;
    }
    queue.retain(|ev| !matches!(ev, Ev::Runtime(_)));
    queue.push_back(Ev::Runtime(Box::new(rt)));
}

/// Polls file stamps every 500 ms and rdwm's state every second; wakes the UI only on change.
/// Costs two `stat` calls per tick while nothing happens. Ends once the window's queue is gone.
pub fn spawn_watcher(files: Vec<PathBuf>, sock: PathBuf, queue: Queue, wake: Arc<dyn Fn() + Send + Sync>) {
    let queue: Weak<Mutex<VecDeque<Ev>>> = Arc::downgrade(&queue);
    std::thread::Builder::new()
        .name("decsettings-watch".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut stamps: Vec<Stamp> = files.iter().map(|f| stamp(f)).collect();
            let mut last: Option<Runtime> = None;
            let mut n = 0u32;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if queue.strong_count() == 0 {
                    return;
                }
                n = n.wrapping_add(1);
                let now: Vec<Stamp> = files.iter().map(|f| stamp(f)).collect();
                if now != stamps {
                    stamps = now;
                    let Some(queue) = queue.upgrade() else { return };
                    queue.lock().unwrap().push_back(Ev::FilesChanged);
                    wake();
                }
                if n % 2 == 0 {
                    let rt = poll_runtime(&sock);
                    let changed = last.as_ref() != Some(&rt);
                    last = Some(rt.clone());
                    let Some(queue) = queue.upgrade() else { return };
                    queue_runtime(&queue, rt);
                    if changed {
                        wake();
                    }
                }
            }
        })
        .ok();
}

// ---------------------------------------------------------------- small helpers

pub fn spawn_detached(cmd: &str, args: &[&str]) -> bool {
    use std::process::Stdio;
    crate::log_debug!("spawn {cmd} {args:?}");
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut c| {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        })
        .inspect_err(|e| crate::log_warn!("spawn failed: {e}"))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decsettings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn effects_reply() -> Value {
        serde_json::json!({"ok":true,"protocol":1,"instance":"fixture-1","backend":"nested-pixman",
            "effects":{"protocol":1,"backend":"wayland","ssd_shadows":true,"csd_shadows":false}})
    }

    #[test]
    fn effects_require_explicit_current_protocol_and_backend() {
        let reply = effects_reply();
        assert!(Effects::from_reply(&reply).unwrap().supports_ssd_shadows());
        for broken in [
            serde_json::json!({"ok":true}),
            {
                let mut v = reply.clone();
                v["effects"] = Value::Null;
                v
            },
            {
                let mut v = reply.clone();
                v["effects"]["protocol"] = 2.into();
                v
            },
            {
                let mut v = reply.clone();
                v["protocol"] = 2.into();
                v
            },
            {
                let mut v = reply.clone();
                v["instance"] = "".into();
                v
            },
            {
                let mut v = reply.clone();
                v["backend"] = "x11".into();
                v
            },
            {
                let mut v = reply.clone();
                v["effects"]["backend"] = "x11".into();
                v
            },
            {
                let mut v = reply.clone();
                v["effects"]["ssd_shadows"] = "yes".into();
                v
            },
        ] {
            assert!(Effects::from_reply(&broken).is_none(), "{broken}");
        }
        let mut disabled = reply;
        disabled["effects"]["ssd_shadows"] = false.into();
        assert!(!Effects::from_reply(&disabled).unwrap().supports_ssd_shadows());
    }

    fn runtime_server(path: &Path, effects: Value, replace_socket: bool) -> std::thread::JoinHandle<()> {
        use std::os::unix::net::UnixListener;
        let listener = UnixListener::bind(path).unwrap();
        let path = path.to_owned();
        std::thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut text = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut text).unwrap();
                let request: Value = serde_json::from_str(&text).unwrap();
                let reply = match request["cmd"].as_str().unwrap() {
                    "get_state" => serde_json::json!({"ok":true,"state":{"fixture":true}}),
                    "get_config" => serde_json::json!({"ok":true,"config":{"decoration":{"shadow":false}}}),
                    "bind" => serde_json::json!({"ok":true,"binds":[]}),
                    "output_query" => {
                        if replace_socket {
                            std::fs::remove_file(&path).unwrap();
                            let _replacement = UnixListener::bind(&path).unwrap();
                        }
                        effects.clone()
                    }
                    other => panic!("unexpected request {other}"),
                };
                writeln!(stream, "{reply}").unwrap();
            }
        })
    }

    #[test]
    fn runtime_worker_clears_old_unknown_and_disconnected_effects() {
        let root = tmpdir("runtime-effects");
        let sock = root.join("wm.sock");
        let server = runtime_server(&sock, effects_reply(), false);
        let supported = poll_runtime_for(&sock, true);
        server.join().unwrap();
        assert!(supported.running);
        assert!(supported.effects.unwrap().supports_ssd_shadows());
        std::fs::remove_file(&sock).unwrap();
        let server = runtime_server(&sock, serde_json::json!({"ok":false,"error":"unknown command"}), false);
        let old = poll_runtime_for(&sock, true);
        server.join().unwrap();
        assert!(old.running);
        assert!(old.effects.is_none(), "an older compositor may parse config keys but cannot render this effect");
        std::fs::remove_file(&sock).unwrap();
        let disconnected = poll_runtime_for(&sock, true);
        assert!(!disconnected.running);
        assert!(disconnected.effects.is_none());
        assert!(disconnected.observed > old.observed);
        let server = runtime_server(&sock, effects_reply(), true);
        let replaced = poll_runtime_for(&sock, true);
        server.join().unwrap();
        assert!(!replaced.running, "do not combine different socket instances");
        assert!(replaced.effects.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_heartbeats_coalesce_and_reject_old_reload_results() {
        let queue: Queue = Default::default();
        queue.lock().unwrap().push_back(Ev::FilesChanged);
        queue_runtime(&queue, Runtime { running: true, observed: 2, ..Runtime::default() });
        queue_runtime(&queue, Runtime { running: false, observed: 4, ..Runtime::default() });
        queue_runtime(&queue, Runtime { running: true, observed: 3, ..Runtime::default() });
        let queue = queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0], Ev::FilesChanged);
        assert!(matches!(&queue[1], Ev::Runtime(rt) if rt.observed == 4 && !rt.running));
        assert_eq!(Runtime { observed: 1, ..Runtime::default() }, Runtime { observed: 2, ..Runtime::default() }, "observations alone must not repaint");
    }

    #[test]
    fn atomic_write_makes_one_backup_and_keeps_mode() {
        let d = tmpdir("atomic");
        let p = d.join("config.toml");
        std::fs::write(&p, "orig = 1\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&p, "a = 1\n").unwrap();
        write_atomic(&p, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a = 2\n");
        assert_eq!(std::fs::read_to_string(d.join("config.toml.bak-decsettings")).unwrap(), "orig = 1\n", "backup is the first original only");
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o640);
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(!left.iter().any(|n| n.ends_with(".tmp")), "{left:?}");
    }

    #[test]
    fn symlinked_config_is_written_through() {
        let d = tmpdir("symlink");
        std::fs::create_dir_all(d.join("dots")).unwrap();
        std::fs::write(d.join("dots/real.toml"), "x = 1\n").unwrap();
        std::os::unix::fs::symlink(d.join("dots/real.toml"), d.join("config.toml")).unwrap();
        write_atomic(&d.join("config.toml"), "x = 2\n").unwrap();
        assert!(std::fs::symlink_metadata(d.join("config.toml")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(d.join("dots/real.toml")).unwrap(), "x = 2\n");
    }

    #[test]
    fn commit_validates_and_detects_external_edits() {
        let d = tmpdir("commit");
        let p = d.join("config.toml");
        std::fs::write(&p, "# hi\n[general]\nborder_px = 2\n").unwrap();
        let mut s = Store::load(&p, Path::new("/nonexistent/rdwmctl"));
        let mut c = s.cfg.clone();
        c.set("general", "border_px", &super::super::tomlcfg::Val::Int(5)).unwrap();
        assert_eq!(s.commit(&c), Ok(true));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# hi\n[general]\nborder_px = 5\n");
        assert_eq!(s.commit(&c), Ok(false), "no change, no write");
        // external edit: next commit refuses to clobber
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&p, "# hi\n[general]\nborder_px = 9\n").unwrap();
        let mut c2 = s.cfg.clone();
        c2.set("general", "gap_inner", &super::super::tomlcfg::Val::Int(3)).unwrap();
        assert!(s.commit(&c2).unwrap_err().contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# hi\n[general]\nborder_px = 9\n");
        assert_eq!(s.check_external(), External::Reloaded);
        assert_eq!(s.cfg.get("general", "border_px"), Some(super::super::tomlcfg::Val::Int(9)));
        assert_eq!(s.check_external(), External::Unchanged);
    }

    #[test]
    fn half_written_external_file_keeps_the_old_document() {
        let d = tmpdir("partial");
        let p = d.join("config.toml");
        std::fs::write(&p, "[general]\nborder_px = 2\n").unwrap();
        let mut s = Store::load(&p, Path::new("/nonexistent"));
        std::fs::write(&p, "[general]\nborder_px =").unwrap();
        assert!(matches!(s.check_external(), External::Invalid(_)));
        assert_eq!(s.cfg.get("general", "border_px"), Some(super::super::tomlcfg::Val::Int(2)));
        std::fs::write(&p, "[general]\nborder_px = 7\n").unwrap();
        assert_eq!(s.check_external(), External::Reloaded);
        assert_eq!(s.cfg.get("general", "border_px"), Some(super::super::tomlcfg::Val::Int(7)));
    }

    #[test]
    fn simultaneous_validations_each_check_their_own_text() {
        let d = tmpdir("validators");
        let target = d.join("config.toml");
        let ctl = d.join("rdwmctl");
        std::fs::write(&ctl, "#!/bin/sh\nsleep 0.1\nif grep -q bad \"$2\"; then echo 'config error: bad value' >&2; exit 1; fi\nexit 0\n").unwrap();
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let run = |text: &'static str| {
            let (c, t) = (ctl.clone(), target.clone());
            std::thread::spawn(move || validate(&c, &t, text))
        };
        let threads: Vec<_> = (0..6).map(|i| (i % 2 == 0, run(if i % 2 == 0 { "bad = 1\n" } else { "good = 1\n" }))).collect();
        for (bad, t) in threads {
            let r = t.join().unwrap();
            assert_eq!(r.is_err(), bad, "{r:?}");
        }
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains("check")).collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn invalid_toml_is_not_written() {
        assert!(validate(Path::new("/nonexistent"), Path::new("/tmp/x.toml"), "a = ").is_err());
    }

    #[test]
    fn json_set_keeps_unknown_keys_and_order() {
        let d = tmpdir("json");
        let p = d.join("dock-settings.json");
        std::fs::write(&p, "{\"zeta\": [1,2], \"appSpacing\": 6, \"alpha\": {\"x\": 1}}").unwrap();
        json_set(&p, "appSpacing", Some(serde_json::json!(9))).unwrap();
        let v = json_load(&p);
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["zeta", "appSpacing", "alpha"]);
        assert_eq!(v["appSpacing"], 9);
        assert_eq!(v["alpha"]["x"], 1);
        json_set(&p, "zeta", None).unwrap();
        assert!(json_load(&p).get("zeta").is_none());
    }
}
