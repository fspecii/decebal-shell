//! One debug log for every Decebal program (decsettings, decbar, decdock, decwidgets, decthemectl, ...).
//!
//! Each program writes to its own file, `<log dir>/<program>.log` (default `~/.local/state/decebal/logs/`, override with
//! `$DECEBAL_LOG_DIR`); a file is rotated to `<program>.log.1` at 2 MiB. Nothing is written until the first record that passes
//! the level, so a quiet program leaves no file.
//!
//! * Level: `$DECEBAL_LOG` = `off|error|warn|info|debug|trace` (`1`/`true`/`on` mean `debug`), else `decebal.json`
//!   `"debug": {"level": "debug"}` (or `"debug": true`), else `warn`. Set the persistent one with `decthemectl log level debug`;
//!   it applies to programs started afterwards.
//! * `$DECEBAL_LOG_STDERR=1` also prints every record to stderr.
//! * A panic is always recorded (message, place, backtrace) unless the level is `off`.
//! * Read it with `decthemectl log [path|tail [N] [PROGRAM]|clear|level LEVEL]`.
//!
//! Use the macros: `log_error!`, `log_warn!`, `log_info!`, `log_debug!`, `log_trace!` (format arguments are only evaluated when the
//! level is enabled). Never log secrets, clipboard text or file contents: names, paths, counts and results only.

use std::fmt::Arguments;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "0" | "false" | "no" => Level::Off,
            "error" => Level::Error,
            "warn" | "warning" => Level::Warn,
            "info" => Level::Info,
            "debug" | "1" | "true" | "on" | "yes" => Level::Debug,
            "trace" | "all" => Level::Trace,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        ["off", "error", "warn", "info", "debug", "trace"][self as usize]
    }
    fn tag(self) -> &'static str {
        ["OFF  ", "ERROR", "WARN ", "INFO ", "DEBUG", "TRACE"][self as usize]
    }
    fn from_u8(n: u8) -> Level {
        [Level::Off, Level::Error, Level::Warn, Level::Info, Level::Debug, Level::Trace][(n as usize).min(5)]
    }
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Warn as u8);
static PROGRAM: OnceLock<String> = OnceLock::new();
static SINK: Mutex<Sink> = Mutex::new(Sink { file: None, bytes: 0, failed: false });
const ROTATE_AT: u64 = 2 * 1024 * 1024;

struct Sink {
    file: Option<std::fs::File>,
    bytes: u64,
    failed: bool,
}

/// True when a record of `level` would be written. The macros call this first.
#[inline]
pub fn enabled(level: Level) -> bool {
    level != Level::Off && level as u8 <= LEVEL.load(Ordering::Relaxed)
}

pub fn level() -> Level {
    Level::from_u8(LEVEL.load(Ordering::Relaxed))
}

pub fn set_level(l: Level) {
    LEVEL.store(l as u8, Ordering::Relaxed);
}

/// The program name for the file: the executable's base name, legacy `mac*` mapped to `dec*`.
pub fn program() -> &'static str {
    PROGRAM.get_or_init(|| {
        let a0 = std::env::args().next().unwrap_or_default();
        let base = a0.rsplit('/').next().unwrap_or("decebal").to_string();
        match base.strip_prefix("mac") {
            Some(rest) if crate::compat::PROGRAMS.contains(&rest.split('-').next().unwrap_or("")) => format!("dec{rest}"),
            _ if base.is_empty() => "decebal".into(),
            _ => base,
        }
    })
}

/// The log folder: `$DECEBAL_LOG_DIR`, else `$XDG_STATE_HOME/decebal/logs`, else `~/.local/state/decebal/logs`.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("DECEBAL_LOG_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if cfg!(test) {
        return std::env::temp_dir().join("decebal-test-logs"); // unit tests never write into the user's state folder
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    state.join("decebal/logs")
}

pub fn path_for(program: &str) -> PathBuf {
    dir().join(format!("{program}.log"))
}

/// The level the settings ask for: env, then `decebal.json`, then `warn`.
pub fn configured_level() -> Level {
    if let Some(l) = std::env::var("DECEBAL_LOG").ok().and_then(|s| Level::parse(&s)) {
        return l;
    }
    let text = std::fs::read_to_string(crate::config::config_dir().join("decebal.json")).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    match v.get("debug") {
        Some(serde_json::Value::Bool(b)) => {
            if *b {
                Level::Debug
            } else {
                Level::Warn
            }
        }
        Some(d) => d.get("level").and_then(|l| l.as_str()).and_then(Level::parse).unwrap_or(Level::Warn),
        None => Level::Warn,
    }
}

