//! Live system numbers for `decdash`'s Performance tab: CPU (total and per core), memory, swap, root
//! disk, network rates, CPU temperature and an optional NVIDIA GPU.
//!
//! Everything is read from `/proc`, `/sys/class/hwmon` and `statvfs` (plus one `nvidia-smi` call when that tool
//! exists), parsed by pure functions that the tests feed with captured samples. A [`Sampler`] turns two consecutive
//! readings into rates; the dashboard only runs it while its window is visible.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Samples kept for the sparklines (one per second).
pub const HISTORY: usize = 60;

// ---------------------------------------------------------------- parsers

/// Jiffies of one `cpu` line of `/proc/stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

/// `[aggregate, core0, core1, ...]` from `/proc/stat`. Guest time is already part of user time, idle includes iowait.
pub fn parse_cpu_stat(text: &str) -> Vec<CpuTimes> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let rest = name.strip_prefix("cpu")?;
            if !rest.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let v: Vec<u64> = it.take(8).map(|f| f.parse().unwrap_or(0)).collect();
            if v.len() < 4 {
                return None;
            }
            let total: u64 = v.iter().sum();
            let idle = v[3] + v.get(4).copied().unwrap_or(0);
            Some(CpuTimes { busy: total.saturating_sub(idle), total })
        })
        .collect()
}

/// Busy fraction (0..=1) between two readings of the same line; 0 when no time passed.
pub fn cpu_usage(prev: CpuTimes, cur: CpuTimes) -> f32 {
    let dt = cur.total.saturating_sub(prev.total);
    if dt == 0 {
        return 0.0;
    }
    (cur.busy.saturating_sub(prev.busy) as f32 / dt as f32).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mem {
    pub total_kb: u64,
    pub available_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

impl Mem {
    pub fn used_kb(&self) -> u64 {
        self.total_kb.saturating_sub(self.available_kb)
    }
    pub fn used_frac(&self) -> f32 {
        frac(self.used_kb(), self.total_kb)
    }
    pub fn swap_used_kb(&self) -> u64 {
        self.swap_total_kb.saturating_sub(self.swap_free_kb)
    }
    pub fn swap_frac(&self) -> f32 {
        frac(self.swap_used_kb(), self.swap_total_kb)
    }
}

fn frac(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f32 / whole as f32).clamp(0.0, 1.0)
    }
}

