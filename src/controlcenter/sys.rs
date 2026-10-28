//! Portable system probes and setters for the Control Center.
//!
//! Every probe runs a standard CLI (`nmcli`, `bluetoothctl`, `wpctl`/`pactl`, `brightnessctl`,
//! `playerctl`, ...) with a hard timeout, so a hung daemon can never freeze the bar. Output
//! parsing is pure and unit-tested against captured samples. A missing tool simply yields `None`.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

/// Makes the child die with the thread that spawned it (Linux `PR_SET_PDEATHSIG`), so a bar that is killed
/// mid-probe, or restarted, never leaves a hung `bluetoothctl` or a `pactl subscribe` behind. Only for children
/// that the spawning thread outlives (probes it waits for, watchers it reads from).
#[cfg(target_os = "linux")]
pub fn die_with_parent(cmd: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    // SAFETY: prctl is async-signal-safe and touches no memory of ours.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    cmd
}

#[cfg(not(target_os = "linux"))]
pub fn die_with_parent(cmd: &mut Command) -> &mut Command {
    cmd
}

/// Runs `argv`, returning stdout if it exits successfully within `timeout`. The child is killed on timeout.
pub fn run(argv: &[&str], timeout: Duration) -> Option<String> {
    let (prog, args) = argv.split_first()?;
    let mut child = die_with_parent(Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let start = Instant::now();
    // Poll quickly at first (these tools usually answer within a few ms), then settle at 15 ms.
    let mut nap = 1;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(nap));
                nap = (nap * 2).min(15);
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let text = reader.join().ok()?;
    status.filter(|s| s.success()).map(|_| text)
}

static DRY_RUN: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// In dry-run mode setters are only recorded (see [`take_log`]), never executed.
pub fn set_dry_run(on: bool) {
    DRY_RUN.store(on, Ordering::SeqCst);
}

pub fn is_dry_run() -> bool {
    DRY_RUN.load(Ordering::SeqCst)
}

/// Commands recorded since the last call (dry-run only).
pub fn take_log() -> Vec<String> {
    LOG.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

/// Fire-and-forget (setters): detached, errors ignored, never blocks the caller.
pub fn spawn(argv: &[&str]) {
    if DRY_RUN.load(Ordering::SeqCst) {
        if let Ok(mut l) = LOG.lock() {
            l.push(argv.join(" "));
        }
        return;
    }
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let _ = run(&refs, Duration::from_secs(10));
    });
}

pub fn have(tool: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file())).unwrap_or(false)
}

// ---------------------------------------------------------------- network

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Net {
    pub wifi_enabled: bool,
    pub wifi_name: String,
    pub wired_name: String,
}

/// Splits an nmcli terse line on unescaped colons (`\:` is a literal colon).
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in line.chars() {
        match (esc, c) {
            (true, c) => {
                if let Some(s) = out.last_mut() {
                    s.push(c);
                }
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, ':') => out.push(String::new()),
            (false, c) => {
                if let Some(s) = out.last_mut() {
                    s.push(c);
                }
            }
        }
    }
    out
}

/// `radio` is the output of `nmcli -t -f WIFI radio`, `devices` of `nmcli -t -f TYPE,STATE,CONNECTION device`.
pub fn parse_net(radio: &str, devices: &str) -> Net {
    let mut net = Net { wifi_enabled: radio.trim() == "enabled", ..Net::default() };
    for line in devices.lines() {
        let f = split_terse(line);
        if f.len() < 3 || f[1] != "connected" {
            continue;
        }
        let name = f[2..].join(":");
        match f[0].as_str() {
            "wifi" if net.wifi_name.is_empty() => net.wifi_name = name,
            "ethernet" if net.wired_name.is_empty() => net.wired_name = name,
            _ => {}
        }
    }
    net
}

