//! Running external tools: a `Runner` trait (real one with a hard timeout, recording fake for tests).
//! Nothing here is ever called on the UI thread; the hub runs jobs on short-lived worker threads.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cmd {
    pub prog: String,
    pub args: Vec<String>,
    /// Per-command timeout in seconds (0 = the runner's default).
    pub secs: u64,
}

impl Cmd {
    pub fn new(prog: &str, args: &[&str]) -> Cmd {
        Cmd { prog: prog.into(), args: args.iter().map(|s| s.to_string()).collect(), secs: 0 }
    }
    pub fn owned(prog: &str, args: Vec<String>) -> Cmd {
        Cmd { prog: prog.into(), args, secs: 0 }
    }
    pub fn timeout(mut self, secs: u64) -> Cmd {
        self.secs = secs;
        self
    }
    pub fn line(&self) -> String {
        std::iter::once(self.prog.as_str()).chain(self.args.iter().map(|s| s.as_str())).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    /// The program is not installed.
    pub missing: bool,
    pub timed_out: bool,
}

impl Out {
    pub fn success(stdout: &str) -> Out {
        Out { ok: true, stdout: stdout.into(), ..Out::default() }
    }
    pub fn fail(stderr: &str) -> Out {
        Out { ok: false, stderr: stderr.into(), ..Out::default() }
    }
    pub fn not_found() -> Out {
        Out { missing: true, stderr: "command not found".into(), ..Out::default() }
    }
    /// One line explaining a failure, for inline error display.
    pub fn error_text(&self, prog: &str) -> String {
        if self.missing {
            return format!("{prog} is not installed");
        }
        if self.timed_out {
            return format!("{prog} did not answer in time");
        }
        let t = if self.stderr.trim().is_empty() { self.stdout.trim() } else { self.stderr.trim() };
        let line = t.lines().find(|l| !l.trim().is_empty()).unwrap_or("failed");
        let line = line.trim().trim_start_matches("Error: ").trim_start_matches("Error:").trim();
        let mut s: String = line.chars().take(160).collect();
        if s.is_empty() {
            s = "failed".into();
        }
        s
    }
}

pub trait Runner: Send + Sync {
    fn run(&self, c: &Cmd) -> Out;
    /// Is the program installed (on `PATH`)?
    fn exists(&self, prog: &str) -> bool;
    /// Spawn and forget (launching an editor, a terminal ...).
    fn spawn(&self, c: &Cmd) -> bool {
        self.run(c).ok
    }
}

pub fn which(prog: &str) -> Option<std::path::PathBuf> {
    if prog.contains('/') {
        let p = std::path::PathBuf::from(prog);
        return p.is_file().then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(prog)).find(|p| p.is_file())
}

pub struct RealRunner {
    pub timeout: Duration,
}

impl Default for RealRunner {
    fn default() -> Self {
        RealRunner { timeout: Duration::from_secs(8) }
    }
}

impl Runner for RealRunner {
    fn exists(&self, prog: &str) -> bool {
        which(prog).is_some()
    }

    fn run(&self, c: &Cmd) -> Out {
        let Some(path) = which(&c.prog) else { return Out::not_found() };
        let mut child = match Command::new(path).args(&c.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(c) => c,
            Err(e) => return Out::fail(&e.to_string()),
        };
        type Buf = std::sync::Arc<Mutex<Vec<u8>>>;
        let read = |mut r: Box<dyn Read + Send>| -> (Buf, Option<std::thread::JoinHandle<()>>) {
            let buf: Buf = Default::default();
            let b2 = buf.clone();
            let h = std::thread::Builder::new()
                .stack_size(256 * 1024)
                .spawn(move || {
                    let mut chunk = [0u8; 8192];
                    while let Ok(n) = r.read(&mut chunk) {
                        if n == 0 || b2.lock().unwrap().len() > (4 << 20) {
                            break;
                        }
                        b2.lock().unwrap().extend_from_slice(&chunk[..n]);
                    }
                })
                .ok();
            (buf, h)
        };
        let so = child.stdout.take().map(|s| read(Box::new(s)));
        let se = child.stderr.take().map(|s| read(Box::new(s)));
        let deadline = Instant::now() + if c.secs > 0 { Duration::from_secs(c.secs) } else { self.timeout };
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) if Instant::now() >= deadline => {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(8)),
                Err(_) => break None,
            }
        };
        // a grandchild holding the pipe open must not block us: wait briefly, then keep what arrived
        let join = |p: Option<(Buf, Option<std::thread::JoinHandle<()>>)>| -> String {
            let Some((buf, h)) = p else { return String::new() };
            let end = Instant::now() + Duration::from_millis(300);
            if let Some(h) = &h {
                while !h.is_finished() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            let b = buf.lock().unwrap();
            String::from_utf8_lossy(&b).into_owned()
        };
        Out { ok: status.is_some_and(|s| s.success()), stdout: join(so), stderr: join(se), missing: false, timed_out }
    }

    fn spawn(&self, c: &Cmd) -> bool {
        let Some(path) = which(&c.prog) else { return false };
        match Command::new(path).args(&c.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(mut ch) => {
                // reap in the background so it does not stay a zombie
                std::thread::spawn(move || {
                    let _ = ch.wait();
                });
                true
            }
            Err(_) => false,
        }
    }
}

