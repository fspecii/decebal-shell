//! Values widgets can show, and the live sources that fill them.
//!
//! [`Data`] is a plain map from a dotted path (`cpu.usage`) to a [`Val`], plus the clock and the workspace tags. The renderer only ever
//! reads a `Data`, so tests and the gallery use [`sample`] (fixed numbers, a fixed time) and the daemon uses [`Live`].

use super::dsl::CommandDef;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Num(f64),
    Text(String),
    Bool(bool),
    Series(Vec<f32>),
}

impl Val {
    pub fn truthy(&self) -> bool {
        match self {
            Val::Num(n) => *n != 0.0,
            Val::Text(s) => !s.is_empty(),
            Val::Bool(b) => *b,
            Val::Series(s) => !s.is_empty(),
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Bool(b) => Some(f64::from(u8::from(*b))),
            Val::Text(s) => s.trim().parse().ok(),
            Val::Series(_) => None,
        }
    }
}

/// Broken-down local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Time {
    pub year: i32,
    /// 0 = January.
    pub mon: u32,
    pub mday: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    /// 0 = Sunday.
    pub wday: u32,
    pub yday: u32,
}

impl Time {
    pub fn from_unix(secs: i64) -> Time {
        let t: libc::time_t = secs as libc::time_t;
        // SAFETY: tm is plain data that localtime_r initialises on success; both pointers are valid for the call.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: see above; localtime_r is thread-safe.
        unsafe { libc::localtime_r(&t, &mut tm) };
        Time {
            year: tm.tm_year + 1900,
            mon: tm.tm_mon.max(0) as u32,
            mday: tm.tm_mday.max(1) as u32,
            hour: tm.tm_hour.max(0) as u32,
            min: tm.tm_min.max(0) as u32,
            sec: tm.tm_sec.max(0) as u32,
            wday: tm.tm_wday.max(0) as u32,
            yday: tm.tm_yday.max(0) as u32,
        }
    }
    pub fn now() -> Time {
        Time::from_unix(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
    }
}

/// One rdwm tag for the `workspaces` node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub active: bool,
    pub occupied: bool,
    pub urgent: bool,
}

#[derive(Debug, Clone)]
pub struct Data {
    pub vals: HashMap<String, Val>,
    pub time: Time,
    pub tags: Vec<Tag>,
}

impl Data {
    pub fn empty() -> Data {
        Data { vals: HashMap::new(), time: Time::now(), tags: Vec::new() }
    }
    pub fn get(&self, path: &str) -> Option<&Val> {
        self.vals.get(path)
    }
    pub fn set(&mut self, path: &str, v: Val) {
        self.vals.insert(path.to_string(), v);
    }
    pub fn num(&self, path: &str) -> Option<f64> {
        self.get(path).and_then(Val::num)
    }
    pub fn truthy(&self, path: &str) -> bool {
        let (neg, p) = path.strip_prefix('!').map(|p| (true, p)).unwrap_or((false, path));
        self.get(p).is_some_and(Val::truthy) != neg
    }
}

/// A data path a widget may name.
pub struct Source {
    pub path: &'static str,
    pub ty: &'static str,
    pub desc: &'static str,
}

macro_rules! src {
    ($($p:literal $t:literal $d:literal;)*) => { &[$(Source { path: $p, ty: $t, desc: $d }),*] };
}

