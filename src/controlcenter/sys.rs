//! Portable system probes and setters for the Control Center.
//!
//! Every probe runs a standard CLI (`nmcli`, `bluetoothctl`, `wpctl`/`pactl`, `brightnessctl`,
//! `playerctl`, ...) with a hard timeout, so a hung daemon can never freeze the bar. Output
//! parsing is pure and unit-tested against captured samples. A missing tool simply yields `None`.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

/// Runs `argv`, returning stdout if it exits successfully within `timeout`. The child is killed on timeout.
pub fn run(argv: &[&str], timeout: Duration) -> Option<String> {
    let (prog, args) = argv.split_first()?;
    let mut child = Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(15)),
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

/// Fire-and-forget (setters): detached, errors ignored, never blocks the caller.
pub fn spawn(argv: &[&str]) {
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
                out.last_mut().map(|s| s.push(c));
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, ':') => out.push(String::new()),
            (false, c) => {
                out.last_mut().map(|s| s.push(c));
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

pub fn set_volume(level: f32) {
    let level = level.clamp(0.0, 1.0);
    if have("wpctl") {
        spawn(&["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{level:.2}")]);
        if level > 0.0 {
            spawn(&["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "0"]);
        }
    } else if have("pactl") {
        spawn(&["pactl", "set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", (level * 100.0).round() as u32)]);
        if level > 0.0 {
            spawn(&["pactl", "set-sink-mute", "@DEFAULT_SINK@", "0"]);
        }
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
    if have("dunstctl") {
        return Some(run(&["dunstctl", "is-paused"], PROBE_TIMEOUT)?.trim() == "true");
    }
    if have("makoctl") {
        return Some(run(&["makoctl", "mode"], PROBE_TIMEOUT)?.lines().any(|l| l.trim() == "do-not-disturb"));
    }
    None
}

pub fn toggle_dnd() {
    if have("dunstctl") {
        spawn(&["dunstctl", "set-paused", "toggle"]);
    } else if have("makoctl") {
        spawn(&["makoctl", "mode", "-t", "do-not-disturb"]);
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

/// `desktop-style appearance` state file: `dark` / `light`.
pub fn is_light() -> bool {
    std::env::var_os("HOME")
        .and_then(|h| std::fs::read_to_string(std::path::PathBuf::from(h).join(".config/omarchy/desktop-styles/appearance")).ok())
        .map(|s| s.trim() == "light")
        .unwrap_or(false)
}

pub fn toggle_appearance() {
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

    #[test]
    fn successful_command_returns_stdout() {
        assert_eq!(run(&["echo", "hi"], Duration::from_secs(2)).as_deref(), Some("hi\n"));
    }

    #[test]
    fn failing_command_returns_none() {
        assert_eq!(run(&["false"], Duration::from_secs(2)), None);
    }
}
