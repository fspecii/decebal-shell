//! macutil: small utilities behind the Control Center's quick toggles and the command line, ported from
//! caelestia-shell's utilities drawer (screen recorder, idle inhibitor, game mode, toasts).
//!
//! Every command works without a daemon of its own: state lives in small files in `$XDG_RUNTIME_DIR/macutil`
//! (gone at logout, like the settings they restore), the work is done with ordinary tools (`ffmpeg` / `wf-recorder`,
//! `slop`, `xset`, `picom`, `powerprofilesctl`) and every command is idempotent (`on` twice changes nothing,
//! `off` without `on` does nothing). All side effects go through [`Sys`], so the logic is unit-tested with a mock.

pub mod game;
pub mod idle;
pub mod record;

use std::path::{Path, PathBuf};
use std::time::Duration;

// ---------------------------------------------------------------- command line

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
    Toggle,
    Status,
}

impl Switch {
    fn parse(s: &str) -> Option<Switch> {
        match s {
            "on" | "enable" | "start" => Some(Switch::On),
            "off" | "disable" | "stop" => Some(Switch::Off),
            "toggle" => Some(Switch::Toggle),
            "status" => Some(Switch::Status),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Record,
    Idle,
    Game,
    Dnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Help,
    Version,
    /// `macutil status`: every feature on one line.
    All,
    Do {
        feature: Feature,
        switch: Switch,
        region: bool,
        quiet: bool,
    },
}

/// Parses the arguments after the program name.
pub fn parse_args(args: &[String]) -> Result<Cmd, String> {
    let mut words: Vec<&str> = Vec::new();
    let (mut region, mut quiet) = (false, false);
    for a in args {
        match a.as_str() {
            "-h" | "--help" | "help" => return Ok(Cmd::Help),
            "-V" | "--version" => return Ok(Cmd::Version),
            "--region" | "-r" => region = true,
            "--quiet" | "-q" => quiet = true,
            f if f.starts_with('-') => return Err(format!("unknown option {f}")),
            w => words.push(w),
        }
    }
    let feature = match words.first().copied() {
        Some("status") if words.len() == 1 => return Ok(Cmd::All),
        Some("record" | "recording") => Feature::Record,
        Some("idle-inhibit" | "idle" | "inhibit") => Feature::Idle,
        Some("gamemode" | "game-mode" | "game") => Feature::Game,
        Some("dnd" | "focus") => Feature::Dnd,
        Some(other) => return Err(format!("unknown command {other}")),
        None => return Err("missing command".into()),
    };
    let switch = match words.get(1).copied() {
        Some(w) => Switch::parse(w).ok_or_else(|| format!("{}: expected on, off, toggle or status, not {w}", words[0]))?,
        None => Switch::Status,
    };
    if words.len() > 2 {
        return Err(format!("unexpected argument {}", words[2]));
    }
    if region && feature != Feature::Record {
        return Err("--region only applies to `record`".into());
    }
    Ok(Cmd::Do { feature, switch, region, quiet })
}

// ---------------------------------------------------------------- environment and side effects

/// Where things live and what the session looks like; tests build one around a temp dir.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// `$XDG_RUNTIME_DIR/macutil`.
    pub state: PathBuf,
    /// Where recordings go (`~/Videos/Recordings`).
    pub videos: PathBuf,
    pub display: Option<String>,
    pub wayland: bool,
    /// No toast notifications.
    pub quiet: bool,
    /// How long a freshly started recorder is watched for an early failure.
    pub settle: Duration,
    /// How long a stopping recorder gets to finish the file before it is terminated.
    pub grace: Duration,
    /// Current time, unix seconds (local-time formatting is [`record::file_name`]'s business).
    pub now: u64,
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let videos = std::env::var_os("MACUTIL_RECORD_DIR").map(PathBuf::from).unwrap_or_else(|| home.join("Videos/Recordings"));
        Ctx {
            state: crate::single::runtime_file("macutil"),
            videos,
            display: std::env::var("DISPLAY").ok().filter(|d| !d.is_empty()),
            wayland: std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty()),
            quiet: std::env::var_os("MACUTIL_QUIET").is_some(),
            settle: Duration::from_millis(400),
            grace: Duration::from_secs(10),
            now: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        }
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.state.join(name)
    }

    /// Writes a state file atomically (creating the directory).
    pub fn write_state(&self, name: &str, text: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.state).map_err(|e| format!("{}: {e}", self.state.display()))?;
        let tmp = self.file(&format!("{name}.tmp"));
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, self.file(name)).map_err(|e| e.to_string())
    }

    pub fn read_state(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.file(name)).ok()
    }

    pub fn clear_state(&self, name: &str) {
        let _ = std::fs::remove_file(self.file(name));
    }
}