pub const SOURCES: &[Source] = src! {
    "fetch.present" "bool" "Fastfetch returned supported data";
    "fetch.status" "text" "Fastfetch backend status";
    "fetch.os" "text" "Fastfetch OS name";
    "fetch.kernel" "text" "Fastfetch kernel release";
    "fetch.cpu" "text" "Fastfetch CPU model";
    "fetch.wm" "text" "Fastfetch window manager";
    "fetch.mem_used" "bytes" "Fastfetch memory used";
    "fetch.mem_total" "bytes" "Fastfetch memory total";
    "fetch.uptime" "time" "Fastfetch uptime in seconds";
    "clock" "time" "now; format with {clock:%H:%M} (%H %I %M %S %p %a %A %b %B %d %e %m %y %Y %j %%)";
    "clock.hour" "number" "0-23";
    "clock.minute" "number" "0-59";
    "clock.second" "number" "0-59";
    "cpu.usage" "fraction" "total CPU load 0..1";
    "cpu.temp" "number" "degrees C";
    "cpu.load" "number" "1-minute load average";
    "cpu.cores" "number" "logical CPUs";
    "cpu.history" "series" "last 60 seconds of cpu.usage";
    "mem.usage" "fraction" "RAM in use 0..1";
    "mem.used" "bytes" "RAM in use";
    "mem.total" "bytes" "installed RAM";
    "swap.usage" "fraction" "swap in use 0..1";
    "disk.usage" "fraction" "root filesystem in use 0..1";
    "disk.used" "bytes" "root filesystem used";
    "disk.total" "bytes" "root filesystem size";
    "disk.free" "bytes" "root filesystem free";
    "net.rx" "rate" "bytes/s received";
    "net.tx" "rate" "bytes/s sent";
    "net.rx_history" "series" "last 60 seconds of net.rx";
    "net.tx_history" "series" "last 60 seconds of net.tx";
    "gpu.present" "bool" "an NVIDIA GPU is available";
    "gpu.usage" "fraction" "GPU load 0..1";
    "gpu.mem" "fraction" "VRAM in use 0..1";
    "gpu.temp" "number" "degrees C";
    "gpu.name" "text" "model name";
    "battery.present" "bool" "the machine has a battery";
    "battery.level" "fraction" "charge 0..1";
    "battery.charging" "bool" "plugged in and charging";
    "battery.status" "text" "Charging, Discharging, Full...";
    "volume.level" "fraction" "output volume 0..1";
    "volume.muted" "bool" "output muted";
    "brightness.level" "fraction" "screen brightness 0..1";
    "workspace.current" "number" "number of the first visible workspace";
    "workspace.count" "number" "number of workspaces";
    "workspace.name" "text" "name of the visible workspace";
    "window.title" "text" "title of the focused window";
    "window.app" "text" "application of the focused window";
    "media.present" "bool" "a media player exists";
    "media.playing" "bool" "it is playing";
    "media.title" "text" "track title";
    "media.artist" "text" "artist";
    "media.album" "text" "album";
    "media.progress" "fraction" "position in the track 0..1";
    "media.position" "time" "seconds into the track";
    "media.length" "time" "track length in seconds";
    "media.art" "path" "cover art image file (for `image` nodes)";
    "weather.present" "bool" "weather is known";
    "weather.temp" "number" "degrees C";
    "weather.icon" "icon" "symbol name for `icon` nodes";
    "weather.code" "number" "wttr.in condition code";
    "user.name" "text" "login name";
    "user.display" "text" "full name";
    "host.name" "text" "host name";
    "uptime" "time" "seconds since boot";
};

/// True when `path` can be named in a widget: a known source, `opt.<key>` (instance options) or `cmd.<name>` (a declared command).
pub fn known(path: &str) -> bool {
    SOURCES.iter().any(|s| s.path == path) || path.starts_with("opt.") && path.len() > 4
}

pub fn suggest(path: &str) -> String {
    SOURCES
        .iter()
        .map(|s| s.path)
        .find(|k| {
            let (a, b): (Vec<char>, Vec<char>) = (path.chars().collect(), k.chars().collect());
            a.len().abs_diff(b.len()) <= 2 && a.iter().zip(&b).filter(|(x, y)| x != y).count() + a.len().abs_diff(b.len()) <= 2
        })
        .map(|k| format!(" (did you mean `{k}`?)"))
        .unwrap_or_default()
}