/// `/proc/meminfo`. Without `MemAvailable` (very old kernels) free + buffers + cached stands in.
pub fn parse_meminfo(text: &str) -> Mem {
    let get = |key: &str| -> Option<u64> { text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')?.split_whitespace().next()?.parse().ok()) };
    let total = get("MemTotal").unwrap_or(0);
    let available = get("MemAvailable").unwrap_or_else(|| get("MemFree").unwrap_or(0) + get("Buffers").unwrap_or(0) + get("Cached").unwrap_or(0));
    Mem { total_kb: total, available_kb: available.min(total), swap_total_kb: get("SwapTotal").unwrap_or(0), swap_free_kb: get("SwapFree").unwrap_or(0) }
}

/// Interfaces that only mirror other traffic (loopback, container bridges and veth pairs).
fn virtual_iface(name: &str) -> bool {
    name == "lo" || ["veth", "docker", "br-", "virbr", "podman", "cni"].iter().any(|p| name.starts_with(p))
}

/// Total received and sent bytes over the real interfaces of `/proc/net/dev`.
pub fn parse_net_dev(text: &str) -> (u64, u64) {
    let (mut rx, mut tx) = (0u64, 0u64);
    for l in text.lines() {
        let Some((name, rest)) = l.split_once(':') else { continue };
        if virtual_iface(name.trim()) {
            continue;
        }
        let f: Vec<u64> = rest.split_whitespace().map(|v| v.parse().unwrap_or(0)).collect();
        if f.len() >= 9 {
            rx = rx.saturating_add(f[0]);
            tx = tx.saturating_add(f[8]);
        }
    }
    (rx, tx)
}

/// Seconds since boot from `/proc/uptime`.
pub fn parse_uptime(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok().filter(|v: &f64| v.is_finite() && *v >= 0.0)
}

/// `up 3d 4h`, `up 2h 15m`, `up 7m`.
pub fn format_uptime(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (d, h, m) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    if d > 0 {
        format!("up {d}d {h}h")
    } else if h > 0 {
        format!("up {h}h {m}m")
    } else {
        format!("up {m}m")
    }
}

/// First load average from `/proc/loadavg`.
pub fn parse_load(text: &str) -> Option<f32> {
    text.split_whitespace().next()?.parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Disk {
    pub total: u64,
    pub used: u64,
}

impl Disk {
    pub fn used_frac(&self) -> f32 {
        frac(self.used, self.total)
    }
}

/// Size and use of the filesystem holding `path`, counted like `df` (used = total - free, percentage against what a user may use).
pub fn disk_usage(path: &Path) -> Option<Disk> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `st` is plain-old-data that statvfs fully initialises on success; `c` is a valid NUL-terminated path.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the duration of the call.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let unit = if st.f_frsize > 0 { st.f_frsize } else { st.f_bsize } as u64;
    let total = (st.f_blocks as u64).saturating_mul(unit);
    let used = (st.f_blocks as u64).saturating_sub(st.f_bfree as u64).saturating_mul(unit);
    // df shows used / (used + available): reserved blocks are not "free" for a user.
    let usable = used.saturating_add((st.f_bavail as u64).saturating_mul(unit));
    Some(Disk { total: if usable > 0 { usable } else { total }, used })
}

/// CPU package temperature in degrees C from a hwmon tree (`/sys/class/hwmon`): the `k10temp`, `coretemp` and
/// `zenpower` drivers (preferring the package / Tctl sensor), then other CPU thermal zones, then ACPI.
pub fn cpu_temp(root: &Path) -> Option<f32> {
    let mut best: Option<(u8, f32)> = None;
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let name = std::fs::read_to_string(d.join("name")).unwrap_or_default();
        let base = match name.trim() {
            "k10temp" | "coretemp" | "zenpower" => 3u8,
            "cpu_thermal" | "cpu-thermal" | "soc_thermal" => 2,
            "acpitz" => 1,
            _ => continue,
        };
        let read = |n: u32| std::fs::read_to_string(d.join(format!("temp{n}_input"))).ok().and_then(|t| t.trim().parse::<f32>().ok()).map(|m| m / 1000.0);
        let mut found: Option<(u8, f32)> = None;
        for n in 1..=16 {
            let Some(t) = read(n) else { continue };
            let label = std::fs::read_to_string(d.join(format!("temp{n}_label"))).unwrap_or_default();
            let rank = match label.trim() {
                "Tctl" | "Package id 0" | "Tdie" => 2,
                _ if n == 1 => 1,
                _ => 0,
            };
            if found.is_none_or(|(r, _)| rank > r) {
                found = Some((rank, t));
            }
        }
        if let Some((rank, t)) = found {
            let score = base * 3 + rank;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, t));
            }
        }
    }
    best.map(|(_, t)| t).filter(|t| (-50.0..200.0).contains(t))
}

/// One NVIDIA GPU as `nvidia-smi --query-gpu=name,utilization.gpu,temperature.gpu,memory.used,memory.total` reports it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gpu {
    pub name: String,
    pub util: Option<f32>,
    pub temp: Option<f32>,
    pub mem_used_mib: Option<u64>,
    pub mem_total_mib: Option<u64>,
}

impl Gpu {
    pub fn mem_frac(&self) -> Option<f32> {
        Some(frac(self.mem_used_mib?, self.mem_total_mib?))
    }
}

/// Arguments of the `nvidia-smi` query.
pub const NVIDIA_QUERY: [&str; 3] = ["nvidia-smi", "--query-gpu=name,utilization.gpu,temperature.gpu,memory.used,memory.total", "--format=csv,noheader,nounits"];

/// First line of the `csv,noheader,nounits` output; `[N/A]` and other junk become `None`.
pub fn parse_nvidia_smi(out: &str) -> Option<Gpu> {
    let line = out.lines().find(|l| !l.trim().is_empty())?;
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 5 {
        return None;
    }
    let (name, nums) = parts.split_at(parts.len() - 4);
    let name = name.join(", ");
    let num = |s: &str| s.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0);
    let gpu = Gpu { name, util: num(nums[0]).map(|v| v.min(100.0)), temp: num(nums[1]), mem_used_mib: num(nums[2]).map(|v| v as u64), mem_total_mib: num(nums[3]).map(|v| v as u64) };
    (!gpu.name.is_empty()).then_some(gpu)
}

// ---------------------------------------------------------------- formatting