/// Everything that touches the outside world, so the logic above it can be tested.
pub trait Sys {
    fn have(&self, prog: &str) -> bool;
    /// Runs to completion (killed after `timeout`); the stdout when it exited with status 0.
    fn run(&self, argv: &[&str], timeout: Duration) -> Option<String>;
    /// Starts `argv` detached (own process group, output to `log`); the pid.
    fn spawn(&self, argv: &[String], log: Option<&Path>) -> Result<u32, String>;
    /// Whether the process is running (a zombie is not).
    fn alive(&self, pid: u32) -> bool;
    /// The process's command line contains `needle` (true when /proc cannot tell).
    fn cmdline_has(&self, pid: u32, needle: &str) -> bool;
    fn signal(&self, pid: u32, sig: i32);
    fn sleep(&self, d: Duration);
    /// Shows a toast through the notification daemon.
    fn notify(&self, summary: &str, body: &str, icon: &str);
    /// Do-not-disturb of whichever notification daemon runs.
    fn dnd_get(&self) -> Option<bool>;
    fn dnd_set(&self, on: bool) -> Result<(), String>;
}

pub struct RealSys;

impl Sys for RealSys {
    fn have(&self, prog: &str) -> bool {
        crate::controlcenter::sys::have(prog)
    }

    fn run(&self, argv: &[&str], timeout: Duration) -> Option<String> {
        crate::controlcenter::sys::run(argv, timeout)
    }

    fn spawn(&self, argv: &[String], log: Option<&Path>) -> Result<u32, String> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        let (prog, args) = argv.split_first().ok_or("empty command")?;
        let out = || match log.and_then(|p| std::fs::File::create(p).ok()) {
            Some(f) => Stdio::from(f),
            None => Stdio::null(),
        };
        let mut cmd = Command::new(prog);
        cmd.args(args).stdin(Stdio::null()).stdout(out()).stderr(out()).process_group(0);
        // A parent that ignores SIGINT (nohup, some launchers) would make the recorder ignore it too, and SIGINT is how it is stopped.
        // SAFETY: the closure only calls signal(2), which is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| format!("{prog}: {e}"))?;
        Ok(child.id())
    }

    fn alive(&self, pid: u32) -> bool {
        // /proc/PID/stat: "pid (comm) S ..."; a zombie (Z) has exited and only waits to be reaped.
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return false };
        stat.rsplit(')').next().and_then(|rest| rest.split_whitespace().next()).is_some_and(|s| s != "Z" && s != "X")
    }

    fn cmdline_has(&self, pid: u32, needle: &str) -> bool {
        match std::fs::read(format!("/proc/{pid}/cmdline")) {
            Ok(cmd) => String::from_utf8_lossy(&cmd).contains(needle),
            Err(_) => true,
        }
    }

    fn signal(&self, pid: u32, sig: i32) {
        if pid > 1 {
            // SAFETY: plain kill(2) on a pid macutil itself recorded.
            unsafe { libc::kill(pid as i32, sig) };
        }
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }

    fn notify(&self, summary: &str, body: &str, icon: &str) {
        if crate::notify::send("macutil", icon, summary, body).is_ok() {
            return;
        }
        if self.have("notify-send") {
            let _ = self.run(&["notify-send", "-a", "macutil", "-i", icon, summary, body], Duration::from_secs(3));
        }
    }

    fn dnd_get(&self) -> Option<bool> {
        if let Some(on) = crate::notify::running_dnd() {
            return Some(on);
        }
        // Another daemon runs (the tools answer only then); else macnotify's settings file decides what the next start does.
        if self.have("dunstctl") {
            if let Some(o) = self.run(&["dunstctl", "is-paused"], Duration::from_secs(2)) {
                return Some(o.trim() == "true");
            }
        }
        if self.have("makoctl") {
            if let Some(o) = self.run(&["makoctl", "mode"], Duration::from_secs(2)) {
                return Some(o.lines().any(|l| l.trim() == "do-not-disturb"));
            }
        }
        Some(crate::notify::Config::load().dnd)
    }

    fn dnd_set(&self, on: bool) -> Result<(), String> {
        if crate::notify::running_dnd().is_none() {
            if self.have("dunstctl") && self.run(&["dunstctl", "set-paused", if on { "true" } else { "false" }], Duration::from_secs(2)).is_some() {
                return Ok(());
            }
            if self.have("makoctl") && self.run(&["makoctl", "mode", if on { "-a" } else { "-r" }, "do-not-disturb"], Duration::from_secs(2)).is_some() {
                return Ok(());
            }
        }
        crate::notify::set_dnd(on)
    }
}

