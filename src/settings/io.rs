//! Files, validation, atomic writes, rdwm IPC and the background watcher.

use super::tomlcfg::Cfg;
use serde_json::Value;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub const BACKUP_SUFFIX: &str = "bak-macsettings";

// ---------------------------------------------------------------- paths

#[derive(Debug, Clone)]
pub struct Paths {
    pub rdwm_config: PathBuf,
    /// `~/.config/omarchy` (or `$MACSHELL_CONFIG`): dock-settings.json etc.
    pub shell_dir: PathBuf,
    pub rdwmctl: PathBuf,
    /// rdwm's IPC socket (`RDWM_SOCK`, else `$XDG_RUNTIME_DIR/rdwm.sock`).
    pub socket: PathBuf,
}

impl Paths {
    pub fn discover() -> Paths {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let rdwm_config = std::env::var_os("MACSETTINGS_CONFIG").or_else(|| std::env::var_os("RDWM_CONFIG")).map(PathBuf::from).unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(|x| PathBuf::from(x).join("rdwm/config.toml")).unwrap_or_else(|| home.join(".config/rdwm/config.toml"))
        });
        let socket = match std::env::var_os("RDWM_SOCK").filter(|s| !s.is_empty()) {
            Some(p) => PathBuf::from(p),
            None => std::env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("rdwm.sock")).unwrap_or_else(|| PathBuf::from("/nonexistent/rdwm.sock")),
        };
        Paths { rdwm_config, shell_dir: crate::config::config_dir(), rdwmctl: std::env::var_os("RDWMCTL").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("rdwmctl")), socket }
    }

    pub fn dock_settings(&self) -> PathBuf {
        self.shell_dir.join("dock-settings.json")
    }
    pub fn dock_pinned(&self) -> PathBuf {
        self.shell_dir.join("dock-pinned-macos.json")
    }
    pub fn shell_json(&self) -> PathBuf {
        self.shell_dir.join("macshell.json")
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

pub fn stamp(path: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    Stamp(std::fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len(), m.ino()))))
}

/// Writes `text` next to `path` and renames it over the target (following a symlinked target so
/// dotfile managers keep working). Keeps the permissions of an existing file, makes a one-time
/// `<name>.bak-macsettings` of the original, and fsyncs before the rename.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = target.parent().ok_or("config path has no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if target.exists() {
        let bak = dir.join(format!("{name}.{BACKUP_SUFFIX}"));
        if !bak.exists() {
            std::fs::copy(&target, &bak).map_err(|e| format!("backup {}: {e}", bak.display()))?;
        }
    }
    let tmp = dir.join(format!(".{name}.macsettings-{}.tmp", std::process::id()));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        if let Ok(m) = std::fs::metadata(&target) {
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(m.permissions().mode() & 0o7777));
        }
        std::fs::rename(&tmp, &target)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", target.display()));
    }
    Ok(())
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

pub struct Store {
    pub path: PathBuf,
    pub cfg: Cfg,
    pub text: String,
    stamp: Stamp,
    pub rdwmctl: PathBuf,
}