/// Called by [`crate::compat::init`], i.e. first thing in every program's `main`. Reads the level, writes a start record at
/// `debug`, and installs a panic hook that logs the panic before the previous hook runs.
pub fn init() {
    set_level(configured_level());
    if enabled(Level::Debug) {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let env: Vec<String> =
            ["DISPLAY", "WAYLAND_DISPLAY", "XDG_SESSION_TYPE", "DECEBAL_BACKEND", "DECEBAL_CONFIG"].iter().filter_map(|k| std::env::var(k).ok().map(|v| format!("{k}={v}"))).collect();
        write_record(Level::Debug, "start", format_args!("{} {} args={args:?} {}", program(), env!("CARGO_PKG_VERSION"), env.join(" ")));
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if level() != Level::Off {
            let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
            let bt = std::backtrace::Backtrace::force_capture();
            write_record(Level::Error, "panic", format_args!("{msg} at {place}\n{bt}"));
        }
        prev(info);
    }));
}

/// Formats `YYYY-MM-DD HH:MM:SS.mmm` in local time.
fn timestamp() -> String {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: plain libc calls on stack values
    unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&ts.tv_sec, &mut tm);
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec, ts.tv_nsec / 1_000_000)
    }
}

/// The text of one record (without the newline).
pub fn format_line(time: &str, level: Level, pid: u32, program: &str, target: &str, msg: &str) -> String {
    format!("{time} {} {pid} {program} {target}: {msg}", level.tag())
}

fn open(sink: &mut Sink) {
    if sink.file.is_some() || sink.failed {
        return;
    }
    let p = path_for(program());
    let _ = std::fs::create_dir_all(dir());
    if std::fs::metadata(&p).map(|m| m.len() >= ROTATE_AT).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
    match std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        Ok(f) => {
            sink.bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
            sink.file = Some(f);
        }
        Err(_) => sink.failed = true,
    }
}

/// Writes one record. Prefer the macros, which check the level first.
pub fn write_record(level: Level, target: &str, args: Arguments) {
    let line = format_line(&timestamp(), level, std::process::id(), program(), target, &args.to_string());
    if std::env::var_os("DECEBAL_LOG_STDERR").is_some_and(|v| !v.is_empty() && v != "0") {
        eprintln!("{line}");
    }
    let Ok(mut sink) = SINK.lock() else { return };
    open(&mut sink);
    let rotate = sink.bytes >= ROTATE_AT;
    if let Some(f) = sink.file.as_mut() {
        if writeln!(f, "{line}").is_ok() {
            sink.bytes += line.len() as u64 + 1;
        }
    }
    if rotate {
        sink.file = None;
        let p = path_for(program());
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
}

#[macro_export]
macro_rules! dlog {
    ($lvl:expr, $($arg:tt)+) => {
        if $crate::dlog::enabled($lvl) {
            $crate::dlog::write_record($lvl, module_path!(), format_args!($($arg)+));
        }
    };
}
#[macro_export]
macro_rules! log_error { ($($arg:tt)+) => { $crate::dlog!($crate::dlog::Level::Error, $($arg)+) }; }
#[macro_export]
macro_rules! log_warn { ($($arg:tt)+) => { $crate::dlog!($crate::dlog::Level::Warn, $($arg)+) }; }
#[macro_export]
macro_rules! log_info { ($($arg:tt)+) => { $crate::dlog!($crate::dlog::Level::Info, $($arg)+) }; }
#[macro_export]
macro_rules! log_debug { ($($arg:tt)+) => { $crate::dlog!($crate::dlog::Level::Debug, $($arg)+) }; }
#[macro_export]
macro_rules! log_trace { ($($arg:tt)+) => { $crate::dlog!($crate::dlog::Level::Trace, $($arg)+) }; }

/// The last `n` lines of a program's log (empty when it has none).
pub fn tail(program: &str, n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(path_for(program)).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
}

/// Programs that have a log file, sorted.
pub fn programs() -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir()).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter_map(|n| n.strip_suffix(".log").map(String::from)).collect();
    v.sort();
    v
}