/// `512 MiB`, `7.6 GiB` for a size given in kibibytes.
pub fn human_kb(kb: u64) -> String {
    human_bytes(kb.saturating_mul(1024))
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 || v >= 100.0 {
        format!("{:.0} {}", v, UNITS[i])
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

/// `0 B/s`, `12 kB/s`, `3.4 MB/s` (decimal units, like network tools).
pub fn human_rate(bytes_per_sec: f64) -> String {
    let b = bytes_per_sec.max(0.0);
    if b < 1000.0 {
        format!("{b:.0} B/s")
    } else if b < 1e6 {
        format!("{:.0} kB/s", b / 1e3)
    } else if b < 1e9 {
        format!("{:.1} MB/s", b / 1e6)
    } else {
        format!("{:.2} GB/s", b / 1e9)
    }
}

// ---------------------------------------------------------------- sampling

/// Where the sampler reads from (replaced by fixtures in tests).
#[derive(Debug, Clone)]
pub struct Paths {
    pub proc: PathBuf,
    pub hwmon: PathBuf,
    pub disk: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Paths { proc: "/proc".into(), hwmon: "/sys/class/hwmon".into(), disk: data_disk_path() }
    }
}

/// Which path's filesystem "the disk" means. That is `/`, except on an immutable system (OSTree, bootc, composefs):
/// there `/` is a small read-only image that is always 100 % full, and the user's data lives on another filesystem
/// that holds `$HOME`, so that one is meant.
pub fn data_disk_path() -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let root = Path::new("/");
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_dir());
    let dev = |p: &Path| std::fs::metadata(p).ok().map(|m| m.dev());
    pick_disk(root, home.as_deref(), read_only(root), dev(root), home.as_deref().and_then(dev))
}

/// The decision of [`data_disk_path`] from the facts it gathers (kept apart so it can be tested).
fn pick_disk(root: &Path, home: Option<&Path>, root_read_only: bool, root_dev: Option<u64>, home_dev: Option<u64>) -> PathBuf {
    match home {
        Some(h) if root_read_only && home_dev.is_some() && home_dev != root_dev => h.to_path_buf(),
        _ => root.to_path_buf(),
    }
}

/// True when the filesystem holding `path` is mounted read-only.
fn read_only(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: `st` is plain-old-data that statvfs fully initialises on success; `c` is a valid NUL-terminated path.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the duration of the call.
    unsafe { libc::statvfs(c.as_ptr(), &mut st) == 0 && (st.f_flag & libc::ST_RDONLY) != 0 }
}

/// One reading, with the sparkline histories (oldest first, at most [`HISTORY`] long).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// Total CPU busy fraction.
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub cpu_temp: Option<f32>,
    pub load1: Option<f32>,
    pub mem: Mem,
    pub disk: Option<Disk>,
    /// Bytes per second since the previous reading.
    pub rx: f64,
    pub tx: f64,
    pub uptime: Option<f64>,
    pub gpu: Option<Gpu>,
    pub cpu_hist: Vec<f32>,
    pub rx_hist: Vec<f32>,
    pub tx_hist: Vec<f32>,
    /// True once a second reading made the rates meaningful.
    pub primed: bool,
}

pub struct Sampler {
    paths: Paths,
    prev_cpu: Vec<CpuTimes>,
    prev_net: Option<(u64, u64, Instant)>,
    cpu_hist: VecDeque<f32>,
    rx_hist: VecDeque<f32>,
    tx_hist: VecDeque<f32>,
    primed: bool,
}

fn push(h: &mut VecDeque<f32>, v: f32) {
    if h.len() >= HISTORY {
        h.pop_front();
    }
    h.push_back(v);
}