impl Store {
    pub fn load(path: &Path, rdwmctl: &Path) -> Store {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let cfg = Cfg::parse(&text).unwrap_or_default();
        Store { path: path.to_path_buf(), cfg, text, stamp: stamp(path), rdwmctl: rdwmctl.to_path_buf() }
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
            // someone edited the file since we last read it: do not clobber, pick up their version
            return Err("config.toml changed on disk; reloaded it, please repeat the change".into());
        }
        validate(&self.rdwmctl, &self.path, &text)?;
        write_atomic(&self.path, &text)?;
        self.cfg = new.clone();
        self.text = text;
        self.stamp = stamp(&self.path);
        Ok(true)
    }

    pub fn restore_backup(&mut self) -> Result<(), String> {
        let bak = self.path.with_file_name(format!("{}.{BACKUP_SUFFIX}", self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        let text = std::fs::read_to_string(&bak).map_err(|e| format!("{}: {e}", bak.display()))?;
        let cfg = Cfg::parse(&text)?;
        validate(&self.rdwmctl, &self.path, &text)?;
        write_atomic(&self.path, &text)?;
        self.cfg = cfg;
        self.text = text;
        self.stamp = stamp(&self.path);
        Ok(())
    }
}

/// Runs `rdwmctl check-config` on a scratch copy; without rdwmctl only the TOML syntax is checked.
pub fn validate(rdwmctl: &Path, target: &Path, text: &str) -> Result<(), String> {
    Cfg::parse(text).map_err(|e| format!("invalid TOML: {e}"))?;
    let dir = std::fs::canonicalize(target).ok().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(|| target.parent().map(Path::to_path_buf)).unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join(format!(".macsettings-check-{}.toml", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
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

// ---------------------------------------------------------------- JSON files (macshell)

pub fn json_load(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(Value::is_object).unwrap_or_else(|| Value::Object(Default::default()))
}

/// Sets (or with `None` removes) a top-level key, keeping every other key and the key order.
pub fn json_set(path: &Path, key: &str, v: Option<Value>) -> Result<(), String> {
    let mut doc = json_load(path);
    let obj = doc.as_object_mut().ok_or("not an object")?;
    match v {
        Some(v) => {
            obj.insert(key.to_string(), v);
        }
        None => {
            obj.shift_remove(key);
        }
    }
    let mut text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    text.push('\n');
    write_atomic(path, &text)
}

// ---------------------------------------------------------------- IPC

/// One request/reply round trip on rdwm's socket. Short timeouts: the UI must never hang on it.
pub fn ipc_call(sock: &Path, req: &Value) -> Result<Value, String> {
    let mut s = UnixStream::connect(sock).map_err(|e| e.to_string())?;
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(1500)));
    writeln!(s, "{req}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if v["ok"] == Value::Bool(false) {
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
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Runtime {
    pub running: bool,
    pub state: Value,
    pub config: Value,
    pub binds: Value,
    /// Error text of the last failed reload, if rdwm still holds one.
    pub error: Option<String>,
    pub version: Option<String>,
}

pub type Queue = Arc<Mutex<VecDeque<Ev>>>;

pub fn poll_runtime(sock: &Path) -> Runtime {
    let Ok(state) = ipc_call(sock, &serde_json::json!({"cmd":"get_state"})) else { return Runtime::default() };
    let config = ipc_call(sock, &serde_json::json!({"cmd":"get_config"})).unwrap_or(Value::Null);
    let binds = ipc_call(sock, &serde_json::json!({"cmd":"bind","op":"list"})).unwrap_or(Value::Null);
    Runtime {
        running: true,
        version: config["version"].as_str().map(String::from),
        error: config["error"].as_str().map(String::from),
        state: state["state"].clone(),
        config: config["config"].clone(),
        binds: binds["binds"].clone(),
    }
}

/// Polls file stamps every 500 ms and rdwm's state every second; wakes the UI only on change.
/// Costs two `stat` calls per tick while nothing happens.
pub fn spawn_watcher(files: Vec<PathBuf>, sock: PathBuf, queue: Queue, wake: Arc<dyn Fn() + Send + Sync>) {
    std::thread::Builder::new()
        .name("macsettings-watch".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut stamps: Vec<Stamp> = files.iter().map(|f| stamp(f)).collect();
            let mut last: Option<Runtime> = None;
            let mut n = 0u32;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                n = n.wrapping_add(1);
                let now: Vec<Stamp> = files.iter().map(|f| stamp(f)).collect();
                if now != stamps {
                    stamps = now;
                    queue.lock().unwrap().push_back(Ev::FilesChanged);
                    wake();
                }
                if n % 2 == 0 {
                    let rt = poll_runtime(&sock);
                    if last.as_ref() != Some(&rt) {
                        last = Some(rt.clone());
                        queue.lock().unwrap().push_back(Ev::Runtime(Box::new(rt)));
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
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("macsettings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
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
        assert_eq!(std::fs::read_to_string(d.join("config.toml.bak-macsettings")).unwrap(), "orig = 1\n", "backup is the first original only");
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
