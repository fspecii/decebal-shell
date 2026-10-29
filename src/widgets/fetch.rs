//! Fixed, bounded Fastfetch adapter. Theme packages select data paths, never commands or modules.
use super::data::{Data, Val};
use std::collections::HashMap;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const LIMIT: usize = 64 * 1024;
const ARGS: &[&str] = &["--config", "none", "--format", "json", "--structure", "OS:Kernel:CPU:Memory:Uptime:WM", "--logo", "none", "--detect-version", "false"];
type Values = HashMap<String, Val>;

/// At most one short-lived worker, only while a visible widget requests `fetch.*`.
pub struct Fetch {
    pending: Option<Receiver<Result<Values, String>>>,
    started: Option<Instant>,
    values: Values,
    waker: crate::app::Waker,
}
impl Fetch {
    pub fn new(waker: crate::app::Waker) -> Self {
        Self { pending: None, started: None, values: Values::new(), waker }
    }
    pub fn update(&mut self, data: &mut Data) {
        let result = self.pending.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Fastfetch worker unavailable".into())),
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = result {
            self.values = result.unwrap_or_else(|status| HashMap::from([("fetch.present".into(), Val::Bool(false)), ("fetch.status".into(), Val::Text(status))]));
            self.pending = None;
        }
        if self.pending.is_none() && self.started.is_none_or(|at| at.elapsed() >= Duration::from_secs(60)) {
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.started = Some(Instant::now());
            let waker = self.waker.clone();
            std::thread::spawn(move || {
                let _ = tx.send(collect());
                waker.wake();
            });
        }
        data.vals.retain(|path, _| !path.starts_with("fetch."));
        for (key, value) in &self.values {
            data.set(key, value.clone());
        }
        if self.values.is_empty() {
            data.set("fetch.present", Val::Bool(false));
            data.set("fetch.status", Val::Text("Loading Fastfetch…".into()));
        }
    }
}

pub fn collect() -> Result<Values, String> {
    let mut cmd = Command::new("fastfetch");
    cmd.args(ARGS);
    let start = Instant::now();
    let mut values = parse(&run(cmd, Duration::from_secs(5))?)?;
    let mut logo = Command::new("fastfetch");
    logo.args(["--config", "none", "--structure", "Break", "--logo-type", "builtin", "--logo-padding-right", "0", "--pipe", "true"]);
    let remaining = Duration::from_secs(5).saturating_sub(start.elapsed());
    match run(logo, remaining).and_then(|bytes| parse_logo(&bytes)) {
        Ok(logo) => {
            values.insert("fetch.logo".into(), Val::Text(logo));
        }
        Err(_) => {
            values.insert("fetch.status".into(), Val::Text("Fastfetch · logo unavailable".into()));
        }
    }
    Ok(values)
}