/// Fixed values for the gallery, tests and `decwidgets render --sample`. Time: Friday 2026-10-23 14:07:42.
pub fn sample() -> Data {
    let mut d = Data::empty();
    d.time = Time { year: 2026, mon: 9, mday: 23, hour: 14, min: 7, sec: 42, wday: 5, yday: 295 };
    let hist: Vec<f32> = (0..60).map(|i| 0.30 + 0.25 * ((i as f32) / 6.0).sin() + 0.1 * ((i as f32) / 2.3).cos()).collect();
    let rx: Vec<f32> = (0..60).map(|i| 200_000.0 + 150_000.0 * ((i as f32) / 5.0).sin().abs()).collect();
    let tx: Vec<f32> = (0..60).map(|i| 60_000.0 + 40_000.0 * ((i as f32) / 3.0).cos().abs()).collect();
    for (k, v) in [
        ("clock.hour", 14.0),
        ("clock.minute", 7.0),
        ("clock.second", 42.0),
        ("cpu.usage", 0.37),
        ("cpu.temp", 54.0),
        ("cpu.load", 1.42),
        ("cpu.cores", 16.0),
        ("mem.usage", 0.62),
        ("mem.used", 19.8e9),
        ("mem.total", 32.0e9),
        ("swap.usage", 0.05),
        ("disk.usage", 0.71),
        ("disk.used", 710e9),
        ("disk.total", 1000e9),
        ("disk.free", 290e9),
        ("net.rx", 412_000.0),
        ("net.tx", 88_000.0),
        ("gpu.usage", 0.18),
        ("gpu.mem", 0.33),
        ("gpu.temp", 47.0),
        ("battery.level", 0.82),
        ("volume.level", 0.55),
        ("brightness.level", 0.7),
        ("workspace.current", 2.0),
        ("workspace.count", 5.0),
        ("media.progress", 0.38),
        ("media.position", 85.0),
        ("media.length", 224.0),
        ("weather.temp", 18.0),
        ("weather.code", 116.0),
        ("uptime", 93_784.0),
    ] {
        d.set(k, Val::Num(v));
    }
    for (k, v) in [("gpu.present", true), ("battery.present", true), ("battery.charging", false), ("volume.muted", false), ("media.present", true), ("media.playing", true), ("weather.present", true)]
    {
        d.set(k, Val::Bool(v));
    }
    for (k, v) in [
        ("gpu.name", "RTX 4070"),
        ("battery.status", "Discharging"),
        ("workspace.name", "2"),
        ("window.title", "Decebal - theme engine"),
        ("window.app", "Terminal"),
        ("media.title", "Midnight City"),
        ("media.artist", "M83"),
        ("media.album", "Hurry Up, We're Dreaming"),
        ("weather.icon", "weather-partly"),
        ("user.name", "ada"),
        ("user.display", "Ada Lovelace"),
        ("host.name", "analytical"),
    ] {
        d.set(k, Val::Text(v.into()));
    }
    d.set("fetch.present", Val::Bool(true));
    d.set("fetch.status", Val::Text("Fastfetch sample data".into()));
    d.set("fetch.os", Val::Text("Example Linux".into()));
    d.set("fetch.kernel", Val::Text("6.12.0-example".into()));
    d.set("fetch.cpu", Val::Text("Example 16-core CPU".into()));
    d.set("fetch.wm", Val::Text("rdwm".into()));
    d.set("fetch.mem_used", Val::Num(19.8e9));
    d.set("fetch.mem_total", Val::Num(32.0e9));
    d.set("fetch.uptime", Val::Num(93784.0));
    d.set("cpu.history", Val::Series(hist));
    d.set("net.rx_history", Val::Series(rx));
    d.set("net.tx_history", Val::Series(tx));
    d.tags = (1..=5).map(|i| Tag { name: i.to_string(), active: i == 2, occupied: matches!(i, 1 | 2 | 4), urgent: i == 4 }).collect();
    d
}

// ---------------------------------------------------------------- live sources

/// A waker nobody listens to, for one-shot sampling (`decwidgets render --live`).
pub fn null_waker() -> crate::app::Waker {
    let (a, b) = std::os::unix::net::UnixStream::pair().expect("socketpair");
    let _ = a.set_nonblocking(true);
    std::mem::forget(b);
    crate::app::Waker::new(a)
}

/// The sources a widget set uses, by first path segment (`cpu`, `mem`, `media`...).
pub fn groups_of(paths: &BTreeSet<String>) -> BTreeSet<String> {
    paths.iter().map(|p| p.split('.').next().unwrap_or("").to_string()).collect()
}

