//! System-wide numbers for the Performance tab: CPU (total and per core), memory and swap, disk throughput, network
//! per interface, GPUs (NVIDIA through `nvidia-smi`, AMD and Intel through sysfs), temperatures and battery power.
//!
//! Everything is read from `/proc` and `/sys` by pure parsers; a [`SysSampler`] turns consecutive readings into rates
//! and keeps a fixed-size history (60 samples) per series. It only runs while the monitor window is open.

use crate::dash::sysinfo::{self, CpuTimes, Gpu as NvGpu, Mem};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const HISTORY: usize = 60;

/// Where the files are (the tests point them at fixture trees).
#[derive(Debug, Clone)]
pub struct Roots {
    pub proc: PathBuf,
    pub sys: PathBuf,
    pub power: PathBuf,
    /// Query `nvidia-smi` (off in tests and when the tool is missing).
    pub nvidia: bool,
}

impl Default for Roots {
    fn default() -> Roots {
        let power = std::env::var_os("DECBAR_POWER_SUPPLY").map_or_else(|| PathBuf::from("/sys/class/power_supply"), PathBuf::from);
        Roots { proc: "/proc".into(), sys: "/sys".into(), power, nvidia: crate::controlcenter::sys::have("nvidia-smi") }
    }
}

// ---------------------------------------------------------------- parsers

/// `(read bytes, written bytes)` summed over the whole disks listed in `/proc/diskstats` (sectors are 512 bytes).
pub fn parse_diskstats(text: &str, whole: &HashSet<String>) -> (u64, u64) {
    let (mut r, mut w) = (0u64, 0u64);
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 10 || !whole.contains(f[2]) || f[2].starts_with("loop") || f[2].starts_with("ram") || f[2].starts_with("dm-") || f[2].starts_with("zram") {
            continue;
        }
        r += f[5].parse::<u64>().unwrap_or(0) * 512;
        w += f[9].parse::<u64>().unwrap_or(0) * 512;
    }
    (r, w)
}

/// Per-interface `(name, received, sent)` byte counters of `/proc/net/dev` without loopback and container plumbing.
pub fn parse_net_dev(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let name = name.trim();
            if name == "lo" || ["veth", "docker", "br-", "virbr", "vnet"].iter().any(|p| name.starts_with(p)) {
                return None;
            }
            let f: Vec<u64> = rest.split_whitespace().map(|v| v.parse().unwrap_or(0)).collect();
            (f.len() >= 9).then(|| (name.to_string(), f[0], f[8]))
        })
        .collect()
}

/// A `Key:   123 kB` field of `/proc/meminfo`.
pub fn meminfo_kb(text: &str, key: &str) -> u64 {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')?.split_whitespace().next()?.parse().ok()).unwrap_or(0)
}

/// The CPU model from `/proc/cpuinfo`.
pub fn cpu_model(text: &str) -> Option<String> {
    let l = text.lines().find(|l| l.starts_with("model name") || l.starts_with("Model") || l.starts_with("Hardware"))?;
    let v = l.split_once(':')?.1.trim();
    Some(v.split_whitespace().collect::<Vec<_>>().join(" "))
}

// ---------------------------------------------------------------- GPUs and battery

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gpu {
    pub name: String,
    pub util: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub temp: Option<f32>,
    pub power_w: Option<f32>,
    pub freq_mhz: Option<u32>,
}

fn read(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_num<T: std::str::FromStr>(p: &Path) -> Option<T> {
    read(p)?.parse().ok()
}

/// The first `hwmon*` directory under `dev/hwmon`.
fn hwmon_of(dev: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dev.join("hwmon")).ok()?.flatten().map(|e| e.path()).collect();
    v.sort();
    v.into_iter().next()
}

