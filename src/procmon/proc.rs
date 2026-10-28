//! `/proc` readers for the process list: pure parsers (fed fixture text by the tests) and a [`Scanner`] that turns two
//! consecutive scans into per-process CPU and disk rates.
//!
//! Everything is read straight from `/proc/<pid>/{stat,io,cmdline}`; the owner is the owner of the `/proc/<pid>` directory.
//! A process is identified by `(pid, start time)`, never by the pid alone: pids get reused.

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// `PF_KTHREAD`: a kernel thread.
const PF_KTHREAD: u64 = 0x0020_0000;

/// What `/proc/<pid>/stat` tells.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stat {
    pub pid: u32,
    pub comm: String,
    pub state: char,
    pub ppid: u32,
    pub flags: u64,
    pub utime: u64,
    pub stime: u64,
    pub threads: u32,
    /// Clock ticks after boot: with the pid it names one process for good.
    pub start: u64,
    pub rss_pages: u64,
}

/// Parses one `/proc/<pid>/stat` line. The command name sits in parentheses and may itself contain spaces and parentheses,
/// so the fields after it are found from the last `)`.
pub fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let pid = text[..open].trim().parse().ok()?;
    let comm = text[open + 1..close].to_string();
    // After the name: state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt utime stime cutime cstime
    // priority nice threads itrealvalue starttime vsize rss ...
    let f: Vec<&str> = text[close + 1..].split_whitespace().collect();
    let n = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
    Some(Stat {
        pid,
        comm,
        state: f.first()?.chars().next()?,
        ppid: n(1)? as u32,
        flags: n(6)?,
        utime: n(11)?,
        stime: n(12)?,
        threads: n(17).unwrap_or(1) as u32,
        start: n(19)?,
        rss_pages: n(21).unwrap_or(0),
    })
}

/// `/proc/<pid>/cmdline`: NUL-separated arguments.
pub fn parse_cmdline(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect()
}

/// Bytes read from and written to storage, from `/proc/<pid>/io`.
pub fn parse_io(text: &str) -> Option<(u64, u64)> {
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)?.trim().parse::<u64>().ok());
    Some((get("read_bytes:")?, get("write_bytes:")?))
}

/// The name to show for a process: `comm` is cut to 15 characters, so the program's own name wins when it extends it.
pub fn display_name(comm: &str, cmdline: &[String]) -> String {
    if let Some(first) = cmdline.first() {
        let base = first.rsplit('/').next().unwrap_or(first);
        if base.len() > comm.len() && base.starts_with(comm) && !base.contains(' ') {
            return base.to_string();
        }
    }
    comm.to_string()
}

/// One process at one moment, with rates against the previous scan.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub start: u64,
    pub uid: u32,
    pub comm: String,
    pub name: String,
    pub cmdline: Vec<String>,
    pub state: char,
    pub threads: u32,
    pub kthread: bool,
    pub rss: u64,
    /// Percent of one core (100 = one core busy, more with several threads running).
    pub cpu: f32,
    /// Bytes per second from / to storage; `None` when `/proc/<pid>/io` is not readable (other users' processes).
    pub io: Option<(f64, f64)>,
}

impl Proc {
    pub fn key(&self) -> (u32, u64) {
        (self.pid, self.start)
    }
    pub fn io_total(&self) -> f64 {
        self.io.map_or(0.0, |(r, w)| r + w)
    }
}

/// A whole scan.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub procs: Vec<Proc>,
}

#[derive(Debug, Clone)]
struct Seen {
    cpu_ticks: u64,
    io: Option<(u64, u64)>,
}

/// Reads `/proc` (or a fixture tree) repeatedly and keeps what the rates need from the scan before.
pub struct Scanner {
    root: PathBuf,
    page: u64,
    hz: f64,
    prev: HashMap<(u32, u64), Seen>,
    at: Option<Instant>,
    /// Command lines never change for one process (`(pid, start)`), so each is read once.
    cmdlines: HashMap<(u32, u64), Vec<String>>,
}

impl Scanner {
    pub fn new(root: impl Into<PathBuf>) -> Scanner {
        // SAFETY: sysconf only reads system constants.
        let (page, hz) = unsafe { (libc::sysconf(libc::_SC_PAGESIZE), libc::sysconf(libc::_SC_CLK_TCK)) };
        Scanner::with_units(root, if page > 0 { page as u64 } else { 4096 }, if hz > 0 { hz as f64 } else { 100.0 })
    }