/// Real data. Only the groups some widget names are sampled; `update` is cheap and meant to run about once a second.
pub struct Live {
    groups: BTreeSet<String>,
    sampler: Option<crate::dash::sysinfo::Sampler>,
    gpu: Arc<Mutex<Option<crate::dash::sysinfo::Gpu>>>,
    gpu_at: Option<std::time::Instant>,
    vol: Arc<Mutex<Option<(f32, bool)>>>,
    vol_at: Option<std::time::Instant>,
    tags: crate::menubar::rdwm::Shared,
    mpris: Option<crate::dash::mpris::Mpris>,
    cmds: Arc<Mutex<HashMap<String, String>>>,
    user: crate::dash::info::User,
    wm: Option<Box<dyn crate::menubar::active::ActiveWindow>>,
    fetch: Option<super::fetch::Fetch>,
}

impl Live {
    /// `paths` are every data path the widgets use; `commands` are the allowed `[commands.*]` (empty unless exec is enabled).
    pub fn new(paths: &BTreeSet<String>, commands: &[CommandDef], waker: crate::app::Waker) -> Live {
        let wake_fn = |w: &crate::app::Waker| -> Arc<dyn Fn() + Send + Sync> {
            let w = w.clone();
            Arc::new(move || w.wake())
        };
        let groups = groups_of(paths);
        let has = |g: &str| groups.contains(g);
        let tags: crate::menubar::rdwm::Shared = Arc::new(Mutex::new(None));
        if has("workspace") {
            let w = waker.clone();
            crate::menubar::rdwm::spawn_watcher(tags.clone(), move || w.wake());
        }
        let cmds = Arc::new(Mutex::new(HashMap::new()));
        for c in commands {
            spawn_command(c.clone(), cmds.clone(), wake_fn(&waker));
        }
        let mut wm: Option<Box<dyn crate::menubar::active::ActiveWindow>> = None;
        if has("window") {
            let mut w = crate::menubar::active::WmActive::default();
            crate::menubar::active::ActiveWindow::set_waker(&mut w, waker.clone());
            wm = Some(Box::new(w));
        }
        Live {
            fetch: has("fetch").then(|| super::fetch::Fetch::new(waker.clone())),
            sampler: ["cpu", "mem", "swap", "disk", "net", "uptime"].iter().any(|g| has(g)).then(|| crate::dash::sysinfo::Sampler::new(crate::dash::sysinfo::Paths::default())),
            gpu: Arc::new(Mutex::new(None)),
            gpu_at: None,
            vol: Arc::new(Mutex::new(None)),
            vol_at: None,
            tags,
            mpris: has("media").then(|| crate::dash::mpris::Mpris::start(wake_fn(&waker))),
            cmds,
            user: crate::dash::info::load_user(),
            wm,
            groups,
        }
    }