/// AMD and Intel GPUs from `/sys/class/drm/card*`; NVIDIA cards are left to `nvidia-smi` (their driver exposes nothing here).
pub fn drm_gpus(sys: &Path) -> Vec<Gpu> {
    let Ok(rd) = std::fs::read_dir(sys.join("class/drm")) else { return Vec::new() };
    let mut cards: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit()))).collect();
    cards.sort();
    let mut out = Vec::new();
    for c in cards {
        let dev = c.join("device");
        let n = c.file_name().and_then(|n| n.to_str()).unwrap_or("card").to_string();
        let hw = hwmon_of(&dev);
        let temp = hw.as_ref().and_then(|h| read_num::<f32>(&h.join("temp1_input"))).map(|t| t / 1000.0);
        let power_w = hw.as_ref().and_then(|h| read_num::<f64>(&h.join("power1_average")).or_else(|| read_num::<f64>(&h.join("power1_input")))).map(|u| (u / 1e6) as f32);
        match read(&dev.join("vendor")).as_deref() {
            Some("0x1002") => out.push(Gpu {
                name: read(&dev.join("product_name")).unwrap_or_else(|| format!("AMD Radeon ({n})")),
                util: read_num::<f32>(&dev.join("gpu_busy_percent")),
                mem_used: read_num(&dev.join("mem_info_vram_used")),
                mem_total: read_num(&dev.join("mem_info_vram_total")),
                temp,
                power_w,
                freq_mhz: None,
            }),
            Some("0x8086") => out.push(Gpu {
                name: format!("Intel Graphics ({n})"),
                temp,
                power_w,
                freq_mhz: read_num(&c.join("gt_cur_freq_mhz")).or_else(|| read_num(&c.join("gt/gt0/rps_cur_freq_mhz"))),
                ..Gpu::default()
            }),
            _ => {}
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Battery {
    pub percent: u32,
    pub status: String,
    /// Power flowing in or out, watts.
    pub watts: f32,
}

/// The first battery under a `power_supply` directory with its power draw (`power_now`, or current times voltage).
pub fn battery(root: &Path) -> Option<Battery> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        if read(&d.join("type")).as_deref() != Some("Battery") || read(&d.join("scope")).as_deref() == Some("Device") {
            continue;
        }
        let Some(percent) = read_num::<u32>(&d.join("capacity")) else { continue };
        let watts = read_num::<f64>(&d.join("power_now")).map(|u| u / 1e6).or_else(|| Some(read_num::<f64>(&d.join("current_now"))? * read_num::<f64>(&d.join("voltage_now"))? / 1e12)).unwrap_or(0.0);
        return Some(Battery { percent: percent.min(100), status: read(&d.join("status")).unwrap_or_default(), watts: watts.abs() as f32 });
    }
    None
}

fn nvidia(g: &NvGpu) -> Gpu {
    Gpu { name: g.name.clone(), util: g.util, mem_used: g.mem_used_mib.map(|m| m << 20), mem_total: g.mem_total_mib.map(|m| m << 20), temp: g.temp, ..Gpu::default() }
}

// ---------------------------------------------------------------- the sampler

#[derive(Debug, Clone, Default)]
pub struct NetIf {
    pub name: String,
    pub rx: f64,
    pub tx: f64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub rx_hist: Vec<f32>,
    pub tx_hist: Vec<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct GpuStat {
    pub gpu: Gpu,
    pub util_hist: Vec<f32>,
    pub mem_hist: Vec<f32>,
}

/// One reading of everything, with histories.
#[derive(Debug, Clone, Default)]
pub struct Perf {
    pub primed: bool,
    pub cpu_model: String,
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub cpu_hist: Vec<f32>,
    pub core_hist: Vec<Vec<f32>>,
    pub cpu_temp: Option<f32>,
    pub cpu_mhz: Option<u32>,
    pub load: Option<f32>,
    pub uptime: Option<f64>,
    pub processes: usize,
    pub threads: u64,
    pub mem: Mem,
    pub cached_kb: u64,
    pub mem_hist: Vec<f32>,
    pub swap_hist: Vec<f32>,
    pub disk_read: f64,
    pub disk_write: f64,
    pub disk_read_hist: Vec<f32>,
    pub disk_write_hist: Vec<f32>,
    pub nets: Vec<NetIf>,
    pub gpus: Vec<GpuStat>,
    pub battery: Option<Battery>,
    pub power_hist: Vec<f32>,
}

type Hist = VecDeque<f32>;

fn push(h: &mut Hist, v: f32) {
    if h.len() >= HISTORY {
        h.pop_front();
    }
    h.push_back(v);
}

fn vec(h: &Hist) -> Vec<f32> {
    h.iter().copied().collect()
}

#[derive(Default)]
struct NetHist {
    rx: Hist,
    tx: Hist,
}

#[derive(Default)]
struct GpuHist {
    util: Hist,
    mem: Hist,
}

pub struct SysSampler {
    roots: Roots,
    model: String,
    prev_cpu: Vec<CpuTimes>,
    prev_disk: Option<(u64, u64, Instant)>,
    prev_net: HashMap<String, (u64, u64, Instant)>,
    cpu: Hist,
    cores: Vec<Hist>,
    mem: Hist,
    swap: Hist,
    dr: Hist,
    dw: Hist,
    power: Hist,
    nets: HashMap<String, NetHist>,
    gpus: Vec<GpuHist>,
    nv: Option<Gpu>,
    nv_fails: u32,
    tick: u64,
}

impl SysSampler {
    pub fn new(roots: Roots) -> SysSampler {
        let model = read(&roots.proc.join("cpuinfo")).and_then(|t| cpu_model(&t)).unwrap_or_default();
        SysSampler {
            roots,
            model,
            prev_cpu: Vec::new(),
            prev_disk: None,
            prev_net: HashMap::new(),
            cpu: Hist::new(),
            cores: Vec::new(),
            mem: Hist::new(),
            swap: Hist::new(),
            dr: Hist::new(),
            dw: Hist::new(),
            power: Hist::new(),
            nets: HashMap::new(),
            gpus: Vec::new(),
            nv: None,
            nv_fails: 0,
            tick: 0,
        }
    }