pub fn probe_net() -> Option<Net> {
    let radio = run(&["nmcli", "-t", "-f", "WIFI", "radio"], PROBE_TIMEOUT)?;
    let devices = run(&["nmcli", "-t", "-f", "TYPE,STATE,CONNECTION", "device"], PROBE_TIMEOUT).unwrap_or_default();
    Some(parse_net(&radio, &devices))
}

pub fn set_wifi(on: bool) {
    spawn(&["nmcli", "radio", "wifi", if on { "on" } else { "off" }]);
}

// -------------------------------------------------------------- bluetooth

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bluetooth {
    pub powered: bool,
    pub connected: usize,
}

/// `show` is `bluetoothctl show`; `connected` is `bluetoothctl devices Connected`.
pub fn parse_bluetooth(show: &str, connected: &str) -> Option<Bluetooth> {
    // "No default controller available" => no adapter.
    let powered_line = show.lines().find(|l| l.trim_start().starts_with("Powered:"))?;
    Some(Bluetooth { powered: powered_line.contains("yes"), connected: connected.lines().filter(|l| l.starts_with("Device ")).count() })
}

pub fn probe_bluetooth() -> Option<Bluetooth> {
    let show = run(&["bluetoothctl", "show"], PROBE_TIMEOUT)?;
    let connected = run(&["bluetoothctl", "devices", "Connected"], PROBE_TIMEOUT).unwrap_or_default();
    parse_bluetooth(&show, &connected)
}

pub fn set_bluetooth(on: bool) {
    spawn(&["bluetoothctl", "power", if on { "on" } else { "off" }]);
}

// ------------------------------------------------------------------ sound

#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    /// 0.0 ..= 1.0 (may exceed 1.0 when over-amplified; the UI clamps).
    pub level: f32,
    pub muted: bool,
}

/// `wpctl get-volume` output: `Volume: 0.50` or `Volume: 0.50 [MUTED]`.
pub fn parse_wpctl_volume(out: &str) -> Option<Volume> {
    let rest = out.trim().strip_prefix("Volume:")?.trim();
    let level: f32 = rest.split_whitespace().next()?.parse().ok()?;
    Some(Volume { level, muted: rest.contains("[MUTED]") })
}

/// `pactl get-sink-volume` and `get-sink-mute` outputs.
pub fn parse_pactl_volume(vol: &str, mute: &str) -> Option<Volume> {
    let pct = vol.split('/').find(|p| p.trim().ends_with('%'))?.trim().trim_end_matches('%').trim().parse::<f32>().ok()?;
    Some(Volume { level: pct / 100.0, muted: mute.trim().ends_with("yes") })
}

/// `node.description = "..."` from `wpctl inspect`.
pub fn parse_sink_name(inspect: &str) -> Option<String> {
    inspect.lines().find_map(|l| {
        let l = l.trim().trim_start_matches('*').trim();
        l.strip_prefix("node.description = \"").and_then(|r| r.strip_suffix('"')).map(str::to_string)
    })
}

pub fn probe_volume() -> Option<(Volume, String)> {
    if let Some(out) = run(&["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"], PROBE_TIMEOUT) {
        let v = parse_wpctl_volume(&out)?;
        let name = run(&["wpctl", "inspect", "@DEFAULT_AUDIO_SINK@"], PROBE_TIMEOUT).and_then(|o| parse_sink_name(&o)).unwrap_or_default();
        return Some((v, name));
    }
    let vol = run(&["pactl", "get-sink-volume", "@DEFAULT_SINK@"], PROBE_TIMEOUT)?;
    let mute = run(&["pactl", "get-sink-mute", "@DEFAULT_SINK@"], PROBE_TIMEOUT).unwrap_or_default();
    Some((parse_pactl_volume(&vol, &mute)?, String::new()))
}

