//! Network: Wi-Fi (radio, scan, join, saved), ethernet, VPN list, IP/DNS info. Backend: `nmcli`.

use super::run::{Cmd, Runner};
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Row};

#[derive(Debug, Clone, PartialEq)]
pub struct Ap {
    pub in_use: bool,
    pub ssid: String,
    pub signal: u32,
    pub security: String,
}

impl Ap {
    pub fn secured(&self) -> bool {
        !self.security.trim().is_empty() && self.security.trim() != "--"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conn {
    pub name: String,
    pub uuid: String,
    pub kind: String,
    /// Device it is active on ("" = not active).
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dev {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub conn: String,
    pub hw: String,
    pub ip4: Vec<String>,
    pub gw: String,
    pub dns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub available: bool,
    pub error: Option<String>,
    pub wifi: Option<bool>,
    pub aps: Vec<Ap>,
    pub conns: Vec<Conn>,
    pub devs: Vec<Dev>,
}

#[derive(Default)]
pub struct Ui {
    /// SSID whose password sheet is open.
    pub join: Option<String>,
    pub password: String,
    pub scanning: bool,
}

// ---------------------------------------------------------------- parsers

/// Splits a line of `nmcli -t` output at unescaped colons (`\:` and `\\` are escapes).
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                if let Some(n) = it.next() {
                    out.last_mut().expect("last").push(n);
                }
            }
            ':' => out.push(String::new()),
            c => out.last_mut().expect("last").push(c),
        }
    }
    out
}

/// `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY dev wifi list`: one entry per SSID (strongest), in-use first.
pub fn parse_wifi_list(text: &str) -> Vec<Ap> {
    let mut aps: Vec<Ap> = Vec::new();
    for l in text.lines() {
        let f = split_terse(l);
        if f.len() < 4 || f[1].trim().is_empty() {
            continue;
        }
        let ap = Ap { in_use: f[0].trim() == "*", ssid: f[1].clone(), signal: f[2].trim().parse().unwrap_or(0), security: f[3].clone() };
        match aps.iter_mut().find(|a| a.ssid == ap.ssid) {
            Some(old) => {
                if ap.in_use || (!old.in_use && ap.signal > old.signal) {
                    let in_use = old.in_use || ap.in_use;
                    *old = Ap { in_use, ..ap };
                }
            }
            None => aps.push(ap),
        }
    }
    aps.sort_by(|a, b| b.in_use.cmp(&a.in_use).then(b.signal.cmp(&a.signal)).then(a.ssid.cmp(&b.ssid)));
    aps
}

/// `nmcli -t -f NAME,UUID,TYPE,DEVICE connection show`.
pub fn parse_connections(text: &str) -> Vec<Conn> {
    text.lines()
        .filter_map(|l| {
            let f = split_terse(l);
            (f.len() >= 4 && !f[1].is_empty()).then(|| Conn { name: f[0].clone(), uuid: f[1].clone(), kind: f[2].clone(), device: f[3].clone() })
        })
        .filter(|c| c.kind != "loopback")
        .collect()
}

/// `nmcli -t -f GENERAL.DEVICE,GENERAL.TYPE,GENERAL.STATE,GENERAL.CONNECTION,GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS device show`.
pub fn parse_device_show(text: &str) -> Vec<Dev> {
    let mut devs: Vec<Dev> = Vec::new();
    for l in text.lines() {
        let (k, v) = match l.split_once(':') {
            Some((k, v)) => (k, v.replace("\\:", ":")),
            None => continue,
        };
        let key = k.split('[').next().unwrap_or(k);
        if key == "GENERAL.DEVICE" {
            devs.push(Dev { name: v, ..Dev::default() });
            continue;
        }
        let Some(d) = devs.last_mut() else { continue };
        match key {
            "GENERAL.TYPE" => d.kind = v,
            "GENERAL.STATE" => d.state = v.split_once(' ').map(|(_, s)| s.trim_matches(|c| c == '(' || c == ')').to_string()).unwrap_or(v),
            "GENERAL.CONNECTION" => d.conn = v,
            "GENERAL.HWADDR" => d.hw = v,
            "IP4.ADDRESS" if !v.is_empty() => d.ip4.push(v),
            "IP4.GATEWAY" => d.gw = v,
            "IP4.DNS" if !v.is_empty() => d.dns.push(v),
            _ => {}
        }
    }
    devs.retain(|d| d.kind != "loopback" && d.name != "lo");
    devs
}