// ---------------------------------------------------------------- dispatch

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

/// Current state of a feature.
pub fn state(feature: Feature, ctx: &Ctx, sys: &dyn Sys) -> bool {
    match feature {
        Feature::Record => record::status(ctx, sys).is_some(),
        Feature::Idle => idle::status(ctx),
        Feature::Game => game::status(ctx),
        Feature::Dnd => sys.dnd_get().unwrap_or(false),
    }
}

/// Runs one command; the text to print.
pub fn execute(cmd: &Cmd, ctx: &Ctx, sys: &dyn Sys) -> Result<String, String> {
    let ctx = &match cmd {
        Cmd::Do { quiet: true, .. } => Ctx { quiet: true, ..ctx.clone() },
        _ => ctx.clone(),
    };
    match cmd {
        Cmd::Help | Cmd::Version => Ok(String::new()),
        Cmd::All => Ok([Feature::Record, Feature::Idle, Feature::Game, Feature::Dnd].into_iter().map(|f| format!("{}={}", feature_name(f), on_off(state(f, ctx, sys)))).collect::<Vec<_>>().join(" ")),
        Cmd::Do { feature, switch, region, .. } => {
            let now = state(*feature, ctx, sys);
            let want = match switch {
                Switch::Status => return Ok(on_off(now).into()),
                Switch::On => true,
                Switch::Off => false,
                Switch::Toggle => !now,
            };
            match feature {
                Feature::Record if want => record::start(ctx, sys, *region),
                Feature::Record => record::stop(ctx, sys),
                Feature::Idle if want => idle::on(ctx, sys),
                Feature::Idle => idle::off(ctx, sys),
                Feature::Game if want => game::on(ctx, sys),
                Feature::Game => game::off(ctx, sys),
                Feature::Dnd => {
                    if now != want {
                        sys.dnd_set(want)?;
                    }
                    Ok(format!("do not disturb {}", on_off(want)))
                }
            }
        }
    }
}

pub fn feature_name(f: Feature) -> &'static str {
    match f {
        Feature::Record => "record",
        Feature::Idle => "idle-inhibit",
        Feature::Game => "gamemode",
        Feature::Dnd => "dnd",
    }
}

/// Toast unless the command was quiet.
pub(crate) fn toast(ctx: &Ctx, sys: &dyn Sys, summary: &str, body: &str, icon: &str) {
    if !ctx.quiet {
        sys.notify(summary, body, icon);
    }
}

/// `path` with `$HOME` shortened to `~` (for toasts).
pub fn tilde(path: &str) -> String {
    tilde_with(path, &std::env::var("HOME").unwrap_or_default())
}