impl Sampler {
    pub fn new(paths: Paths) -> Self {
        Sampler { paths, prev_cpu: Vec::new(), prev_net: None, cpu_hist: VecDeque::new(), rx_hist: VecDeque::new(), tx_hist: VecDeque::new(), primed: false }
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.paths.proc.join(name)).unwrap_or_default()
    }

    /// Reads everything once. The first call only primes the counters (rates and usage are 0 and `primed` is false).
    pub fn sample(&mut self, now: Instant) -> Stats {
        let cpu_now = parse_cpu_stat(&self.read("stat"));
        let (cpu, cores) = if !self.prev_cpu.is_empty() && self.prev_cpu.len() == cpu_now.len() {
            (cpu_usage(self.prev_cpu[0], cpu_now[0]), self.prev_cpu.iter().zip(&cpu_now).skip(1).map(|(p, c)| cpu_usage(*p, *c)).collect())
        } else {
            (0.0, vec![0.0; cpu_now.len().saturating_sub(1)])
        };
        self.primed = !self.prev_cpu.is_empty();
        self.prev_cpu = cpu_now;

        let (rx_total, tx_total) = parse_net_dev(&self.read("net/dev"));
        let (mut rx, mut tx) = (0.0, 0.0);
        if let Some((prx, ptx, at)) = self.prev_net {
            let dt = now.saturating_duration_since(at).max(Duration::from_millis(1)).as_secs_f64();
            rx = rx_total.saturating_sub(prx) as f64 / dt;
            tx = tx_total.saturating_sub(ptx) as f64 / dt;
        }
        self.prev_net = Some((rx_total, tx_total, now));

        if self.primed {
            push(&mut self.cpu_hist, cpu);
            push(&mut self.rx_hist, rx as f32);
            push(&mut self.tx_hist, tx as f32);
        }
        Stats {
            cpu,
            cores,
            cpu_temp: cpu_temp(&self.paths.hwmon),
            load1: parse_load(&self.read("loadavg")),
            mem: parse_meminfo(&self.read("meminfo")),
            disk: disk_usage(&self.paths.disk),
            rx,
            tx,
            uptime: parse_uptime(&self.read("uptime")),
            gpu: None,
            cpu_hist: self.cpu_hist.iter().copied().collect(),
            rx_hist: self.rx_hist.iter().copied().collect(),
            tx_hist: self.tx_hist.iter().copied().collect(),
            primed: self.primed,
        }
    }
}