/// Test double: answers by command line, remembers everything it ran.
#[derive(Default)]
pub struct FakeRunner {
    pub log: Mutex<Vec<Cmd>>,
    replies: Mutex<HashMap<String, Out>>,
    /// Programs that exist even without a canned reply.
    present: Mutex<Vec<String>>,
    /// Unmatched commands: Ok(empty) when true, "not found" when false.
    pub lenient: bool,
}

impl FakeRunner {
    pub fn new() -> FakeRunner {
        FakeRunner::default()
    }
    pub fn lenient() -> FakeRunner {
        FakeRunner { lenient: true, ..FakeRunner::default() }
    }
    pub fn on(self, line: &str, out: Out) -> FakeRunner {
        self.replies.lock().unwrap().insert(line.to_string(), out);
        self
    }
    pub fn ok(self, line: &str, stdout: &str) -> FakeRunner {
        self.on(line, Out::success(stdout))
    }
    pub fn have(self, prog: &str) -> FakeRunner {
        self.present.lock().unwrap().push(prog.to_string());
        self
    }
    pub fn lines(&self) -> Vec<String> {
        self.log.lock().unwrap().iter().map(|c| c.line()).collect()
    }
}

impl Runner for FakeRunner {
    fn run(&self, c: &Cmd) -> Out {
        self.log.lock().unwrap().push(c.clone());
        if let Some(o) = self.replies.lock().unwrap().get(&c.line()) {
            return o.clone();
        }
        if self.exists(&c.prog) || self.lenient {
            Out::success("")
        } else {
            Out::not_found()
        }
    }
    fn exists(&self, prog: &str) -> bool {
        self.present.lock().unwrap().iter().any(|p| p == prog) || self.replies.lock().unwrap().keys().any(|k| k.split(' ').next() == Some(prog))
    }
    fn spawn(&self, c: &Cmd) -> bool {
        self.log.lock().unwrap().push(c.clone());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_program_is_reported_not_panicked() {
        let r = RealRunner::default();
        let o = r.run(&Cmd::new("definitely-not-installed-xyz", &[]));
        assert!(o.missing && !o.ok);
        assert_eq!(o.error_text("x"), "x is not installed");
    }

    #[test]
    fn real_runner_captures_and_times_out() {
        let r = RealRunner { timeout: Duration::from_millis(300) };
        let o = r.run(&Cmd::new("sh", &["-c", "echo hi; echo oops >&2"]));
        assert!(o.ok);
        assert_eq!(o.stdout.trim(), "hi");
        assert_eq!(o.stderr.trim(), "oops");
        let t = Instant::now();
        let o = r.run(&Cmd::new("sh", &["-c", "sleep 5"]));
        assert!(o.timed_out && !o.ok);
        assert!(t.elapsed() < Duration::from_secs(2));
        // a background grandchild keeping the pipe open does not hang us
        let t = Instant::now();
        let o = r.run(&Cmd::new("sh", &["-c", "sleep 3 & echo done"]));
        assert!(o.ok && o.stdout.contains("done"));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn error_text_picks_first_line() {
        assert_eq!(Out::fail("Error: Not authorized\nmore").error_text("nmcli"), "Not authorized");
        assert_eq!(Out { timed_out: true, ..Out::default() }.error_text("nmcli"), "nmcli did not answer in time");
    }

    #[test]
    fn fake_records_and_answers() {
        let f = FakeRunner::new().ok("echo hi", "hi\n");
        assert_eq!(f.run(&Cmd::new("echo", &["hi"])).stdout, "hi\n");
        assert!(f.run(&Cmd::new("nothing", &[])).missing);
        assert_eq!(f.lines(), ["echo hi", "nothing"]);
    }
}