    fn whole_disks(&self) -> HashSet<String> {
        std::fs::read_dir(self.roots.sys.join("block")).map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default()
    }

    pub fn sample(&mut self, now: Instant) -> Perf {
        let proc = &self.roots.proc;
        let text = |f: &str| std::fs::read_to_string(proc.join(f)).unwrap_or_default();
        let cpu_now = sysinfo::parse_cpu_stat(&text("stat"));
        let primed = !self.prev_cpu.is_empty() && self.prev_cpu.len() == cpu_now.len();
        let (cpu, cores) = if primed {
            (sysinfo::cpu_usage(self.prev_cpu[0], cpu_now[0]), self.prev_cpu.iter().zip(&cpu_now).skip(1).map(|(p, c)| sysinfo::cpu_usage(*p, *c)).collect::<Vec<_>>())
        } else {
            (0.0, vec![0.0; cpu_now.len().saturating_sub(1)])
        };
        self.prev_cpu = cpu_now;
        if self.cores.len() != cores.len() {
            self.cores = vec![Hist::new(); cores.len()];
        }

        let meminfo = text("meminfo");
        let mem = sysinfo::parse_meminfo(&meminfo);
        let cached_kb = meminfo_kb(&meminfo, "Cached") + meminfo_kb(&meminfo, "Buffers") + meminfo_kb(&meminfo, "SReclaimable");

        // Disk throughput.
        let (dr_total, dw_total) = parse_diskstats(&text("diskstats"), &self.whole_disks());
        let (mut dr, mut dw) = (0.0, 0.0);
        if let Some((pr, pw, at)) = self.prev_disk {
            let dt = now.saturating_duration_since(at).max(Duration::from_millis(1)).as_secs_f64();
            dr = dr_total.saturating_sub(pr) as f64 / dt;
            dw = dw_total.saturating_sub(pw) as f64 / dt;
        }
        self.prev_disk = Some((dr_total, dw_total, now));

        // Network per interface.
        let mut nets = Vec::new();
        let mut live = HashSet::new();
        for (name, rx_t, tx_t) in parse_net_dev(&text("net/dev")) {
            let (mut rx, mut tx) = (0.0, 0.0);
            if let Some((pr, pt, at)) = self.prev_net.get(&name) {
                let dt = now.saturating_duration_since(*at).max(Duration::from_millis(1)).as_secs_f64();
                rx = rx_t.saturating_sub(*pr) as f64 / dt;
                tx = tx_t.saturating_sub(*pt) as f64 / dt;
            }
            self.prev_net.insert(name.clone(), (rx_t, tx_t, now));
            live.insert(name.clone());
            let h = self.nets.entry(name.clone()).or_default();
            if primed {
                push(&mut h.rx, rx as f32);
                push(&mut h.tx, tx as f32);
            }
            // An interface that never moved a byte (a down device) is not worth a row.
            if rx_t + tx_t > 0 {
                nets.push(NetIf { name, rx, tx, rx_total: rx_t, tx_total: tx_t, rx_hist: vec(&h.rx), tx_hist: vec(&h.tx) });
            }
        }
        self.prev_net.retain(|k, _| live.contains(k));
        self.nets.retain(|k, _| live.contains(k));

        // GPUs: sysfs every second, nvidia-smi (a process) every other second.
        if self.roots.nvidia && self.nv_fails < 3 && self.tick % 2 == 0 {
            match sysinfo::probe_gpu() {
                Some(g) => {
                    self.nv = Some(nvidia(&g));
                    self.nv_fails = 0;
                }
                None => self.nv_fails += 1,
            }
        }
        let mut gpus: Vec<Gpu> = self.nv.clone().into_iter().collect();
        gpus.extend(drm_gpus(&self.roots.sys));
        if self.gpus.len() != gpus.len() {
            self.gpus = (0..gpus.len()).map(|_| GpuHist::default()).collect();
        }
        let gpu_stats: Vec<GpuStat> = gpus
            .into_iter()
            .zip(self.gpus.iter_mut())
            .map(|(gpu, h)| {
                if primed {
                    push(&mut h.util, gpu.util.unwrap_or(0.0) / 100.0);
                    push(&mut h.mem, gpu.mem_used.zip(gpu.mem_total).map_or(0.0, |(u, t)| u as f32 / t.max(1) as f32));
                }
                GpuStat { gpu, util_hist: vec(&h.util), mem_hist: vec(&h.mem) }
            })
            .collect();

        let bat = battery(&self.roots.power);
        if primed {
            push(&mut self.cpu, cpu);
            for (h, c) in self.cores.iter_mut().zip(&cores) {
                push(h, *c);
            }
            push(&mut self.mem, mem.used_frac());
            push(&mut self.swap, mem.swap_frac());
            push(&mut self.dr, dr as f32);
            push(&mut self.dw, dw as f32);
            if let Some(b) = &bat {
                push(&mut self.power, b.watts);
            }
        }
        self.tick += 1;
        Perf {
            primed,
            cpu_model: self.model.clone(),
            cpu,
            cores,
            cpu_hist: vec(&self.cpu),
            core_hist: self.cores.iter().map(vec).collect(),
            cpu_temp: sysinfo::cpu_temp(&self.roots.sys.join("class/hwmon")),
            cpu_mhz: read_num::<u64>(&self.roots.sys.join("devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")).map(|k| (k / 1000) as u32),
            load: sysinfo::parse_load(&text("loadavg")),
            uptime: sysinfo::parse_uptime(&text("uptime")),
            processes: 0,
            threads: 0,
            mem,
            cached_kb,
            mem_hist: vec(&self.mem),
            swap_hist: vec(&self.swap),
            disk_read: dr,
            disk_write: dw,
            disk_read_hist: vec(&self.dr),
            disk_write_hist: vec(&self.dw),
            nets,
            gpus: gpu_stats,
            battery: bat,
            power_hist: vec(&self.power),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decmon-sample-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn parses_disk_and_net_counters() {
        let ds = "   8       0 sda 100 0 2000 50 200 0 4000 80 0 0 0\n   8       1 sda1 90 0 1000 40 100 0 1000 70 0 0 0\n 259 0 nvme0n1 5 0 100 1 6 0 200 1 0 0 0\n   7 0 loop0 1 0 8 0 0 0 0 0 0 0 0\n 253 0 dm-0 1 0 8 0 0 0 8 0 0 0 0\n";
        let whole: HashSet<String> = ["sda", "nvme0n1", "loop0", "dm-0"].map(String::from).into();
        assert_eq!(parse_diskstats(ds, &whole), ((2000 + 100) * 512, (4000 + 200) * 512), "partitions, loop and device-mapper devices are not counted twice");
        let nd = "Inter-|   Receive |  Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n    lo: 500 5 0 0 0 0 0 0 500 5 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\nveth1a: 9 9 0 0 0 0 0 0 9 9 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(nd), vec![("eth0".to_string(), 1000, 2000)]);
        assert_eq!(meminfo_kb("Cached:   1234 kB\nSwapCached: 9 kB\n", "Cached"), 1234, "not SwapCached");
        assert_eq!(cpu_model("processor : 0\nmodel name\t: AMD  Ryzen 9\n").as_deref(), Some("AMD Ryzen 9"));
    }

    #[test]
    fn reads_amd_and_intel_gpus_from_sysfs_and_battery_power() {
        let s = tmp("gpu");
        put(&s, "class/drm/card0/device/vendor", "0x1002\n");
        put(&s, "class/drm/card0/device/gpu_busy_percent", "37\n");
        put(&s, "class/drm/card0/device/mem_info_vram_used", "1073741824\n");
        put(&s, "class/drm/card0/device/mem_info_vram_total", "8589934592\n");
        put(&s, "class/drm/card0/device/hwmon/hwmon3/temp1_input", "61000\n");
        put(&s, "class/drm/card0/device/hwmon/hwmon3/power1_average", "45000000\n");
        put(&s, "class/drm/card1/device/vendor", "0x8086\n");
        put(&s, "class/drm/card1/gt_cur_freq_mhz", "900\n");
        put(&s, "class/drm/card2/device/vendor", "0x10de\n");
        put(&s, "class/drm/card0-DP-1/status", "connected\n");
        let g = drm_gpus(&s);
        assert_eq!(g.len(), 2, "NVIDIA is not read from sysfs, connectors are not cards");
        assert_eq!((g[0].util, g[0].mem_used, g[0].mem_total, g[0].temp, g[0].power_w), (Some(37.0), Some(1 << 30), Some(8 << 30), Some(61.0), Some(45.0)));
        assert_eq!((g[1].util, g[1].freq_mhz), (None, Some(900)));
        let p = tmp("bat");
        put(&p, "BAT0/type", "Battery\n");
        put(&p, "BAT0/capacity", "64\n");
        put(&p, "BAT0/status", "Discharging\n");
        put(&p, "BAT0/current_now", "1500000\n");
        put(&p, "BAT0/voltage_now", "11400000\n");
        put(&p, "AC/type", "Mains\n");
        let b = battery(&p).unwrap();
        assert_eq!((b.percent, b.status.as_str()), (64, "Discharging"));
        assert!((b.watts - 17.1).abs() < 0.01, "{}", b.watts);
        put(&p, "BAT0/power_now", "8200000\n");
        assert!((battery(&p).unwrap().watts - 8.2).abs() < 0.01, "power_now wins");
        assert!(battery(&tmp("nobat")).is_none());
        for d in [s, p] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn sampler_turns_counters_into_rates_with_bounded_history() {
        let r = tmp("run");
        let roots = Roots { proc: r.join("proc"), sys: r.join("sys"), power: r.join("ps"), nvidia: false };
        let setup = |busy: u64, idle: u64, rx: u64, sect: u64| {
            put(&r, "proc/stat", &format!("cpu {busy} 0 0 {idle} 0 0 0 0\ncpu0 {busy} 0 0 {idle} 0 0 0 0\n"));
            put(&r, "proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 250 kB\nSwapTotal: 100 kB\nSwapFree: 100 kB\nCached: 300 kB\n");
            put(&r, "proc/net/dev", &format!("  eth0: {rx} 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n"));
            put(&r, "proc/diskstats", &format!("8 0 sda 0 0 {sect} 0 0 0 0 0 0 0 0\n"));
            put(&r, "sys/block/sda/size", "1");
        };
        setup(100, 900, 1000, 0);
        let mut s = SysSampler::new(roots);
        let t0 = Instant::now();
        let a = s.sample(t0);
        assert!(!a.primed && a.cpu == 0.0 && a.cpu_hist.is_empty());
        setup(150, 950, 3000, 2048);
        let b = s.sample(t0 + Duration::from_secs(1));
        assert!(b.primed);
        assert!((b.cpu - 0.5).abs() < 1e-6, "{}", b.cpu);
        assert_eq!((b.cores.len(), b.nets.len()), (1, 1));
        assert_eq!(b.nets[0].rx, 2000.0);
        assert_eq!(b.disk_read, 2048.0 * 512.0);
        assert!((b.mem.used_frac() - 0.75).abs() < 1e-6);
        assert_eq!(b.cached_kb, 300);
        for i in 0..100 {
            setup(150 + i, 950, 3000, 2048);
            s.sample(t0 + Duration::from_secs(2 + i));
        }
        let c = s.sample(t0 + Duration::from_secs(200));
        assert_eq!((c.cpu_hist.len(), c.core_hist[0].len(), c.mem_hist.len()), (HISTORY, HISTORY, HISTORY), "history is bounded");
        let _ = std::fs::remove_dir_all(&r);
    }
}