/// Queries the first NVIDIA GPU, `None` when the tool is missing or fails.
pub fn probe_gpu() -> Option<Gpu> {
    crate::controlcenter::sys::run(&NVIDIA_QUERY, Duration::from_millis(1500)).and_then(|o| parse_nvidia_smi(&o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_immutable_systems_read_only_root_is_not_the_disk() {
        let (root, home) = (Path::new("/"), Path::new("/var/home/l1"));
        assert_eq!(pick_disk(root, Some(home), true, Some(1), Some(2)), home, "a read-only / and a separate data disk: the data disk");
        assert_eq!(pick_disk(root, Some(home), false, Some(1), Some(2)), root, "a writable / is the disk even when $HOME is elsewhere");
        assert_eq!(pick_disk(root, Some(home), true, Some(1), Some(1)), root, "$HOME on the same filesystem as /");
        assert_eq!(pick_disk(root, None, true, Some(1), None), root, "no $HOME to look at");
        assert_eq!(pick_disk(root, Some(home), true, Some(1), None), root, "an unreadable $HOME falls back to /");
    }

    #[test]
    fn the_default_path_is_a_real_filesystem() {
        let d = Paths::default().disk;
        assert!(disk_usage(&d).is_some_and(|d| d.total > 0), "{d:?}");
    }

    const STAT: &str = "cpu  4705 150 1120 16250 520 30 40 0 0 0\ncpu0 2400 70 560 8100 260 15 20 0 0 0\ncpu1 2305 80 560 8150 260 15 20 0 0 0\nintr 12345 1 2\nctxt 99\n";

    #[test]
    fn parses_cpu_lines_and_skips_the_rest() {
        let t = parse_cpu_stat(STAT);
        assert_eq!(t.len(), 3, "aggregate plus two cores; intr/ctxt ignored");
        // total = 4705+150+1120+16250+520+30+40 = 22815; idle = 16250+520
        assert_eq!(t[0], CpuTimes { busy: 22815 - 16770, total: 22815 });
        assert!(parse_cpu_stat("cpufreq 1 2 3 4\n").is_empty(), "'cpufreq' is not a cpu line");
        assert!(parse_cpu_stat("cpu 1 2\n").is_empty(), "too few fields");
    }

    #[test]
    fn usage_between_two_readings() {
        let a = CpuTimes { busy: 100, total: 1000 };
        let b = CpuTimes { busy: 150, total: 1100 };
        assert!((cpu_usage(a, b) - 0.5).abs() < 1e-6);
        assert_eq!(cpu_usage(a, a), 0.0, "no time passed");
        assert_eq!(cpu_usage(b, a), 0.0, "counters going backwards");
        assert_eq!(cpu_usage(a, CpuTimes { busy: 2000, total: 1001 }), 1.0, "clamped");
    }

    const MEMINFO: &str = "MemTotal:       16000000 kB\nMemFree:         1000000 kB\nMemAvailable:    4000000 kB\nBuffers:          100 kB\nSwapTotal:       2000000 kB\nSwapFree:        1500000 kB\n";

    #[test]
    fn parses_meminfo() {
        let m = parse_meminfo(MEMINFO);
        assert_eq!((m.total_kb, m.available_kb, m.used_kb()), (16_000_000, 4_000_000, 12_000_000));
        assert!((m.used_frac() - 0.75).abs() < 1e-6);
        assert_eq!((m.swap_used_kb(), m.swap_total_kb), (500_000, 2_000_000));
        assert!((m.swap_frac() - 0.25).abs() < 1e-6);
        let old = parse_meminfo("MemTotal: 100 kB\nMemFree: 10 kB\nBuffers: 5 kB\nCached: 15 kB\n");
        assert_eq!(old.available_kb, 30, "no MemAvailable: free + buffers + cached");
        assert_eq!(old.swap_frac(), 0.0, "no swap");
        assert_eq!(parse_meminfo("").used_frac(), 0.0);
    }

    const NETDEV: &str = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo: 5000 10 0 0 0 0 0 0 5000 10 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 2000 10 0 0 0 0 0 0\nwlan0:  300 3 0 0 0 0 0 0  400 3 0 0 0 0 0 0\ndocker0: 9999 1 0 0 0 0 0 0 9999 1 0 0 0 0 0 0\nveth1a:9999 1 0 0 0 0 0 0 9999 1 0 0 0 0 0 0\n";

    #[test]
    fn net_dev_sums_real_interfaces_only() {
        assert_eq!(parse_net_dev(NETDEV), (1300, 2400));
        assert_eq!(parse_net_dev(""), (0, 0));
        assert_eq!(parse_net_dev("eth0: 1 2 3\n"), (0, 0), "short line ignored");
    }

    #[test]
    fn uptime_and_load() {
        assert_eq!(parse_uptime("12345.67 9999.00\n"), Some(12345.67));
        assert_eq!(parse_uptime("garbage"), None);
        assert_eq!(format_uptime(90_061.0), "up 1d 1h");
        assert_eq!(format_uptime(8100.0), "up 2h 15m");
        assert_eq!(format_uptime(420.0), "up 7m");
        assert_eq!(format_uptime(-5.0), "up 0m");
        assert_eq!(parse_load("0.52 0.40 0.30 1/200 999\n"), Some(0.52));
    }

    #[test]
    fn disk_usage_of_a_real_path_is_sane() {
        let d = disk_usage(Path::new("/")).expect("statvfs /");
        assert!(d.total > 0 && d.used <= d.total && (0.0..=1.0).contains(&d.used_frac()));
        assert!(disk_usage(Path::new("/definitely/not/here")).is_none());
    }

    fn hwmon_dir(root: &Path, dir: &str, name: &str, temps: &[(u32, Option<&str>, i64)]) {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("name"), format!("{name}\n")).unwrap();
        for (n, label, milli) in temps {
            std::fs::write(d.join(format!("temp{n}_input")), format!("{milli}\n")).unwrap();
            if let Some(l) = label {
                std::fs::write(d.join(format!("temp{n}_label")), format!("{l}\n")).unwrap();
            }
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decdash-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn cpu_temp_prefers_the_package_sensor_and_the_cpu_driver() {
        let root = tmp("hwmon");
        hwmon_dir(&root, "hwmon0", "nvme", &[(1, Some("Composite"), 31000)]);
        hwmon_dir(&root, "hwmon1", "acpitz", &[(1, None, 20000)]);
        assert_eq!(cpu_temp(&root), Some(20.0), "only ACPI around");
        hwmon_dir(&root, "hwmon2", "k10temp", &[(1, Some("Tctl"), 55500), (3, Some("Tccd1"), 50000)]);
        assert_eq!(cpu_temp(&root), Some(55.5), "k10temp Tctl beats ACPI and nvme");
        hwmon_dir(&root, "hwmon3", "coretemp", &[(1, Some("Package id 0"), 61000), (2, Some("Core 0"), 58000)]);
        assert_eq!(cpu_temp(&root), Some(55.5), "an equally good second CPU driver does not replace the first");
        hwmon_dir(&root, "hwmon4", "nct6775", &[(1, None, 99000)]);
        assert_eq!(cpu_temp(&root), Some(55.5), "board sensors are not the CPU");
        assert_eq!(cpu_temp(&root.join("missing")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nvidia_smi_csv() {
        let g = parse_nvidia_smi("NVIDIA GeForce RTX 4070, 12, 45, 1024, 12282\n").unwrap();
        assert_eq!((g.name.as_str(), g.util, g.temp, g.mem_used_mib, g.mem_total_mib), ("NVIDIA GeForce RTX 4070", Some(12.0), Some(45.0), Some(1024), Some(12282)));
        assert!((g.mem_frac().unwrap() - 1024.0 / 12282.0).abs() < 1e-6);
        let na = parse_nvidia_smi("Tesla T4, [N/A], 40, [N/A], 15360\n").unwrap();
        assert_eq!((na.util, na.mem_used_mib, na.temp), (None, None, Some(40.0)));
        assert_eq!(na.mem_frac(), None);
        let comma = parse_nvidia_smi("Weird, Name, 5, 6, 7, 8\n").unwrap();
        assert_eq!((comma.name.as_str(), comma.util), ("Weird, Name", Some(5.0)));
        assert!(parse_nvidia_smi("").is_none());
        assert!(parse_nvidia_smi("NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver.").is_none());
        assert_eq!(parse_nvidia_smi("X, 250, 1, 1, 1").unwrap().util, Some(100.0), "clamped");
    }

    #[test]
    fn human_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_kb(8 * 1024 * 1024), "8.0 GiB");
        assert_eq!(human_kb(120 * 1024 * 1024), "120 GiB");
        assert_eq!(human_rate(0.0), "0 B/s");
        assert_eq!(human_rate(12_345.0), "12 kB/s");
        assert_eq!(human_rate(3_400_000.0), "3.4 MB/s");
        assert_eq!(human_rate(-1.0), "0 B/s");
    }

    #[test]
    fn sampler_turns_counters_into_rates() {
        let root = tmp("proc");
        std::fs::create_dir_all(root.join("net")).unwrap();
        let write = |stat: &str, rx: u64, tx: u64| {
            std::fs::write(root.join("stat"), stat).unwrap();
            std::fs::write(root.join("net/dev"), format!("eth0: {rx} 1 0 0 0 0 0 0 {tx} 1 0 0 0 0 0 0\n")).unwrap();
            std::fs::write(root.join("meminfo"), MEMINFO).unwrap();
            std::fs::write(root.join("uptime"), "100.5 50.0\n").unwrap();
            std::fs::write(root.join("loadavg"), "1.25 1 1 1/1 1\n").unwrap();
        };
        let mut s = Sampler::new(Paths { proc: root.clone(), hwmon: root.join("none"), disk: "/".into() });
        let t0 = Instant::now();
        write("cpu 100 0 100 800 0 0 0 0\ncpu0 100 0 100 800 0 0 0 0\n", 1000, 500);
        let a = s.sample(t0);
        assert!(!a.primed && a.cpu == 0.0 && a.rx == 0.0, "first reading only primes");
        assert_eq!(a.cores, vec![0.0]);
        write("cpu 200 0 200 900 0 0 0 0\ncpu0 200 0 200 900 0 0 0 0\n", 3000, 1500);
        let b = s.sample(t0 + Duration::from_secs(2));
        assert!(b.primed);
        assert!((b.cpu - 400.0 / 600.0).abs() < 1e-5, "cpu {}", b.cpu);
        assert_eq!(b.cores.len(), 1);
        assert_eq!((b.rx, b.tx), (1000.0, 500.0), "bytes per second over the 2 s interval");
        assert_eq!((b.load1, b.uptime, b.mem.total_kb), (Some(1.25), Some(100.5), 16_000_000));
        assert_eq!(b.cpu_temp, None, "no hwmon");
        assert_eq!(b.cpu_hist.len(), 1, "history starts with the first real reading");
        for i in 0..(HISTORY as u64 + 10) {
            write(&format!("cpu {} 0 0 {} 0 0 0 0\ncpu0 1 0 0 1 0 0 0 0\n", 200 + i, 1000 + i), 3000 + i, 1500);
            s.sample(t0 + Duration::from_secs(3 + i));
        }
        let c = s.sample(t0 + Duration::from_secs(200));
        assert_eq!(c.cpu_hist.len(), HISTORY, "history is bounded");
        assert_eq!(c.rx_hist.len(), HISTORY);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sampler_survives_a_missing_proc() {
        let mut s = Sampler::new(Paths { proc: "/nonexistent".into(), hwmon: "/nonexistent".into(), disk: "/nonexistent".into() });
        let st = s.sample(Instant::now());
        assert!(st.cores.is_empty() && st.disk.is_none() && st.mem.total_kb == 0);
        let st = s.sample(Instant::now());
        assert!(!st.primed, "nothing to prime from");
    }
}