    pub fn update(&mut self, d: &mut Data) {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        d.time = Time::now();
        if let Some(fetch) = self.fetch.as_mut() {
            fetch.update(d);
        }
        let has = |g: &str| self.groups.contains(g);
        d.set("clock.hour", Val::Num(f64::from(d.time.hour)));
        d.set("clock.minute", Val::Num(f64::from(d.time.min)));
        d.set("clock.second", Val::Num(f64::from(d.time.sec)));
        if let Some(s) = self.sampler.as_mut() {
            let st = s.sample(now);
            d.set("cpu.usage", Val::Num(f64::from(st.cpu)));
            d.set("cpu.cores", Val::Num(st.cores.len() as f64));
            d.set("cpu.load", Val::Num(f64::from(st.load1.unwrap_or(0.0))));
            if let Some(t) = st.cpu_temp {
                d.set("cpu.temp", Val::Num(f64::from(t)));
            }
            d.set("cpu.history", Val::Series(st.cpu_hist.clone()));
            d.set("mem.usage", Val::Num(f64::from(st.mem.used_frac())));
            d.set("mem.used", Val::Num(st.mem.used_kb() as f64 * 1024.0));
            d.set("mem.total", Val::Num(st.mem.total_kb as f64 * 1024.0));
            d.set("swap.usage", Val::Num(f64::from(st.mem.swap_frac())));
            if let Some(disk) = &st.disk {
                d.set("disk.usage", Val::Num(f64::from(disk.used_frac())));
                d.set("disk.used", Val::Num(disk.used as f64));
                d.set("disk.total", Val::Num(disk.total as f64));
                d.set("disk.free", Val::Num(disk.total.saturating_sub(disk.used) as f64));
            }
            d.set("net.rx", Val::Num(st.rx));
            d.set("net.tx", Val::Num(st.tx));
            d.set("net.rx_history", Val::Series(st.rx_hist.clone()));
            d.set("net.tx_history", Val::Series(st.tx_hist.clone()));
            if let Some(u) = st.uptime {
                d.set("uptime", Val::Num(u));
            }
        }
        if has("gpu") && self.gpu_at.is_none_or(|t| now.duration_since(t) > Duration::from_secs(3)) {
            self.gpu_at = Some(now);
            let slot = self.gpu.clone();
            std::thread::spawn(move || *slot.lock().unwrap_or_else(|p| p.into_inner()) = crate::dash::sysinfo::probe_gpu());
        }
        if has("gpu") {
            let g = self.gpu.lock().unwrap_or_else(|p| p.into_inner()).clone();
            d.set("gpu.present", Val::Bool(g.is_some()));
            if let Some(g) = g {
                d.set("gpu.usage", Val::Num(f64::from(g.util.unwrap_or(0.0)) / 100.0));
                d.set("gpu.mem", Val::Num(f64::from(g.mem_frac().unwrap_or(0.0))));
                d.set("gpu.temp", Val::Num(f64::from(g.temp.unwrap_or(0.0))));
                d.set("gpu.name", Val::Text(g.name));
            }
        }
        if has("battery") {
            let b = crate::controlcenter::sys::probe_battery();
            d.set("battery.present", Val::Bool(b.is_some()));
            if let Some(b) = b {
                d.set("battery.level", Val::Num(f64::from(b.percent) / 100.0));
                d.set("battery.charging", Val::Bool(b.charging));
                d.set("battery.status", Val::Text(b.status));
            }
        }
        if has("volume") && self.vol_at.is_none_or(|t| now.duration_since(t) > Duration::from_secs(2)) {
            self.vol_at = Some(now);
            let slot = self.vol.clone();
            std::thread::spawn(move || *slot.lock().unwrap_or_else(|p| p.into_inner()) = crate::controlcenter::sys::probe_volume().map(|(v, _)| (v.level, v.muted)));
        }
        if has("volume") {
            if let Some((l, m)) = *self.vol.lock().unwrap_or_else(|p| p.into_inner()) {
                d.set("volume.level", Val::Num(f64::from(l.clamp(0.0, 1.0))));
                d.set("volume.muted", Val::Bool(m));
            }
        }
        if has("brightness") {
            if let Some(b) = crate::controlcenter::sys::probe_brightness() {
                d.set("brightness.level", Val::Num(f64::from(b) / 100.0));
            }
        }
        if has("workspace") {
            let st = self.tags.lock().unwrap_or_else(|p| p.into_inner()).clone();
            d.tags = st.as_ref().map(|s| s.tags.iter().map(|t| Tag { name: t.name.clone(), active: t.active, occupied: t.occupied, urgent: t.urgent }).collect()).unwrap_or_default();
            let cur = d.tags.iter().position(|t| t.active);
            d.set("workspace.count", Val::Num(d.tags.len() as f64));
            d.set("workspace.current", Val::Num(cur.map_or(1.0, |i| (i + 1) as f64)));
            d.set("workspace.name", Val::Text(cur.map(|i| d.tags[i].name.clone()).unwrap_or_default()));
        }
        if let Some(w) = &self.wm {
            d.set("window.title", Val::Text(w.active_title().unwrap_or_default()));
            d.set("window.app", Val::Text(w.active_app_name().map(|a| crate::menubar::active::display_name(&a)).unwrap_or_default()));
        }
        if let Some(m) = &self.mpris {
            let snap = m.snapshot();
            let p = crate::dash::mpris::pick_active(&snap.players, None);
            d.set("media.present", Val::Bool(p.is_some()));
            if let Some(p) = p {
                d.set("media.playing", Val::Bool(p.status == crate::dash::mpris::Status::Playing));
                d.set("media.title", Val::Text(p.meta.title.clone()));
                d.set("media.artist", Val::Text(p.meta.artist.clone()));
                d.set("media.album", Val::Text(p.meta.album.clone()));
                d.set("media.progress", Val::Num(f64::from(p.progress(now))));
                d.set("media.position", Val::Num(p.position_at(now) as f64 / 1e6));
                d.set("media.length", Val::Num(p.meta.length_us.unwrap_or(0) as f64 / 1e6));
                if let Some(path) = p.meta.art_url.as_deref().and_then(crate::dash::mpris::file_url_path) {
                    d.set("media.art", Val::Text(path.display().to_string()));
                }
            }
        }
        if has("weather") {
            let w = crate::menubar::weather::load_cached();
            d.set("weather.present", Val::Bool(w.is_some()));
            if let Some(w) = w {
                d.set("weather.temp", Val::Num(f64::from(w.temp_c)));
                d.set("weather.code", Val::Num(f64::from(w.code)));
                d.set("weather.icon", Val::Text(crate::menubar::weather::symbol_for(w.code, w.night).to_string()));
            }
        }
        if has("user") || has("host") {
            d.set("user.name", Val::Text(self.user.login.clone()));
            d.set("user.display", Val::Text(self.user.display.clone()));
            d.set("host.name", Val::Text(self.user.host.clone()));
        }
        for (k, v) in self.cmds.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            d.set(&format!("cmd.{k}"), Val::Text(v.clone()));
        }
    }
}