/// The commands that set the default output to `level` (and unmute when audible).
fn volume_cmds(level: f32) -> Vec<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if have("wpctl") {
        let mut c = vec![s(&["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{level:.2}")])];
        if level > 0.0 {
            c.push(s(&["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "0"]));
        }
        c
    } else if have("pactl") {
        let mut c = vec![s(&["pactl", "set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", (level * 100.0).round() as u32)])];
        if level > 0.0 {
            c.push(s(&["pactl", "set-sink-mute", "@DEFAULT_SINK@", "0"]));
        }
        c
    } else {
        Vec::new()
    }
}

static VOL_PENDING: Mutex<Option<f32>> = Mutex::new(None);
static VOL_WORKER: AtomicBool = AtomicBool::new(false);

/// Sets the output volume. Requests are coalesced onto one worker so rapid scrolling or dragging
/// applies in order and only the latest value matters; the caller never blocks.
pub fn set_volume(level: f32) {
    let level = level.clamp(0.0, 1.0);
    if DRY_RUN.load(Ordering::SeqCst) {
        for c in volume_cmds(level) {
            let refs: Vec<&str> = c.iter().map(String::as_str).collect();
            spawn(&refs);
        }
        return;
    }
    *VOL_PENDING.lock().unwrap_or_else(|p| p.into_inner()) = Some(level);
    if VOL_WORKER.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| loop {
        let next = VOL_PENDING.lock().unwrap_or_else(|p| p.into_inner()).take();
        match next {
            Some(l) => {
                for c in volume_cmds(l) {
                    let refs: Vec<&str> = c.iter().map(String::as_str).collect();
                    let _ = run(&refs, Duration::from_secs(5));
                }
            }
            None => {
                VOL_WORKER.store(false, Ordering::SeqCst);
                // A request that slipped in between the take and the store needs a worker too.
                if VOL_PENDING.lock().unwrap_or_else(|p| p.into_inner()).is_none() || VOL_WORKER.swap(true, Ordering::SeqCst) {
                    break;
                }
            }
        }
    });
}

/// Toggles mute on the default output.
pub fn toggle_mute() {
    if have("wpctl") {
        spawn(&["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
    } else if have("pactl") {
        spawn(&["pactl", "set-sink-mute", "@DEFAULT_SINK@", "toggle"]);
    }
}

/// True when a line of `pactl subscribe` output concerns the default output's volume or identity.
pub fn is_sink_event(line: &str) -> bool {
    line.starts_with("Event 'change' on sink #") || line.starts_with("Event 'change' on server") || line.starts_with("Event 'new' on sink #") || line.starts_with("Event 'remove' on sink #")
}

/// Calls `on_event` (from a worker thread) whenever the output volume may have changed, so volume
/// keys pressed elsewhere show up at once. Uses `pactl subscribe` (a blocking read, zero polling);
/// without it the volume is re-read every few seconds.
pub fn watch_volume<F: Fn() + Send + 'static>(on_event: F) {
    if !have("pactl") {
        if have("wpctl") {
            std::thread::spawn(move || {
                let mut last = probe_volume().map(|v| v.0);
                loop {
                    std::thread::sleep(Duration::from_secs(3));
                    let now = probe_volume().map(|v| v.0);
                    if now != last {
                        last = now;
                        on_event();
                    }
                }
            });
        }
        return;
    }
    std::thread::spawn(move || loop {
        if let Ok(mut child) = die_with_parent(Command::new("pactl").arg("subscribe").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())).spawn() {
            if let Some(out) = child.stdout.take() {
                for line in std::io::BufRead::lines(std::io::BufReader::new(out)).map_while(Result::ok) {
                    if is_sink_event(&line) {
                        on_event();
                    }
                }
            }
            let _ = child.wait();
        }
        std::thread::sleep(Duration::from_secs(5));
    });
}

// ----------------------------------------------------------- wi-fi networks

#[derive(Debug, Clone, PartialEq)]
pub struct WifiNet {
    pub ssid: String,
    pub signal: u32,
    pub secure: bool,
    pub active: bool,
}