fn tilde_with(path: &str, home: &str) -> String {
    match path.strip_prefix(home) {
        Some(rest) if !home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

// ---------------------------------------------------------------- mock for tests

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};

    /// Records every call; `outputs` maps a command line (joined with spaces) to its stdout (absent = failure).
    #[derive(Default)]
    pub struct Mock {
        pub tools: RefCell<HashSet<String>>,
        pub outputs: RefCell<HashMap<String, String>>,
        pub log: RefCell<Vec<String>>,
        pub alive: RefCell<HashSet<u32>>,
        /// Pids that die when sent SIGINT.
        pub dies_on_int: Cell<bool>,
        pub next_pid: Cell<u32>,
        pub toasts: RefCell<Vec<(String, String)>>,
        pub dnd: Cell<Option<bool>>,
        pub spawn_fails: Cell<bool>,
        pub dies_at_once: Cell<bool>,
    }

    impl Mock {
        pub fn new(tools: &[&str]) -> Mock {
            let m = Mock { next_pid: Cell::new(4000), dies_on_int: Cell::new(true), ..Default::default() };
            *m.tools.borrow_mut() = tools.iter().map(|s| s.to_string()).collect();
            m
        }

        pub fn out(&self, cmd: &str, stdout: &str) {
            self.outputs.borrow_mut().insert(cmd.to_string(), stdout.to_string());
        }

        pub fn log(&self) -> Vec<String> {
            self.log.borrow().clone()
        }
    }

    impl Sys for Mock {
        fn have(&self, prog: &str) -> bool {
            self.tools.borrow().contains(prog)
        }

        fn run(&self, argv: &[&str], _t: Duration) -> Option<String> {
            let line = argv.join(" ");
            self.log.borrow_mut().push(line.clone());
            if !self.have(argv[0]) {
                return None;
            }
            self.outputs.borrow().get(&line).cloned()
        }

        fn spawn(&self, argv: &[String], _log: Option<&Path>) -> Result<u32, String> {
            self.log.borrow_mut().push(format!("spawn {}", argv.join(" ")));
            if self.spawn_fails.get() {
                return Err("no such program".into());
            }
            let pid = self.next_pid.get();
            self.next_pid.set(pid + 1);
            if !self.dies_at_once.get() {
                self.alive.borrow_mut().insert(pid);
            }
            Ok(pid)
        }

        fn alive(&self, pid: u32) -> bool {
            self.alive.borrow().contains(&pid)
        }

        fn cmdline_has(&self, _pid: u32, _needle: &str) -> bool {
            true
        }

        fn signal(&self, pid: u32, sig: i32) {
            self.log.borrow_mut().push(format!("signal {pid} {sig}"));
            if (sig == libc::SIGINT && self.dies_on_int.get()) || sig == libc::SIGTERM || sig == libc::SIGKILL {
                self.alive.borrow_mut().remove(&pid);
            }
        }

        fn sleep(&self, _d: Duration) {}

        fn notify(&self, summary: &str, body: &str, _icon: &str) {
            self.toasts.borrow_mut().push((summary.into(), body.into()));
        }

        fn dnd_get(&self) -> Option<bool> {
            self.dnd.get()
        }

        fn dnd_set(&self, on: bool) -> Result<(), String> {
            self.log.borrow_mut().push(format!("dnd {on}"));
            self.dnd.set(Some(on));
            Ok(())
        }
    }

    pub fn ctx(dir: &Path) -> Ctx {
        Ctx {
            state: dir.join("state"),
            videos: dir.join("videos"),
            display: Some(":9".into()),
            wayland: false,
            quiet: false,
            settle: Duration::ZERO,
            grace: Duration::from_millis(100),
            now: 1_700_000_000,
        }
    }

    pub fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("macutil-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}

#[cfg(test)]
mod tests {
    use super::mock::*;
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_every_documented_form() {
        let d = |feature, switch| Ok(Cmd::Do { feature, switch, region: false, quiet: false });
        assert_eq!(parse_args(&args("record start")), d(Feature::Record, Switch::On));
        assert_eq!(parse_args(&args("record stop")), d(Feature::Record, Switch::Off));
        assert_eq!(parse_args(&args("record toggle")), d(Feature::Record, Switch::Toggle));
        assert_eq!(parse_args(&args("record")), d(Feature::Record, Switch::Status));
        assert_eq!(parse_args(&args("idle-inhibit on")), d(Feature::Idle, Switch::On));
        assert_eq!(parse_args(&args("idle-inhibit status")), d(Feature::Idle, Switch::Status));
        assert_eq!(parse_args(&args("gamemode off")), d(Feature::Game, Switch::Off));
        assert_eq!(parse_args(&args("gamemode toggle")), d(Feature::Game, Switch::Toggle));
        assert_eq!(parse_args(&args("dnd toggle")), d(Feature::Dnd, Switch::Toggle));
        assert_eq!(parse_args(&args("status")), Ok(Cmd::All));
        assert_eq!(parse_args(&args("record start --region")), Ok(Cmd::Do { feature: Feature::Record, switch: Switch::On, region: true, quiet: false }));
        assert_eq!(parse_args(&args("-q gamemode on")), Ok(Cmd::Do { feature: Feature::Game, switch: Switch::On, region: false, quiet: true }));
        assert_eq!(parse_args(&args("--help")), Ok(Cmd::Help));
        assert_eq!(parse_args(&args("--version")), Ok(Cmd::Version));
    }

    #[test]
    fn rejects_bad_input() {
        for bad in ["", "frobnicate", "record maybe", "gamemode on extra", "dnd on --region", "record start --nope"] {
            assert!(parse_args(&args(bad)).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn state_files_roundtrip_and_clear() {
        let dir = tmp("state");
        let c = ctx(&dir);
        assert_eq!(c.read_state("x.json"), None);
        c.write_state("x.json", "hello").unwrap();
        assert_eq!(c.read_state("x.json").as_deref(), Some("hello"));
        c.write_state("x.json", "again").unwrap();
        assert_eq!(c.read_state("x.json").as_deref(), Some("again"));
        assert!(!c.file("x.json.tmp").exists(), "atomic write leaves no temp file");
        c.clear_state("x.json");
        assert_eq!(c.read_state("x.json"), None);
        c.clear_state("x.json");
    }

    #[test]
    fn dnd_commands_are_idempotent() {
        let dir = tmp("dnd");
        let (c, m) = (ctx(&dir), Mock::new(&[]));
        m.dnd.set(Some(false));
        let on = Cmd::Do { feature: Feature::Dnd, switch: Switch::On, region: false, quiet: false };
        assert_eq!(execute(&on, &c, &m).unwrap(), "do not disturb on");
        assert_eq!(execute(&on, &c, &m).unwrap(), "do not disturb on");
        assert_eq!(m.log(), ["dnd true"], "the second `on` changes nothing");
        let toggle = Cmd::Do { feature: Feature::Dnd, switch: Switch::Toggle, region: false, quiet: false };
        assert_eq!(execute(&toggle, &c, &m).unwrap(), "do not disturb off");
        assert_eq!(m.dnd.get(), Some(false));
        let status = Cmd::Do { feature: Feature::Dnd, switch: Switch::Status, region: false, quiet: false };
        assert_eq!(execute(&status, &c, &m).unwrap(), "off");
    }

    #[test]
    fn status_line_lists_every_feature() {
        let dir = tmp("all");
        let (c, m) = (ctx(&dir), Mock::new(&[]));
        m.dnd.set(Some(true));
        assert_eq!(execute(&Cmd::All, &c, &m).unwrap(), "record=off idle-inhibit=off gamemode=off dnd=on");
    }

    #[test]
    fn tilde_shortens_home() {
        assert_eq!(tilde_with("/home/ann/Videos/a.mp4", "/home/ann"), "~/Videos/a.mp4");
        assert_eq!(tilde_with("/tmp/a.mp4", "/home/ann"), "/tmp/a.mp4");
        assert_eq!(tilde_with("/home/annabel/a.mp4", "/home/ann"), "/home/annabel/a.mp4");
        assert_eq!(tilde_with("/a.mp4", ""), "/a.mp4");
    }
}