/// Removes every log file (and rotated copies). Returns how many.
pub fn clear() -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(dir()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if (name.ends_with(".log") || name.ends_with(".log.1")) && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

pub const HELP: &str = "decthemectl log [status]            level, folder and the programs that have a log
decthemectl log path [PROGRAM]      the log file (default: every program's)
decthemectl log tail [N] [PROGRAM]  the last N lines (default 40) of one program, or of all of them merged by time
decthemectl log level LEVEL         off|error|warn|info|debug|trace, kept in decebal.json for programs started afterwards
decthemectl log clear               delete every log file
Per run: DECEBAL_LOG=debug decsettings   (DECEBAL_LOG_STDERR=1 also prints; DECEBAL_LOG_DIR moves the folder)";

/// `decthemectl log ...`; returns the process exit code.
pub fn cli(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("status") => {
            println!("level   {} (from {})", configured_level().as_str(), if std::env::var_os("DECEBAL_LOG").is_some() { "$DECEBAL_LOG" } else { "decebal.json, default warn" });
            println!("folder  {}", dir().display());
            for p in programs() {
                let size = std::fs::metadata(path_for(&p)).map(|m| m.len()).unwrap_or(0);
                println!("  {p:<14} {} KiB", size / 1024);
            }
            0
        }
        Some("path") => {
            match args.get(1) {
                Some(p) => println!("{}", path_for(p).display()),
                None => programs().iter().for_each(|p| println!("{}", path_for(p).display())),
            }
            0
        }
        Some("tail") => {
            let n = args.get(1).and_then(|a| a.parse::<usize>().ok());
            let prog = args.iter().skip(1).find(|a| a.parse::<usize>().is_err());
            let n = n.unwrap_or(40);
            let mut lines: Vec<String> = match prog {
                Some(p) => tail(p, n),
                // merged: the timestamp is the first 23 characters, so a plain sort orders them
                None => programs().iter().flat_map(|p| tail(p, n)).collect(),
            };
            lines.sort();
            let skip = lines.len().saturating_sub(n);
            lines[skip..].iter().for_each(|l| println!("{l}"));
            0
        }
        Some("level") => {
            let Some(l) = args.get(1).and_then(|a| Level::parse(a)) else {
                eprintln!("decthemectl log level off|error|warn|info|debug|trace");
                return 2;
            };
            let path = crate::config::config_dir().join("decebal.json");
            match crate::settings::io::json_set(&path, "debug", Some(serde_json::json!({"level": l.as_str()}))) {
                Ok(()) => {
                    println!("log level {} (programs started from now on)", l.as_str());
                    0
                }
                Err(e) => {
                    eprintln!("decthemectl: {e}");
                    1
                }
            }
        }
        Some("clear") => {
            println!("removed {} file(s)", clear());
            0
        }
        Some(_) => {
            eprintln!("{HELP}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_parse_and_order() {
        assert_eq!(Level::parse("DEBUG"), Some(Level::Debug));
        assert_eq!(Level::parse("1"), Some(Level::Debug));
        assert_eq!(Level::parse("off"), Some(Level::Off));
        assert_eq!(Level::parse("loud"), None);
        assert!(Level::Error < Level::Trace);
        for l in [Level::Off, Level::Error, Level::Warn, Level::Info, Level::Debug, Level::Trace] {
            assert_eq!(Level::parse(l.as_str()), Some(l));
        }
    }

    #[test]
    fn a_line_has_time_level_pid_program_target_and_message() {
        let l = format_line("2026-01-02 03:04:05.006", Level::Warn, 42, "decbar", "decebal::bar", "hello");
        assert_eq!(l, "2026-01-02 03:04:05.006 WARN  42 decbar decebal::bar: hello");
        assert_eq!(timestamp().len(), 23);
    }

    #[test]
    fn the_file_is_written_only_when_a_record_passes_the_level() {
        set_level(Level::Warn);
        let pid = format!(" {} ", std::process::id());
        let mine = |t: Vec<String>| t.into_iter().filter(|l| l.contains("dlog-test-marker") && l.contains(&pid)).collect::<Vec<_>>();
        crate::log_debug!("dlog-test-marker hidden {}", 1);
        crate::log_warn!("dlog-test-marker shown {}", 2);
        crate::log_error!("dlog-test-marker also shown");
        let t = mine(tail(program(), 500));
        assert_eq!(t.len(), 2, "{t:?}");
        assert!(t[0].contains("WARN") && t[0].ends_with("shown 2"), "{}", t[0]);
        assert!(programs().contains(&program().to_string()));
    }
}