/// `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY dev wifi list`: strongest entry per SSID, active first.
pub fn parse_wifi_list(out: &str) -> Vec<WifiNet> {
    let mut nets: Vec<WifiNet> = Vec::new();
    for line in out.lines() {
        let f = split_terse(line);
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let net = WifiNet { ssid: f[1].clone(), signal: f[2].trim().parse().unwrap_or(0), secure: !f[3].trim().is_empty() && f[3].trim() != "--", active: f[0].trim() == "*" };
        match nets.iter_mut().find(|n| n.ssid == net.ssid) {
            Some(old) => {
                if net.active || (!old.active && net.signal > old.signal) {
                    *old = net;
                }
            }
            None => nets.push(net),
        }
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets
}

pub fn probe_wifi_list() -> Vec<WifiNet> {
    run(&["nmcli", "-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "no"], PROBE_TIMEOUT).map(|o| parse_wifi_list(&o)).unwrap_or_default()
}

/// Asks NetworkManager to rescan (result shows on the next refresh).
pub fn rescan_wifi() {
    spawn(&["nmcli", "dev", "wifi", "rescan"]);
}

/// Connects to a network (saved profile or open network); posts a notification when that fails,
/// e.g. a secured network without a saved password.
pub fn connect_wifi(ssid: &str) {
    if is_dry_run() {
        spawn(&["nmcli", "dev", "wifi", "connect", ssid]);
        return;
    }
    let ssid = ssid.to_string();
    std::thread::spawn(move || {
        let ok = run(&["nmcli", "-w", "20", "connection", "up", "id", &ssid], Duration::from_secs(25)).is_some()
            || run(&["nmcli", "-w", "20", "dev", "wifi", "connect", &ssid], Duration::from_secs(25)).is_some();
        if !ok && have("notify-send") {
            let _ = run(&["notify-send", "-a", "decbar", "Wi-Fi", &format!("Could not connect to {ssid}. Enter its password in the network editor first.")], Duration::from_secs(3));
        }
    });
}

// ------------------------------------------------------- bluetooth devices

#[derive(Debug, Clone, PartialEq)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
}

/// `bluetoothctl devices` lines (`Device AA:BB:.. Name`) plus the MACs of `devices Connected`.
pub fn parse_bt_devices(all: &str, connected: &str) -> Vec<BtDevice> {
    let conn: Vec<&str> = connected.lines().filter_map(|l| l.strip_prefix("Device ")).filter_map(|r| r.split_whitespace().next()).collect();
    let mut v: Vec<BtDevice> = all
        .lines()
        .filter_map(|l| l.strip_prefix("Device "))
        .filter_map(|r| {
            let (mac, name) = r.split_once(' ')?;
            Some(BtDevice { mac: mac.to_string(), name: name.trim().to_string(), connected: conn.contains(&mac) })
        })
        .collect();
    v.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

pub fn probe_bt_devices() -> Vec<BtDevice> {
    let all = run(&["bluetoothctl", "devices", "Paired"], PROBE_TIMEOUT).filter(|o| !o.trim().is_empty()).or_else(|| run(&["bluetoothctl", "devices"], PROBE_TIMEOUT)).unwrap_or_default();
    let conn = run(&["bluetoothctl", "devices", "Connected"], PROBE_TIMEOUT).unwrap_or_default();
    parse_bt_devices(&all, &conn)
}

pub fn set_bt_device(mac: &str, connect: bool) {
    spawn(&["bluetoothctl", if connect { "connect" } else { "disconnect" }, mac]);
}

// ------------------------------------------------------------------ battery

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub percent: u32,
    pub charging: bool,
    pub status: String,
}

/// Reads the first system battery under `root` (normally `/sys/class/power_supply`). Peripheral
/// batteries (`scope` = Device: mice, headsets) are skipped. `None` on desktops.
pub fn probe_battery_in(root: &std::path::Path) -> Option<Battery> {
    let mut dirs: Vec<_> = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let read = |f: &str| std::fs::read_to_string(d.join(f)).ok().map(|s| s.trim().to_string());
        if read("type").as_deref() != Some("Battery") || read("scope").as_deref() == Some("Device") {
            continue;
        }
        let Some(percent) = read("capacity").and_then(|c| c.parse::<u32>().ok()) else { continue };
        let status = read("status").unwrap_or_default();
        return Some(Battery { percent: percent.min(100), charging: status == "Charging", status });
    }
    None
}

pub fn probe_battery() -> Option<Battery> {
    // DECBAR_POWER_SUPPLY lets tests point at a fake tree.
    match std::env::var_os("DECBAR_POWER_SUPPLY") {
        Some(p) => probe_battery_in(std::path::Path::new(&p)),
        None => probe_battery_in(std::path::Path::new("/sys/class/power_supply")),
    }
}

// ------------------------------------------------------------- brightness

/// `brightnessctl -m` machine output: `intel_backlight,backlight,19200,50%,38400` -> 50.
pub fn parse_brightnessctl(out: &str) -> Option<u32> {
    let line = out.lines().find(|l| l.split(',').nth(1) == Some("backlight"))?;
    line.split(',').nth(3)?.trim().trim_end_matches('%').parse().ok()
}

pub fn probe_brightness() -> Option<u32> {
    if have("brightnessctl") {
        if let Some(v) = run(&["brightnessctl", "-m", "-c", "backlight"], PROBE_TIMEOUT).and_then(|o| parse_brightnessctl(&o)) {
            return Some(v);
        }
    }
    // Omarchy's wrapper also drives external monitors over DDC; only used when installed.
    if have("omarchy") {
        return run(&["omarchy", "brightness", "display", "--no-osd"], PROBE_TIMEOUT)?.trim().parse().ok();
    }
    None
}

pub fn set_brightness(percent: u32) {
    let p = percent.min(100);
    if have("brightnessctl") && probe_has_backlight() {
        spawn(&["brightnessctl", "-q", "-c", "backlight", "set", &format!("{p}%")]);
    } else if have("omarchy") {
        spawn(&["omarchy", "brightness", "display", "--no-osd", &format!("{p}%")]);
    }
}

fn probe_has_backlight() -> bool {
    run(&["brightnessctl", "-m", "-c", "backlight"], PROBE_TIMEOUT).and_then(|o| parse_brightnessctl(&o)).is_some()
}

// ------------------------------------------------------------------ media

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Media {
    pub playing: bool,
    pub title: String,
    pub artist: String,
}

/// `playerctl metadata --format '{{status}}\t{{title}}\t{{artist}}'`.
pub fn parse_playerctl(out: &str) -> Option<Media> {
    let line = out.lines().next()?;
    let mut f = line.split('\t');
    let status = f.next()?.trim();
    if status.is_empty() {
        return None;
    }
    Some(Media { playing: status == "Playing", title: f.next().unwrap_or("").trim().to_string(), artist: f.next().unwrap_or("").trim().to_string() })
}

pub fn probe_media() -> Option<Media> {
    parse_playerctl(&run(&["playerctl", "metadata", "--format", "{{status}}\t{{title}}\t{{artist}}"], PROBE_TIMEOUT)?)
}

pub fn media_command(cmd: &str) {
    spawn(&["playerctl", cmd]);
}

// ------------------------------------------------------- focus / DND / misc

/// DND state from whichever notification daemon is running.
pub fn probe_dnd() -> Option<bool> {
    // decnotify answers only while it is the running daemon (non-zero exit otherwise).
    if have("decnotify") {
        if let Some(out) = run(&["decnotify", "dnd", "status"], PROBE_TIMEOUT) {
            return Some(out.trim() == "on");
        }
    }
    if have("dunstctl") {
        return Some(run(&["dunstctl", "is-paused"], PROBE_TIMEOUT)?.trim() == "true");
    }
    if have("makoctl") {
        return Some(run(&["makoctl", "mode"], PROBE_TIMEOUT)?.lines().any(|l| l.trim() == "do-not-disturb"));
    }
    None
}

pub fn toggle_dnd() {
    if have("decnotify") && run(&["decnotify", "dnd", "status"], PROBE_TIMEOUT).is_some() {
        spawn(&["decnotify", "dnd", "toggle"]);
    } else if have("dunstctl") {
        spawn(&["dunstctl", "set-paused", "toggle"]);
    } else if have("makoctl") {
        spawn(&["makoctl", "mode", "-t", "do-not-disturb"]);
    }
}

// ------------------------------------------------------- decutil quick toggles

/// State of the `decutil` toggles (screen recording, idle inhibit, game mode).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Utils {
    pub record: bool,
    pub idle: bool,
    pub game: bool,
}