pub fn load(r: &dyn Runner) -> State {
    if !r.exists("nmcli") {
        return State { error: Some("NetworkManager's nmcli is not installed.".into()), ..State::default() };
    }
    let radio = r.run(&Cmd::new("nmcli", &["-t", "-f", "WIFI", "radio"]));
    if !radio.ok {
        let why = if radio.timed_out { "NetworkManager did not answer.".to_string() } else { format!("NetworkManager is not running ({}).", radio.error_text("nmcli")) };
        return State { error: Some(why), ..State::default() };
    }
    let wifi = Some(radio.stdout.trim() == "enabled");
    let aps = if wifi == Some(true) { parse_wifi_list(&r.run(&Cmd::new("nmcli", &["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list"])).stdout) } else { vec![] };
    let conns = parse_connections(&r.run(&Cmd::new("nmcli", &["-t", "-f", "NAME,UUID,TYPE,DEVICE", "connection", "show"])).stdout);
    let devs = parse_device_show(&r.run(&Cmd::new("nmcli", &["-t", "-f", "GENERAL.DEVICE,GENERAL.TYPE,GENERAL.STATE,GENERAL.CONNECTION,GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS", "device", "show"])).stdout);
    State { available: true, error: None, wifi, aps, conns, devs }
}

// ---------------------------------------------------------------- setters

pub fn valid_psk(p: &str) -> Result<(), String> {
    let n = p.chars().count();
    if p.chars().any(|c| c.is_control()) {
        return Err("The password contains invalid characters".into());
    }
    if n == 64 && p.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(());
    }
    if !(8..=63).contains(&n) {
        return Err("A Wi-Fi password has 8 to 63 characters".into());
    }
    Ok(())
}

fn valid_name(s: &str) -> Result<(), String> {
    if s.is_empty() || s.len() > 128 || s.chars().any(|c| c.is_control()) || s.starts_with('-') {
        return Err("Invalid name".into());
    }
    Ok(())
}

pub fn set_wifi(on: bool) -> Vec<Cmd> {
    vec![Cmd::new("nmcli", &["radio", "wifi", if on { "on" } else { "off" }])]
}

pub fn rescan() -> Vec<Cmd> {
    vec![Cmd::new("nmcli", &["device", "wifi", "rescan"]).timeout(15)]
}

/// Join a network: a saved profile is brought up, otherwise a new connection is made (with `password` if secured).
pub fn join(st: &State, ssid: &str, password: Option<&str>) -> Result<Vec<Cmd>, String> {
    valid_name(ssid)?;
    if let Some(c) = st.conns.iter().find(|c| c.kind == "802-11-wireless" && c.name == ssid) {
        return Ok(vec![Cmd::new("nmcli", &["connection", "up", "uuid", &c.uuid]).timeout(30)]);
    }
    let mut a = vec!["device".to_string(), "wifi".into(), "connect".into(), ssid.into()];
    if let Some(p) = password {
        valid_psk(p)?;
        a.push("password".into());
        a.push(p.into());
    }
    Ok(vec![Cmd::owned("nmcli", a).timeout(40)])
}

pub fn disconnect(c: &Conn) -> Vec<Cmd> {
    vec![Cmd::new("nmcli", &["connection", "down", "uuid", &c.uuid])]
}

pub fn forget(c: &Conn) -> Vec<Cmd> {
    vec![Cmd::new("nmcli", &["connection", "delete", "uuid", &c.uuid])]
}