fn run(mut cmd: Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| "Fastfetch unavailable".to_string())?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let fd = stdout.as_raw_fd();
    // SAFETY: fd belongs to the live pipe; fcntl only changes this pipe's read mode.
    let nonblocking = unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0
    };
    let result = if nonblocking {
        let start = Instant::now();
        let mut out = Vec::new();
        'read: loop {
            let mut buf = [0; 4096];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if out.len() + n > LIMIT {
                            break 'read Err("Fastfetch output too large".into());
                        }
                        out.extend_from_slice(&buf[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break 'read Err("Fastfetch read failed".into()),
                }
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    // Drain bytes written between the last read and process exit.
                    match stdout.by_ref().take((LIMIT - out.len() + 1) as u64).read_to_end(&mut out) {
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => break Err("Fastfetch read failed".into()),
                    }
                    break if out.len() <= LIMIT { Ok(out) } else { Err("Fastfetch output too large".into()) };
                }
                Ok(Some(_)) | Err(_) => break Err("Fastfetch failed".into()),
                Ok(None) => {}
            }
            if start.elapsed() >= timeout {
                break Err("Fastfetch timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    } else {
        Err("Fastfetch pipe failed".into())
    };
    // Always reap the child, including output-limit, timeout and read failures.
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// Preserve the actual logo grid. Terminal escapes and oversized art are rejected.
fn parse_logo(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > LIMIT {
        return Err("Fastfetch logo too large".into());
    }
    let logo = std::str::from_utf8(bytes).map_err(|_| "Invalid Fastfetch logo".to_string())?.trim_end_matches(['\r', '\n']);
    if logo.is_empty() || logo.chars().any(|c| c.is_control() && c != '\n') || logo.lines().count() > 40 || logo.lines().any(|line| line.chars().count() > 80) {
        return Err("Invalid Fastfetch logo grid".into());
    }
    Ok(logo.into())
}

fn parse(bytes: &[u8]) -> Result<Values, String> {
    if bytes.len() > LIMIT {
        return Err("Fastfetch output too large".into());
    }
    let modules: Vec<serde_json::Value> = serde_json::from_slice(bytes).map_err(|_| "Invalid Fastfetch JSON".to_string())?;
    let mut out = Values::new();
    for module in modules {
        let Some(result) = module.get("result") else { continue };
        let mut text = |path: &str, key: &str| {
            if let Some(value) = result.get(key).and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
                out.insert(path.into(), Val::Text(value.chars().filter(|c| !c.is_control()).take(160).collect()));
            }
        };
        match module.get("type").and_then(|v| v.as_str()) {
            Some("OS") => text("fetch.os", "prettyName"),
            Some("Kernel") => text("fetch.kernel", "release"),
            Some("CPU") => {
                text("fetch.cpu", "cpu");
                text("fetch.cpu", "name");
            }
            Some("WM") => text("fetch.wm", "prettyName"),
            Some("Memory") => {
                for (key, path) in [("used", "fetch.mem_used"), ("total", "fetch.mem_total")] {
                    if let Some(n) = result.get(key).and_then(|v| v.as_u64()) {
                        out.insert(path.into(), Val::Num(n as f64));
                    }
                }
            }
            Some("Uptime") => {
                if let Some(ms) = result.get("uptime").and_then(|v| v.as_u64()) {
                    out.insert("fetch.uptime".into(), Val::Num(ms as f64 / 1000.0));
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err("Fastfetch returned no supported data".into());
    }
    out.insert("fetch.present".into(), Val::Bool(true));
    out.insert("fetch.status".into(), Val::Text("Fastfetch · refreshed every minute".into()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_results_ignore_errors_unknown_modules_and_control_characters() {
        let values = parse(br#"[{"type":"OS","result":{"prettyName":"Test\nOS"}},{"type":"CPU","result":{"name":"Test CPU"}},{"type":"Memory","result":{"used":0,"total":4096}},{"type":"Uptime","result":{"uptime":12000}},{"type":"WM","error":"not running"},{"type":"Custom","result":{"command":"do not run"}}]"#).unwrap();
        assert_eq!(values.get("fetch.os"), Some(&Val::Text("TestOS".into())));
        assert_eq!(values.get("fetch.cpu"), Some(&Val::Text("Test CPU".into())));
        assert_eq!(values.get("fetch.mem_used"), Some(&Val::Num(0.0)));
        assert_eq!(values.get("fetch.uptime"), Some(&Val::Num(12.0)));
        assert!(!values.contains_key("fetch.wm"));
        for invalid in [b"[]".as_slice(), b"{}", b"not json"] {
            assert!(parse(invalid).is_err());
        }
    }
    #[test]
    fn failed_refresh_clears_stale_values_without_starting_another_worker() {
        let mut fetch = Fetch::new(super::super::data::null_waker());
        let (tx, rx) = mpsc::channel();
        fetch.pending = Some(rx);
        fetch.started = Some(Instant::now());
        let mut data = super::super::data::sample();
        tx.send(Err("Fastfetch unavailable".into())).unwrap();
        fetch.update(&mut data);
        assert!(data.get("fetch.os").is_none());
        assert_eq!(data.get("fetch.present"), Some(&Val::Bool(false)));
        assert_eq!(data.get("fetch.status"), Some(&Val::Text("Fastfetch unavailable".into())));
        assert!(fetch.pending.is_none());
        fetch.update(&mut data);
        assert!(fetch.pending.is_none(), "rate limit must also apply to failure");
    }

    #[test]
    fn distro_logo_is_preserved_and_terminal_escape_sequences_are_rejected() {
        let fixture = include_bytes!("../../tests/fixtures/widgets/fastfetch-omarchy-logo.txt");
        let logo = parse_logo(fixture).unwrap();
        assert_eq!(logo, std::str::from_utf8(fixture).unwrap().trim_end_matches('\n'));
        assert_eq!(logo.lines().count(), 15);
        assert!(logo.contains("██              ██"));
        for invalid in ["", "\u{1b}[31mred", "tab\tlogo", &"X".repeat(81), &"X\n".repeat(41)] {
            assert!(parse_logo(invalid.as_bytes()).is_err());
        }
    }

    #[test]
    fn subprocess_failure_timeout_and_output_are_bounded() {
        assert!(run(Command::new("/nonexistent/decebal-fastfetch"), Duration::from_millis(20)).is_err());
        let mut slow = Command::new("sh");
        slow.args(["-c", "exec sleep 5"]);
        let start = Instant::now();
        assert_eq!(run(slow, Duration::from_millis(30)).unwrap_err(), "Fastfetch timed out");
        assert!(start.elapsed() < Duration::from_secs(2));
        let mut huge = Command::new("head");
        huge.args(["-c", "70000", "/dev/zero"]);
        assert_eq!(run(huge, Duration::from_secs(2)).unwrap_err(), "Fastfetch output too large");
    }
    #[test]
    #[ignore = "requires an installed Fastfetch; run explicitly for integration evidence"]
    fn installed_fastfetch_returns_real_system_data() {
        let values = collect().unwrap();
        assert!(matches!(values.get("fetch.logo"), Some(Val::Text(s)) if s.lines().count() > 1));
        assert!(matches!(values.get("fetch.os"), Some(Val::Text(s)) if !s.is_empty()));
        assert!(matches!(values.get("fetch.cpu"), Some(Val::Text(s)) if !s.is_empty()));
        assert!(matches!(values.get("fetch.mem_total"), Some(Val::Num(n)) if *n > 0.0));
        if let Some(dir) = std::env::var_os("WIDGET_EVIDENCE_DIR") {
            use crate::theme::model::{Def, Selection};
            use std::path::Path;
            let mut fonts = crate::text::Fonts::load().expect("render verification requires fonts");
            let mut data = Data::empty();
            data.vals = values;
            data.set("opt.background_opacity", Val::Num(0.6));
            let base = crate::theme::builtin::def("nord").unwrap();
            let palette = Def::parse(include_str!("../../palettes/catppuccin.toml")).unwrap();
            for dark in [true, false] {
                let th = base.with_palette(&palette).resolve("nord", dark, &Selection { opacity: Some(0.7), ..Selection::default() });
                let pm = super::super::paint::render(&super::super::samples::get("fetch-summary").unwrap(), &data, &th, &mut fonts, Path::new("."), 16).unwrap();
                let mode = if dark { "dark" } else { "light" };
                pm.save_png(Path::new(&dir).join(format!("fetch-summary-fastfetch-live-{mode}.png"))).unwrap();
            }
        }
    }
}