    pub fn with_units(root: impl Into<PathBuf>, page: u64, hz: f64) -> Scanner {
        Scanner { root: root.into(), page, hz, prev: HashMap::new(), at: None, cmdlines: HashMap::new() }
    }

    /// One scan at time `now`; the first has no rates (all 0), later ones are deltas over the elapsed wall time.
    pub fn scan(&mut self, now: Instant) -> Scan {
        let dt = self.at.map(|t| now.saturating_duration_since(t).as_secs_f64()).filter(|d| *d > 0.001);
        let mut out = Vec::new();
        let mut seen: HashMap<(u32, u64), Seen> = HashMap::with_capacity(self.prev.len() + 8);
        let Ok(dir) = std::fs::read_dir(&self.root) else { return Scan::default() };
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            let base = e.path();
            // A process may exit at any point of this: every failed read just drops it from the scan.
            let Some(st) = std::fs::read_to_string(base.join("stat")).ok().and_then(|t| parse_stat(&t)) else { continue };
            if st.pid != pid {
                continue;
            }
            let key = (pid, st.start);
            let uid = std::fs::metadata(&base).map_or(0, |m| m.uid());
            let kthread = st.flags & PF_KTHREAD != 0 || pid == 2;
            let cmdline = if kthread { Vec::new() } else { self.cmdlines.entry(key).or_insert_with(|| std::fs::read(base.join("cmdline")).map(|r| parse_cmdline(&r)).unwrap_or_default()).clone() };
            let io = std::fs::read_to_string(base.join("io")).ok().and_then(|t| parse_io(&t));
            let ticks = st.utime + st.stime;
            let prev = self.prev.get(&key);
            let (mut cpu, mut rate) = (0.0f32, None);
            if let (Some(p), Some(dt)) = (prev, dt) {
                cpu = (ticks.saturating_sub(p.cpu_ticks) as f64 / self.hz / dt * 100.0) as f32;
                if let (Some((r, w)), Some((pr, pw))) = (io, p.io) {
                    rate = Some((r.saturating_sub(pr) as f64 / dt, w.saturating_sub(pw) as f64 / dt));
                }
            }
            seen.insert(key, Seen { cpu_ticks: ticks, io });
            let name = display_name(&st.comm, &cmdline);
            out.push(Proc {
                pid,
                ppid: st.ppid,
                start: st.start,
                uid,
                comm: st.comm,
                name,
                cmdline,
                state: st.state,
                threads: st.threads,
                kthread,
                rss: st.rss_pages * self.page,
                cpu,
                io: if io.is_some() { Some(rate.unwrap_or((0.0, 0.0))) } else { None },
            });
        }
        // Forget processes that are gone (this also bounds the caches).
        self.cmdlines.retain(|k, _| seen.contains_key(k));
        self.prev = seen;
        self.at = Some(now);
        Scan { procs: out }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Start time of `pid` right now, `None` when it is gone.
pub fn start_time(root: &Path, pid: u32) -> Option<u64> {
    std::fs::read_to_string(root.join(pid.to_string()).join("stat")).ok().and_then(|t| parse_stat(&t)).map(|s| s.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const STAT: &str = "1234 (fire fox (x)) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 42 0 98765 1000000 2500 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";

    #[test]
    fn parses_a_stat_line_with_awkward_name() {
        let s = parse_stat(STAT).unwrap();
        assert_eq!((s.pid, s.comm.as_str(), s.state, s.ppid), (1234, "fire fox (x)", 'S', 1));
        assert_eq!((s.utime, s.stime, s.threads, s.start, s.rss_pages), (250, 50, 42, 98765, 2500));
        assert_eq!(s.flags & PF_KTHREAD, 0);
        assert!(parse_stat("garbage").is_none());
        assert!(parse_stat("1 (x) S 1").is_none(), "truncated line");
        assert!(parse_stat("x (a) S 1 1 1 0 -1 0 0 0 0 0 1 1 0 0 20 0 1 0 5 0 0").is_none(), "pid not a number");
    }

    #[test]
    fn parses_cmdline_and_io() {
        assert_eq!(parse_cmdline(b"/usr/bin/sleep\x00600\x00"), ["/usr/bin/sleep", "600"]);
        assert!(parse_cmdline(b"").is_empty());
        let io = "rchar: 1\nwchar: 2\nsyscr: 3\nsyscw: 4\nread_bytes: 4096\nwrite_bytes: 8192\ncancelled_write_bytes: 0\n";
        assert_eq!(parse_io(io), Some((4096, 8192)));
        assert_eq!(parse_io("rchar: 1\n"), None);
    }

    #[test]
    fn display_name_prefers_the_long_program_name() {
        assert_eq!(display_name("chrome_crashpad", &["/opt/x/chrome_crashpad_handler".into()]), "chrome_crashpad_handler");
        assert_eq!(display_name("sleep", &["sleep".into(), "5".into()]), "sleep");
        assert_eq!(display_name("kworker/0:1", &[]), "kworker/0:1");
        assert_eq!(display_name("python3", &["/usr/bin/python3.12".into(), "x.py".into()]), "python3.12");
    }

    fn fixture(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decmon-proc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[allow(clippy::too_many_arguments)]
    fn put(root: &Path, pid: u32, comm: &str, ppid: u32, flags: u64, ticks: (u64, u64), start: u64, rss: u64, io: Option<(u64, u64)>, cmd: &str) {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        let stat = format!("{pid} ({comm}) S {ppid} 1 1 0 -1 {flags} 0 0 0 0 {} {} 0 0 20 0 3 0 {start} 0 {rss} 0", ticks.0, ticks.1);
        std::fs::write(d.join("stat"), stat).unwrap();
        std::fs::write(d.join("cmdline"), cmd.replace(' ', "\0")).unwrap();
        if let Some((r, w)) = io {
            std::fs::write(d.join("io"), format!("rchar: 0\nread_bytes: {r}\nwrite_bytes: {w}\n")).unwrap();
        }
    }

    #[test]
    fn scans_and_computes_rates() {
        let root = fixture("scan");
        put(&root, 10, "busy", 1, 0, (100, 0), 500, 256, Some((0, 0)), "busy --x");
        put(&root, 11, "kworker/0:1", 2, PF_KTHREAD, (0, 0), 600, 0, None, "");
        std::fs::create_dir_all(root.join("self")).unwrap();
        std::fs::write(root.join("cpuinfo"), "x").unwrap();
        let mut sc = Scanner::with_units(&root, 4096, 100.0);
        let t0 = Instant::now();
        let first = sc.scan(t0);
        assert_eq!(first.procs.len(), 2, "only numeric directories");
        assert!(first.procs.iter().all(|p| p.cpu == 0.0), "no rates yet");
        let busy = first.procs.iter().find(|p| p.pid == 10).unwrap();
        assert_eq!((busy.rss, busy.cmdline.clone(), busy.kthread), (256 * 4096, vec!["busy".to_string(), "--x".to_string()], false));
        assert!(first.procs.iter().find(|p| p.pid == 11).unwrap().kthread);
        // 50 more ticks (0.5 s of CPU) in 1 s = 50 %; 1 MiB written meanwhile.
        put(&root, 10, "busy", 1, 0, (130, 20), 500, 256, Some((4096, 1 << 20)), "busy --x");
        let second = sc.scan(t0 + Duration::from_secs(1));
        let busy = second.procs.iter().find(|p| p.pid == 10).unwrap();
        assert!((busy.cpu - 50.0).abs() < 0.01, "{}", busy.cpu);
        assert_eq!(busy.io, Some((4096.0, (1 << 20) as f64)));
        assert_eq!(second.procs.iter().find(|p| p.pid == 11).unwrap().io, None, "unreadable io");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_reused_pid_starts_from_zero_and_vanished_ones_are_forgotten() {
        let root = fixture("reuse");
        put(&root, 20, "old", 1, 0, (1000, 0), 100, 1, None, "old");
        let mut sc = Scanner::with_units(&root, 4096, 100.0);
        let t0 = Instant::now();
        sc.scan(t0);
        // The pid comes back as a different program (new start time) with fewer ticks than before.
        put(&root, 20, "new", 1, 0, (5, 0), 900, 1, None, "new");
        let s = sc.scan(t0 + Duration::from_secs(1));
        assert_eq!(s.procs[0].cpu, 0.0, "a new process must not inherit the old one's counters");
        assert_eq!(s.procs[0].cmdline, ["new"], "and not its cached command line");
        std::fs::remove_dir_all(root.join("20")).unwrap();
        assert!(sc.scan(t0 + Duration::from_secs(2)).procs.is_empty());
        assert!(sc.prev.is_empty() && sc.cmdlines.is_empty(), "no memory kept for dead processes");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn start_time_reads_the_live_value() {
        let root = fixture("start");
        put(&root, 30, "x", 1, 0, (0, 0), 777, 0, None, "x");
        assert_eq!(start_time(&root, 30), Some(777));
        assert_eq!(start_time(&root, 31), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