pub fn vpn_toggle(c: &Conn, up: bool) -> Vec<Cmd> {
    vec![Cmd::new("nmcli", &["connection", if up { "up" } else { "down" }, "uuid", &c.uuid]).timeout(30)]
}

// ---------------------------------------------------------------- pane

fn bars(s: u32) -> &'static str {
    match s {
        0..=24 => "▁",
        25..=49 => "▂▃",
        50..=74 => "▂▃▅",
        _ => "▂▃▅▇",
    }
}

fn is_vpn(c: &Conn) -> bool {
    c.kind == "vpn" || c.kind == "wireguard"
}

pub fn items(h: &Hub) -> Vec<Item> {
    let Some(st) = &h.net.data else { return vec![Item::Note { text: "Reading network state...".into(), warn: false }] };
    if !st.available {
        return super::sound::unavailable("Network", st.error.as_deref().unwrap_or("NetworkManager is not available."));
    }
    let ui = &h.ui_net;
    let mut v = Vec::new();
    // password sheet
    if let Some(ssid) = &ui.join {
        v.push(Item::Header(format!("Join \"{ssid}\"")));
        v.push(Item::Card(vec![
            Row::new("Password").detail("8 to 63 characters (or 64 hex digits)").ctl(Ctl::new("s:net:pw:secret", Kind::Text { val: if ui.password.is_empty() { String::new() } else { "•".repeat(ui.password.chars().count()) }, hint: "Wi-Fi password".into() })),
            Row::new("").ctl(Ctl::new("s:net:cancel:", Kind::Button { label: "Cancel".into(), danger: false })).ctl(Ctl::new("s:net:joinpw:", Kind::Button { label: "Join".into(), danger: false })),
        ]));
    }
    let wifi_dev = st.devs.iter().find(|d| d.kind == "wifi");
    let mut wrows = vec![Row::new("Wi-Fi").detail(match (st.wifi, wifi_dev) {
        (Some(false), _) => "Off".to_string(),
        (_, Some(d)) if !d.conn.is_empty() && d.conn != "--" => format!("Connected to {}", d.conn),
        (_, Some(_)) => "Not connected".to_string(),
        (_, None) => "No Wi-Fi adapter found".to_string(),
    })];
    if wifi_dev.is_some() || st.wifi == Some(false) {
        wrows[0] = wrows[0].clone().ctl(Ctl::new("s:net:wifi:", Kind::Toggle(st.wifi == Some(true))));
    }
    v.push(Item::Header("Wi-Fi".into()));
    v.push(Item::Card(wrows));
    if st.wifi == Some(true) && wifi_dev.is_some() {
        v.push(Item::Header("Available networks".into()));
        let mut rows: Vec<Row> = st
            .aps
            .iter()
            .map(|a| {
                let known = st.conns.iter().any(|c| c.kind == "802-11-wireless" && c.name == a.ssid);
                let detail = format!("{} {}%{}{}", bars(a.signal), a.signal, if a.secured() { format!(" · {}", a.security.trim()) } else { " · open".into() }, if known { " · saved" } else { "" });
                let mut r = Row::new(a.ssid.clone()).detail(detail);
                if a.in_use {
                    r.selected = true;
                    if let Some(c) = st.conns.iter().find(|c| c.name == a.ssid && !c.device.is_empty()) {
                        r = r.ctl(Ctl::new(format!("s:net:down:{}", c.uuid), Kind::Button { label: "Disconnect".into(), danger: false }));
                    }
                } else {
                    r = r.ctl(Ctl::new(format!("s:net:join:{}", a.ssid), Kind::Button { label: "Join".into(), danger: false }));
                }
                r
            })
            .collect();
        if rows.is_empty() {
            rows.push(Row::new(if ui.scanning { "Scanning..." } else { "No networks found" }));
        }
        rows.push(Row::new("Scan again").detail("Look for networks now").ctl(Ctl::new("s:net:rescan:", Kind::Button { label: if ui.scanning { "Scanning...".into() } else { "Scan".into() }, danger: false })));
        v.push(Item::Card(rows));
    }
    // saved
    let saved: Vec<&Conn> = st.conns.iter().filter(|c| c.kind == "802-11-wireless").collect();
    if !saved.is_empty() {
        v.push(Item::Header("Saved networks".into()));
        v.push(Item::Card(saved.iter().map(|c| Row::new(c.name.clone()).detail(if c.device.is_empty() { "" } else { "Connected" }).ctl(Ctl::new(format!("s:net:forget:{}", c.uuid), Kind::Button { label: "Forget".into(), danger: true }))).collect()));
    }
    // vpn
    let vpns: Vec<&Conn> = st.conns.iter().filter(|c| is_vpn(c)).collect();
    v.push(Item::Header("VPN".into()));
    if vpns.is_empty() {
        v.push(Item::Card(vec![Row::new("No VPN configured").detail("Add one with nmcli or your VPN's NetworkManager plugin")]));
    } else {
        v.push(Item::Card(vpns.iter().map(|c| Row::new(c.name.clone()).detail(c.kind.clone()).ctl(Ctl::new(format!("s:net:vpn:{}", c.uuid), Kind::Toggle(!c.device.is_empty())))).collect()));
    }
    // devices
    v.push(Item::Header("Connections".into()));
    let mut rows = Vec::new();
    for d in st.devs.iter().filter(|d| d.kind != "wifi-p2p" && d.kind != "bridge" && d.kind != "tun") {
        let state = if d.state.is_empty() { "unknown".to_string() } else { d.state.clone() };
        let label = match d.kind.as_str() {
            "ethernet" => "Ethernet",
            "wifi" => "Wi-Fi adapter",
            k => k,
        };
        let mut detail = format!("{} · {}", d.name, state);
        if !d.ip4.is_empty() {
            detail += &format!(" · {}", d.ip4.join(", "));
        }
        rows.push(Row::new(label.to_string()).detail(detail).ctl(Ctl::new(format!("lbl:net:{}", d.name), Kind::Label(if d.conn.is_empty() || d.conn == "--" { "-".into() } else { d.conn.clone() }))));
        if !d.gw.is_empty() {
            rows.push(Row::new("  Gateway").ctl(Ctl::new(format!("lbl:gw:{}", d.name), Kind::Label(d.gw.clone()))));
        }
        if !d.dns.is_empty() {
            rows.push(Row::new("  DNS").ctl(Ctl::new(format!("lbl:dns:{}", d.name), Kind::Label(d.dns.join(", ")))));
        }
        if !d.hw.is_empty() {
            rows.push(Row::new("  Hardware address").ctl(Ctl::new(format!("lbl:hw:{}", d.name), Kind::Label(d.hw.clone()))));
        }
    }
    if rows.is_empty() {
        rows.push(Row::new("No network devices"));
    }
    v.push(Item::Card(rows));
    v
}