/// Runs a user command on a worker thread every `interval` seconds (at least 1), keeping the first line of stdout.
fn spawn_command(c: CommandDef, out: Arc<Mutex<HashMap<String, String>>>, waker: Arc<dyn Fn() + Send + Sync>) {
    use std::io::Read;
    std::thread::spawn(move || loop {
        let child = std::process::Command::new("sh").arg("-c").arg(&c.run).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn();
        if let Ok(mut child) = child {
            let start = std::time::Instant::now();
            let mut text = String::new();
            let mut stdout = child.stdout.take();
            let reader = std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(s) = stdout.as_mut() {
                    let _ = s.take(4096).read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            });
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if start.elapsed() > std::time::Duration::from_secs(5) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                    Err(_) => break,
                }
            }
            if let Ok(t) = reader.join() {
                text = t;
            }
            let line = text.lines().next().unwrap_or("").chars().filter(|c| !c.is_control()).take(200).collect::<String>();
            let mut g = out.lock().unwrap_or_else(|p| p.into_inner());
            if g.get(&c.name) != Some(&line) {
                g.insert(c.name.clone(), line);
                drop(g);
                waker();
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(u64::from(c.interval.max(1))));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_paths_and_sample_agree() {
        let d = sample();
        for s in SOURCES {
            if matches!(s.path, "clock" | "window.title" | "media.art") {
                continue;
            }
            assert!(d.get(s.path).is_some(), "sample() lacks {}", s.path);
        }
        assert!(known("cpu.usage") && known("opt.format") && !known("cpu.usag") && !known("opt."));
        assert!(suggest("cpu.usag").contains("cpu.usage"));
    }

    #[test]
    fn truthiness_and_negation() {
        let d = sample();
        assert!(d.truthy("media.playing") && !d.truthy("!media.playing") && d.truthy("!battery.charging"));
        assert!(!d.truthy("nope.nothing") && d.truthy("!nope.nothing"));
    }

    #[test]
    fn time_conversion_is_sane() {
        let t = Time::from_unix(0);
        assert!(t.year == 1970 || t.year == 1969);
        assert!(Time::now().year >= 2024);
    }
}