/// Where `decutil` is (next to the running binary, else on `PATH`); `None` when it is not installed.
pub fn decutil() -> Option<String> {
    let p = crate::launch::sibling_or_path("decutil");
    // Dry runs (tests) only record commands, so they do not need the real thing.
    (is_dry_run() || (p.contains('/') && std::path::Path::new(&p).is_file()) || have("decutil")).then_some(p)
}

/// Reads decutil's state files directly (no process is started); `None` when decutil is not installed.
pub fn probe_utils() -> Option<Utils> {
    decutil()?;
    let ctx = crate::util::Ctx::from_env();
    Some(Utils { record: crate::util::record::status(&ctx, &crate::util::RealSys).is_some(), idle: crate::util::idle::status(&ctx), game: crate::util::game::status(&ctx) })
}

/// `decutil FEATURE toggle` in the background (`feature`: `record`, `idle-inhibit`, `gamemode`, `dnd`).
pub fn util_toggle(feature: &str) {
    if let Some(m) = decutil() {
        spawn(&[&m, feature, "toggle"]);
    }
}

/// `{"enabled":false,"temperature":null}` from `omarchy toggle nightlight --status`.
pub fn parse_nightlight(out: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(out.trim()).ok()?.get("enabled")?.as_bool()
}

pub fn probe_night_shift() -> Option<bool> {
    if have("omarchy") {
        return parse_nightlight(&run(&["omarchy", "toggle", "nightlight", "--status"], PROBE_TIMEOUT)?);
    }
    None
}