pub fn apply(h: &mut Hub, act: &str, arg: &str, val: CtlVal) {
    let Some(st) = h.net.data.clone() else { return };
    let conn = |u: &str| st.conns.iter().find(|c| c.uuid == u).cloned();
    let res: Result<Option<Action>, String> = (|| match (act, &val) {
        ("wifi", CtlVal::Bool(on)) => Ok(Some(Action::new(Dom::Network, "Wi-Fi", set_wifi(*on)).undo(set_wifi(!*on)))),
        ("rescan", _) => {
            h.ui_net.scanning = true;
            Ok(Some(Action::new(Dom::Network, "Scan", rescan())))
        }
        ("join", CtlVal::Click) => {
            let ap = st.aps.iter().find(|a| a.ssid == arg).ok_or("That network is no longer in range")?;
            let known = st.conns.iter().any(|c| c.kind == "802-11-wireless" && c.name == arg);
            if ap.secured() && !known {
                h.ui_net.join = Some(arg.to_string());
                h.ui_net.password.clear();
                return Ok(None);
            }
            Ok(Some(Action::new(Dom::Network, format!("Join {arg}"), join(&st, arg, None)?).msg(format!("Connected to {arg}"))))
        }
        ("pw", CtlVal::Str(p)) => {
            // the field shows bullets and starts empty when edited: an empty commit must not wipe what was typed
            if !p.is_empty() {
                h.ui_net.password = p.clone();
            }
            Ok(None)
        }
        ("joinpw", _) => {
            let ssid = h.ui_net.join.clone().ok_or("No network selected")?;
            let cmds = join(&st, &ssid, Some(&h.ui_net.password))?;
            h.ui_net.join = None;
            h.ui_net.password.clear();
            Ok(Some(Action::new(Dom::Network, format!("Join {ssid}"), cmds).msg(format!("Connected to {ssid}"))))
        }
        ("cancel", _) => {
            h.ui_net.join = None;
            h.ui_net.password.clear();
            Ok(None)
        }
        ("down", _) => {
            let c = conn(arg).ok_or("That connection is gone")?;
            Ok(Some(Action::new(Dom::Network, format!("Disconnect {}", c.name), disconnect(&c))))
        }
        ("forget", _) => {
            let c = conn(arg).ok_or("That connection is gone")?;
            Ok(Some(Action::new(Dom::Network, format!("Forget {}", c.name), forget(&c)).msg(format!("Forgot {}", c.name))))
        }
        ("vpn", CtlVal::Bool(on)) => {
            let c = conn(arg).ok_or("That connection is gone")?;
            Ok(Some(Action::new(Dom::Network, format!("VPN {}", c.name), vpn_toggle(&c, *on)).undo(vpn_toggle(&c, !*on))))
        }
        _ => Err("Unsupported network change".into()),
    })();
    match res {
        Ok(Some(a)) => {
            if act == "wifi" {
                if let (Some(s), CtlVal::Bool(b)) = (h.net.data.as_mut(), &val) {
                    s.wifi = Some(*b);
                }
            }
            h.run(a);
        }
        Ok(None) => h.dirty = true,
        Err(e) => h.say(e, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::{FakeRunner, Out};
    use crate::settings::system::{exec_action, Hub};
    use std::sync::Arc;

    const WIFI: &str = " :CafeFree:55:\n*:HomeNet:82:WPA2\n :Neighbor\\:5G:71:WPA2 WPA3\n :HomeNet:40:WPA2\n :Guest:20:--\n ::90:WPA2\n";
    const CONNS: &str = "Wired connection 1:9f839627-acca:802-3-ethernet:enp5s0\nHomeNet:aaaa-1111:802-11-wireless:wlan0\nOldCafe:bbbb-2222:802-11-wireless:\nWork VPN:cccc-3333:vpn:\nwg0:dddd-4444:wireguard:wg0\nlo:1979:loopback:lo\n";
    const SHOW: &str = "GENERAL.DEVICE:enp5s0\nGENERAL.TYPE:ethernet\nGENERAL.STATE:100 (connected)\nGENERAL.CONNECTION:Wired connection 1\nGENERAL.HWADDR:9C:6B:00:22:DD:D8\nIP4.ADDRESS[1]:192.168.1.150/24\nIP4.GATEWAY:192.168.1.1\nIP4.DNS[1]:8.8.8.8\nIP4.DNS[2]:8.8.4.4\n\nGENERAL.DEVICE:wlan0\nGENERAL.TYPE:wifi\nGENERAL.STATE:30 (disconnected)\nGENERAL.CONNECTION:\nGENERAL.HWADDR:AA\\:BB\\:CC\\:DD\\:EE\\:FF\nIP4.GATEWAY:\n\nGENERAL.DEVICE:lo\nGENERAL.TYPE:loopback\nGENERAL.STATE:10 (unmanaged)\nIP4.ADDRESS[1]:127.0.0.1/8\n";

    #[test]
    fn terse_splitting_handles_escapes() {
        assert_eq!(split_terse("a:b\\:c:d"), ["a", "b:c", "d"]);
        assert_eq!(split_terse("x\\\\y:z"), ["x\\y", "z"]);
        assert_eq!(split_terse(""), [""]);
    }

    #[test]
    fn wifi_list_dedups_sorts_and_skips_hidden() {
        let aps = parse_wifi_list(WIFI);
        let names: Vec<_> = aps.iter().map(|a| a.ssid.as_str()).collect();
        assert_eq!(names, ["HomeNet", "Neighbor:5G", "CafeFree", "Guest"]);
        assert!(aps[0].in_use);
        assert_eq!(aps[0].signal, 82, "the in-use access point keeps its own signal");
        assert!(aps[1].secured() && !aps[3].secured());
        assert_eq!(aps[1].security, "WPA2 WPA3");
    }

    #[test]
    fn connections_skip_loopback() {
        let c = parse_connections(CONNS);
        assert_eq!(c.len(), 5);
        assert_eq!(c[1].name, "HomeNet");
        assert!(c[2].device.is_empty());
        assert!(is_vpn(&c[3]) && is_vpn(&c[4]));
    }

    #[test]
    fn device_show_blocks() {
        let d = parse_device_show(SHOW);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "enp5s0");
        assert_eq!(d[0].state, "connected");
        assert_eq!(d[0].ip4, ["192.168.1.150/24"]);
        assert_eq!(d[0].dns, ["8.8.8.8", "8.8.4.4"]);
        assert_eq!(d[0].gw, "192.168.1.1");
        assert_eq!(d[1].hw, "AA:BB:CC:DD:EE:FF");
        assert_eq!(d[1].state, "disconnected");
    }

    fn st() -> State {
        State { available: true, error: None, wifi: Some(true), aps: parse_wifi_list(WIFI), conns: parse_connections(CONNS), devs: parse_device_show(SHOW) }
    }

    #[test]
    fn join_builds_commands() {
        let s = st();
        let l = |r: Result<Vec<Cmd>, String>| r.unwrap().iter().map(|c| c.line()).collect::<Vec<_>>();
        assert_eq!(l(join(&s, "CafeFree", None)), ["nmcli device wifi connect CafeFree"]);
        assert_eq!(l(join(&s, "Neighbor:5G", Some("hunter2hunter2"))), ["nmcli device wifi connect Neighbor:5G password hunter2hunter2"]);
        assert_eq!(l(join(&s, "OldCafe", None)), ["nmcli connection up uuid bbbb-2222"], "a saved network is brought up, no password asked");
        assert!(join(&s, "X", Some("short")).is_err());
        assert!(join(&s, "-bad", None).is_err());
        assert!(join(&s, "", None).is_err());
        assert!(valid_psk(&"g".repeat(64)).is_err());
        assert!(valid_psk(&"ab12".repeat(16)).is_ok());
        assert!(valid_psk("12345678").is_ok());
    }

    #[test]
    fn other_setters() {
        let s = st();
        assert_eq!(set_wifi(false)[0].line(), "nmcli radio wifi off");
        assert_eq!(rescan()[0].line(), "nmcli device wifi rescan");
        assert_eq!(forget(&s.conns[2])[0].line(), "nmcli connection delete uuid bbbb-2222");
        assert_eq!(vpn_toggle(&s.conns[3], true)[0].line(), "nmcli connection up uuid cccc-3333");
        assert_eq!(vpn_toggle(&s.conns[3], false)[0].line(), "nmcli connection down uuid cccc-3333");
        assert_eq!(disconnect(&s.conns[1])[0].line(), "nmcli connection down uuid aaaa-1111");
    }

    #[test]
    fn load_states() {
        let r = FakeRunner::new()
            .ok("nmcli -t -f WIFI radio", "enabled\n")
            .ok("nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY dev wifi list", WIFI)
            .ok("nmcli -t -f NAME,UUID,TYPE,DEVICE connection show", CONNS)
            .ok("nmcli -t -f GENERAL.DEVICE,GENERAL.TYPE,GENERAL.STATE,GENERAL.CONNECTION,GENERAL.HWADDR,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS device show", SHOW);
        let s = load(&r);
        assert!(s.available && s.wifi == Some(true));
        assert_eq!(s.aps.len(), 4);
        // radio off: no scan list requested
        let r = FakeRunner::lenient().ok("nmcli -t -f WIFI radio", "disabled\n");
        let s = load(&r);
        assert_eq!(s.wifi, Some(false));
        assert!(!r.lines().iter().any(|l| l.contains("wifi list")));
        // no nmcli
        assert!(load(&FakeRunner::new()).error.unwrap().contains("not installed"));
        // NetworkManager down
        let r = FakeRunner::new().on("nmcli -t -f WIFI radio", Out::fail("Error: NetworkManager is not running."));
        assert!(!load(&r).available);
    }

    #[test]
    fn items_show_sheet_and_hide_wifi_without_adapter() {
        let mut h = Hub::with(Arc::new(FakeRunner::lenient()), std::env::temp_dir().join("ms-net-t"), Arc::new(|| {}));
        h.net.data = Some(st());
        h.ui_net.join = Some("Neighbor:5G".into());
        let it = items(&h);
        assert!(matches!(&it[0], Item::Header(t) if t.contains("Neighbor:5G")));
        let ids: Vec<String> = it.iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().flat_map(|r| r.ctls.iter().map(|c| c.id.clone())).collect();
        assert!(ids.contains(&"s:net:pw:secret".to_string()));
        assert!(ids.contains(&"s:net:vpn:cccc-3333".to_string()));
        assert!(ids.contains(&"s:net:join:CafeFree".to_string()));
        assert!(ids.contains(&"s:net:forget:bbbb-2222".to_string()));
        assert!(ids.contains(&"s:net:down:aaaa-1111".to_string()));
        let mut s = st();
        s.devs.retain(|d| d.kind != "wifi");
        s.wifi = Some(true);
        h.net.data = Some(s);
        h.ui_net.join = None;
        let it = items(&h);
        assert!(!it.iter().any(|i| matches!(i, Item::Header(t) if t == "Available networks")));
    }

    #[test]
    fn password_flow_and_validation_errors() {
        let fr = Arc::new(FakeRunner::lenient());
        let mut h = Hub::with(fr.clone(), std::env::temp_dir().join("ms-net-t2"), Arc::new(|| {}));
        h.net.data = Some(st());
        apply(&mut h, "join", "Neighbor:5G", CtlVal::Click);
        assert_eq!(h.ui_net.join.as_deref(), Some("Neighbor:5G"));
        assert!(fr.lines().is_empty(), "secured unknown network asks first");
        apply(&mut h, "pw", "short", CtlVal::Str("short".into()));
        apply(&mut h, "joinpw", "", CtlVal::Click);
        assert!(h.msgs.last().unwrap().1, "too short is rejected inline");
        assert!(fr.lines().is_empty());
        apply(&mut h, "pw", "", CtlVal::Str("longenough1".into()));
        apply(&mut h, "joinpw", "", CtlVal::Click);
        for _ in 0..100 {
            if !fr.lines().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(fr.lines(), ["nmcli device wifi connect Neighbor:5G password longenough1"]);
        assert!(h.ui_net.join.is_none() && h.ui_net.password.is_empty());
    }

    #[test]
    fn auth_failure_is_surfaced() {
        let a = Action::new(Dom::Network, "Join X", vec![Cmd::new("nmcli", &["device", "wifi", "connect", "X"])]);
        let r = FakeRunner::new().on("nmcli device wifi connect X", Out::fail("Error: Connection activation failed: Secrets were required, but not provided."));
        let d = exec_action(&r, &std::env::temp_dir(), &a);
        assert!(d.msg.unwrap().0.contains("Secrets were required"));
    }
}