pub fn toggle_night_shift() {
    if have("omarchy") {
        spawn(&["omarchy", "toggle", "nightlight"]);
    }
}

fn state_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
}

/// Omarchy's stay-awake indicator file.
pub fn stay_awake() -> bool {
    state_dir().map(|d| d.join("omarchy/indicators/stay-awake").exists()).unwrap_or(false)
}

pub fn toggle_stay_awake() {
    if have("omarchy") {
        spawn(&["omarchy", "toggle", "idle"]);
    }
}

/// Whether the shell is showing a light theme variant right now (the shared theme decides: an explicit
/// mode, or `desktop-style appearance` when the mode is "auto").
pub fn is_light() -> bool {
    !crate::theme::th().dark
}

/// "Switch to Light/Dark Appearance": flips the theme mode (explicit modes) and keeps Omarchy's
/// `desktop-style appearance` in step when it exists (in "auto" mode that alone changes the theme).
pub fn toggle_appearance() {
    use crate::theme::Mode;
    let mode = crate::theme::live::Source::discover().selection().mode;
    if mode != Mode::Auto {
        let next = if is_light() { "dark" } else { "light" };
        let ctl = crate::launch::sibling_or_path("decthemectl");
        spawn(&[&ctl, "set", "mode", next]);
    }
    if have("desktop-style") {
        spawn(&["desktop-style", "appearance", "toggle"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terse_split_honours_escaped_colons() {
        assert_eq!(split_terse(r"wifi:connected:My\:Net"), vec!["wifi", "connected", "My:Net"]);
        assert_eq!(split_terse("a::c"), vec!["a", "", "c"]);
    }

    #[test]
    fn parses_real_nmcli_output() {
        let n = parse_net("enabled\n", "ethernet:connected:Wired connection 1\nloopback:connected (externally):lo\n");
        assert_eq!(n, Net { wifi_enabled: true, wifi_name: String::new(), wired_name: "Wired connection 1".into() });
    }

    #[test]
    fn picks_first_connected_wifi_and_ignores_disconnected() {
        let n = parse_net("disabled", "wifi:disconnected:\nwifi:connected:Home\\:5G\nwifi:connected:Other\n");
        assert!(!n.wifi_enabled);
        assert_eq!(n.wifi_name, "Home:5G");
    }

    #[test]
    fn bluetooth_requires_an_adapter() {
        assert_eq!(parse_bluetooth("No default controller available\n", ""), None);
        let show = "Controller AA:BB (public)\n\tName: pc\n\tPowered: yes\n\tPairable: yes\n";
        let dev = "Device 11:22:33:44:55:66 Headset\nDevice 77:88 Mouse\n";
        assert_eq!(parse_bluetooth(show, dev), Some(Bluetooth { powered: true, connected: 2 }));
        assert_eq!(parse_bluetooth("\tPowered: no\n", ""), Some(Bluetooth { powered: false, connected: 0 }));
    }

    #[test]
    fn parses_wpctl_volume_samples() {
        assert_eq!(parse_wpctl_volume("Volume: 0.50\n"), Some(Volume { level: 0.5, muted: false }));
        assert_eq!(parse_wpctl_volume("Volume: 0.35 [MUTED]\n"), Some(Volume { level: 0.35, muted: true }));
        assert_eq!(parse_wpctl_volume("garbage"), None);
    }

    #[test]
    fn parses_pactl_samples() {
        let vol = "Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: 32768 /  50% / -18.06 dB\n        balance 0.00\n";
        assert_eq!(parse_pactl_volume(vol, "Mute: no\n"), Some(Volume { level: 0.5, muted: false }));
        assert_eq!(parse_pactl_volume(vol, "Mute: yes\n").map(|v| v.muted), Some(true));
        assert_eq!(parse_pactl_volume("nothing", ""), None);
    }

    #[test]
    fn parses_sink_description() {
        let inspect = "  * node.description = \"HyperX Cloud II Core Wireless Analog Stereo\"\n  * node.nick = \"HyperX\"\n";
        assert_eq!(parse_sink_name(inspect).as_deref(), Some("HyperX Cloud II Core Wireless Analog Stereo"));
        assert_eq!(parse_sink_name("nothing"), None);
    }

    #[test]
    fn brightnessctl_ignores_leds_and_reads_backlight() {
        assert_eq!(parse_brightnessctl("input2::kana,leds,0,0%,1\n"), None);
        assert_eq!(parse_brightnessctl("intel_backlight,backlight,19200,50%,38400\n"), Some(50));
        assert_eq!(parse_brightnessctl("input2::kana,leds,0,0%,1\nacpi_video0,backlight,10,100%,10\n"), Some(100));
    }

    #[test]
    fn parses_playerctl() {
        assert_eq!(parse_playerctl("Playing\tSong\tArtist\n"), Some(Media { playing: true, title: "Song".into(), artist: "Artist".into() }));
        assert_eq!(parse_playerctl("Paused\tSong\t\n").map(|m| m.playing), Some(false));
        assert_eq!(parse_playerctl(""), None);
    }

    #[test]
    fn parses_nightlight_json() {
        assert_eq!(parse_nightlight("{\"enabled\":false,\"temperature\":null}\n"), Some(false));
        assert_eq!(parse_nightlight("{\"enabled\":true}"), Some(true));
        assert_eq!(parse_nightlight("oops"), None);
    }

    #[test]
    fn wifi_list_keeps_the_strongest_entry_per_ssid_and_active_first() {
        let out = " :Cafe:40:WPA2\n *:Home\\:5G:70:WPA2\n :Home\\:5G:90:WPA2\n :Open:55:\n :--hidden:10:WPA2\n ::30:WPA2\n";
        let v = parse_wifi_list(out);
        assert_eq!(v.iter().map(|n| n.ssid.as_str()).collect::<Vec<_>>(), vec!["Home:5G", "Open", "Cafe", "--hidden"]);
        assert!(v[0].active && v[0].secure);
        assert!(!v[1].secure);
        assert_eq!(parse_wifi_list("garbage"), vec![]);
    }

    #[test]
    fn bluetooth_device_list_marks_connected_ones() {
        let all = "Device 11:22 Zeta Buds\nDevice 33:44 Alpha Mouse\nDevice 55:66 Headset\n";
        let v = parse_bt_devices(all, "Device 55:66 Headset\n");
        assert_eq!(v.iter().map(|d| (d.name.as_str(), d.connected)).collect::<Vec<_>>(), vec![("Headset", true), ("Alpha Mouse", false), ("Zeta Buds", false)]);
        assert_eq!(parse_bt_devices("", ""), vec![]);
    }

    #[test]
    fn battery_probe_skips_mains_and_peripherals_and_desktops() {
        let root = std::env::temp_dir().join(format!("decbar-ps-{}", std::process::id()));
        let mk = |name: &str, files: &[(&str, &str)]| {
            let d = root.join(name);
            std::fs::create_dir_all(&d).unwrap();
            for (f, c) in files {
                std::fs::write(d.join(f), c).unwrap();
            }
        };
        mk("AC", &[("type", "Mains\n")]);
        mk("hid-1", &[("type", "Battery\n"), ("scope", "Device\n"), ("capacity", "12\n")]);
        assert_eq!(probe_battery_in(&root), None);
        mk("BAT0", &[("type", "Battery\n"), ("capacity", "87\n"), ("status", "Charging\n")]);
        assert_eq!(probe_battery_in(&root), Some(Battery { percent: 87, charging: true, status: "Charging".into() }));
        assert_eq!(probe_battery_in(&root.join("nope")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_sink_and_server_events_count_as_volume_changes() {
        assert!(is_sink_event("Event 'change' on sink #52"));
        assert!(is_sink_event("Event 'change' on server #1"));
        assert!(!is_sink_event("Event 'change' on source #3"));
        assert!(!is_sink_event("Event 'new' on sink-input #9"));
    }

    #[test]
    fn missing_tool_yields_none() {
        assert_eq!(run(&["definitely-not-a-real-tool-xyz"], Duration::from_millis(200)), None);
        assert_eq!(run(&[], Duration::from_millis(200)), None);
    }

    #[test]
    fn hung_command_is_killed_at_the_timeout() {
        let start = Instant::now();
        assert_eq!(run(&["sleep", "5"], Duration::from_millis(150)), None);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_child_dies_with_the_thread_that_spawned_it() {
        let pid = std::thread::spawn(|| die_with_parent(Command::new("sleep").arg("30")).spawn().map(|c| c.id()).unwrap()).join().unwrap();
        let gone = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(30));
            // A killed child that nobody waited for is a zombie: its state letter is Z (or the entry is gone).
            std::fs::read_to_string(format!("/proc/{pid}/stat")).map(|s| s.rsplit(") ").next().unwrap_or("").starts_with('Z')).unwrap_or(true)
        });
        assert!(gone, "the sleep outlived the thread that spawned it");
    }

    #[test]
    fn successful_command_returns_stdout() {
        assert_eq!(run(&["echo", "hi"], Duration::from_secs(2)).as_deref(), Some("hi\n"));
    }

    #[test]
    fn failing_command_returns_none() {
        assert_eq!(run(&["false"], Duration::from_secs(2)), None);
    }
}
