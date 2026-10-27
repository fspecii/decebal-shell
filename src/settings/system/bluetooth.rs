//! Bluetooth: power, discoverable, scan, pair/connect/forget. Backend: `bluetoothctl`.

use super::run::{Cmd, Runner};
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Row};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Device {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub available: bool,
    pub error: Option<String>,
    pub powered: bool,
    pub discoverable: bool,
    pub discovering: bool,
    pub devices: Vec<Device>,
}

#[derive(Default)]
pub struct Ui {
    pub scanning: bool,
}

pub fn valid_mac(s: &str) -> bool {
    let p: Vec<&str> = s.split(':').collect();
    p.len() == 6 && p.iter().all(|x| x.len() == 2 && x.chars().all(|c| c.is_ascii_hexdigit()))
}

fn yes(text: &str, key: &str) -> Option<bool> {
    text.lines().find_map(|l| l.trim().strip_prefix(key)?.trim().strip_prefix(':').map(|v| v.trim().starts_with("yes")))
}

/// `bluetoothctl show`. None = no controller.
pub fn parse_show(text: &str) -> Option<(bool, bool, bool)> {
    if text.contains("No default controller") || !text.contains("Powered:") {
        return None;
    }
    Some((yes(text, "Powered").unwrap_or(false), yes(text, "Discoverable").unwrap_or(false), yes(text, "Discovering").unwrap_or(false)))
}

/// `bluetoothctl devices`: "Device AA:BB:CC:DD:EE:FF Name".
pub fn parse_devices(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let r = l.trim().strip_prefix("Device ")?;
            let (mac, name) = r.split_once(' ').unwrap_or((r, ""));
            valid_mac(mac).then(|| (mac.to_string(), if name.trim().is_empty() { mac.to_string() } else { name.trim().to_string() }))
        })
        .collect()
}

/// `bluetoothctl info MAC`.
pub fn parse_info(mac: &str, name: &str, text: &str) -> Device {
    let icon = text.lines().find_map(|l| l.trim().strip_prefix("Icon:")).map(|s| s.trim().to_string()).unwrap_or_default();
    Device { mac: mac.into(), name: name.into(), paired: yes(text, "Paired").unwrap_or(false), connected: yes(text, "Connected").unwrap_or(false), trusted: yes(text, "Trusted").unwrap_or(false), icon }
}

pub fn load(r: &dyn Runner) -> State {
    if !r.exists("bluetoothctl") {
        return State { error: Some("bluetoothctl (BlueZ) is not installed.".into()), ..State::default() };
    }
    let show = r.run(&Cmd::new("bluetoothctl", &["show"]).timeout(4));
    if !show.ok && show.stdout.is_empty() {
        let why = if show.timed_out { "The Bluetooth service did not answer (is bluetooth.service running?)".to_string() } else { show.error_text("bluetoothctl") };
        return State { error: Some(why), ..State::default() };
    }
    let Some((powered, discoverable, discovering)) = parse_show(&show.stdout) else {
        return State { error: Some("No Bluetooth adapter was found.".into()), ..State::default() };
    };
    let mut devices = Vec::new();
    if powered {
        for (mac, name) in parse_devices(&r.run(&Cmd::new("bluetoothctl", &["devices"]).timeout(4)).stdout).into_iter().take(24) {
            let info = r.run(&Cmd::new("bluetoothctl", &["info", &mac]).timeout(4));
            devices.push(parse_info(&mac, &name, &info.stdout));
        }
    }
    devices.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.paired.cmp(&a.paired)).then(a.name.cmp(&b.name)));
    State { available: true, error: None, powered, discoverable, discovering, devices }
}

pub fn power(on: bool) -> Vec<Cmd> {
    vec![Cmd::new("bluetoothctl", &["power", if on { "on" } else { "off" }]).timeout(10)]
}

pub fn discoverable(on: bool) -> Vec<Cmd> {
    vec![Cmd::new("bluetoothctl", &["discoverable", if on { "on" } else { "off" }])]
}

pub fn scan() -> Vec<Cmd> {
    vec![Cmd::new("bluetoothctl", &["--timeout", "8", "scan", "on"]).timeout(12)]
}

pub fn device_cmd(verb: &str, mac: &str) -> Result<Vec<Cmd>, String> {
    if !valid_mac(mac) {
        return Err("Invalid device address".into());
    }
    Ok(match verb {
        "pair" => vec![Cmd::new("bluetoothctl", &["pair", mac]).timeout(40), Cmd::new("bluetoothctl", &["trust", mac]), Cmd::new("bluetoothctl", &["connect", mac]).timeout(25)],
        "connect" => vec![Cmd::new("bluetoothctl", &["connect", mac]).timeout(25)],
        "disconnect" => vec![Cmd::new("bluetoothctl", &["disconnect", mac])],
        "forget" => vec![Cmd::new("bluetoothctl", &["remove", mac])],
        _ => return Err("Unsupported Bluetooth action".into()),
    })
}

pub fn items(h: &Hub) -> Vec<Item> {
    let Some(st) = &h.bt.data else { return vec![Item::Note { text: "Reading Bluetooth state...".into(), warn: false }] };
    if !st.available {
        return super::sound::unavailable("Bluetooth", st.error.as_deref().unwrap_or("Bluetooth is not available."));
    }
    let mut v = vec![Item::Header("Adapter".into())];
    let mut rows = vec![Row::new("Bluetooth").detail(if st.powered { "On" } else { "Off" }).ctl(Ctl::new("s:bt:power:", Kind::Toggle(st.powered)))];
    if st.powered {
        rows.push(Row::new("Discoverable").detail("Let other devices find this computer").ctl(Ctl::new("s:bt:disc:", Kind::Toggle(st.discoverable))));
    }
    v.push(Item::Card(rows));
    if !st.powered {
        return v;
    }
    v.push(Item::Header("Devices".into()));
    let mut rows: Vec<Row> = st
        .devices
        .iter()
        .map(|d| {
            let state = if d.connected {
                "Connected"
            } else if d.paired {
                "Paired"
            } else {
                "Nearby"
            };
            let mut r = Row::new(d.name.clone()).detail(format!("{state} · {}", d.mac));
            r.selected = d.connected;
            if d.connected {
                r = r.ctl(Ctl::new(format!("s:bt:disconnect:{}", d.mac), Kind::Button { label: "Disconnect".into(), danger: false }));
            } else if d.paired {
                r = r.ctl(Ctl::new(format!("s:bt:connect:{}", d.mac), Kind::Button { label: "Connect".into(), danger: false }));
            } else {
                r = r.ctl(Ctl::new(format!("s:bt:pair:{}", d.mac), Kind::Button { label: "Pair".into(), danger: false }));
            }
            if d.paired {
                r = r.ctl(Ctl::new(format!("s:bt:forget:{}", d.mac), Kind::Button { label: "Forget".into(), danger: true }));
            }
            r
        })
        .collect();
    if rows.is_empty() {
        rows.push(Row::new("No devices yet").detail("Put your device in pairing mode and scan"));
    }
    rows.push(Row::new("Scan for devices").detail("Looks for eight seconds").ctl(Ctl::new("s:bt:scan:", Kind::Button { label: if h.ui_bt.scanning || st.discovering { "Scanning...".into() } else { "Scan".into() }, danger: false })));
    v.push(Item::Card(rows));
    v
}

pub fn apply(h: &mut Hub, act: &str, arg: &str, val: CtlVal) {
    let res: Result<Action, String> = match (act, &val) {
        ("power", CtlVal::Bool(on)) => Ok(Action::new(Dom::Bluetooth, "Bluetooth", power(*on)).undo(power(!*on))),
        ("disc", CtlVal::Bool(on)) => Ok(Action::new(Dom::Bluetooth, "Discoverable", discoverable(*on)).undo(discoverable(!*on))),
        ("scan", _) => {
            h.ui_bt.scanning = true;
            Ok(Action::new(Dom::Bluetooth, "Scan", scan()))
        }
        ("pair" | "connect" | "disconnect" | "forget", CtlVal::Click) => {
            let name = h.bt.data.as_ref().and_then(|s| s.devices.iter().find(|d| d.mac == arg)).map(|d| d.name.clone()).unwrap_or_else(|| arg.to_string());
            device_cmd(act, arg).map(|c| {
                let verb = match act {
                    "pair" => "Pair",
                    "connect" => "Connect",
                    "disconnect" => "Disconnect",
                    _ => "Forget",
                };
                Action::new(Dom::Bluetooth, format!("{verb} {name}"), c)
            })
        }
        _ => Err("Unsupported Bluetooth change".into()),
    };
    match res {
        Ok(a) => {
            if let (Some(s), CtlVal::Bool(b)) = (h.bt.data.as_mut(), &val) {
                match act {
                    "power" => s.powered = *b,
                    "disc" => s.discoverable = *b,
                    _ => {}
                }
            }
            h.run(a);
        }
        Err(e) => h.say(e, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::{FakeRunner, Out};
    use std::sync::Arc;

    const SHOW: &str = "Controller 00:1A:7D:DA:71:13 (public)\n\tName: omarchy\n\tAlias: omarchy\n\tPowered: yes\n\tDiscoverable: no\n\tPairable: yes\n\tDiscovering: no\n";
    const DEVS: &str = "Device 11:22:33:44:55:66 Sony WH-1000XM4\nDevice AA:BB:CC:DD:EE:FF Keyboard K380\nDevice 12:34:56:78:9A:BC 12-34-56-78-9A-BC\nnot a device\nDevice ZZ Bad\n";
    const INFO_SONY: &str = "Device 11:22:33:44:55:66 (public)\n\tName: Sony WH-1000XM4\n\tIcon: audio-headset\n\tPaired: yes\n\tTrusted: yes\n\tConnected: yes\n";
    const INFO_KB: &str = "Device AA:BB:CC:DD:EE:FF (public)\n\tPaired: yes\n\tTrusted: no\n\tConnected: no\n";

    #[test]
    fn parses_show() {
        assert_eq!(parse_show(SHOW), Some((true, false, false)));
        assert_eq!(parse_show("No default controller available\n"), None);
        assert_eq!(parse_show(""), None);
        assert_eq!(parse_show("\tPowered: no\n\tDiscoverable: yes\n"), Some((false, true, false)));
    }

    #[test]
    fn parses_devices_and_info() {
        let d = parse_devices(DEVS);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0], ("11:22:33:44:55:66".into(), "Sony WH-1000XM4".into()));
        let i = parse_info("11:22:33:44:55:66", "Sony", INFO_SONY);
        assert!(i.paired && i.connected && i.trusted);
        assert_eq!(i.icon, "audio-headset");
        let k = parse_info("a", "k", INFO_KB);
        assert!(k.paired && !k.connected && !k.trusted);
    }

    #[test]
    fn mac_validation() {
        assert!(valid_mac("AA:BB:CC:DD:EE:FF"));
        assert!(!valid_mac("AA:BB:CC:DD:EE"));
        assert!(!valid_mac("AA:BB:CC:DD:EE:GG"));
        assert!(!valid_mac("-h"));
        assert!(device_cmd("pair", "--help").is_err());
    }

    #[test]
    fn commands() {
        let l = |v: Vec<Cmd>| v.iter().map(|c| c.line()).collect::<Vec<_>>();
        assert_eq!(l(power(true)), ["bluetoothctl power on"]);
        assert_eq!(l(discoverable(false)), ["bluetoothctl discoverable off"]);
        assert_eq!(l(scan()), ["bluetoothctl --timeout 8 scan on"]);
        assert_eq!(l(device_cmd("pair", "AA:BB:CC:DD:EE:FF").unwrap()), ["bluetoothctl pair AA:BB:CC:DD:EE:FF", "bluetoothctl trust AA:BB:CC:DD:EE:FF", "bluetoothctl connect AA:BB:CC:DD:EE:FF"]);
        assert_eq!(l(device_cmd("forget", "AA:BB:CC:DD:EE:FF").unwrap()), ["bluetoothctl remove AA:BB:CC:DD:EE:FF"]);
        assert_eq!(l(device_cmd("disconnect", "AA:BB:CC:DD:EE:FF").unwrap()), ["bluetoothctl disconnect AA:BB:CC:DD:EE:FF"]);
    }

    #[test]
    fn load_sorts_connected_first_and_degrades() {
        let r = FakeRunner::new()
            .ok("bluetoothctl show", SHOW)
            .ok("bluetoothctl devices", DEVS)
            .ok("bluetoothctl info 11:22:33:44:55:66", INFO_SONY)
            .ok("bluetoothctl info AA:BB:CC:DD:EE:FF", INFO_KB)
            .ok("bluetoothctl info 12:34:56:78:9A:BC", "Device 12:34:56:78:9A:BC\n\tPaired: no\n");
        let s = load(&r);
        assert!(s.available && s.powered);
        assert_eq!(s.devices[0].name, "Sony WH-1000XM4");
        assert_eq!(s.devices[1].name, "Keyboard K380");
        assert!(!s.devices[2].paired);
        // powered off: no device queries
        let r = FakeRunner::lenient().ok("bluetoothctl show", "\tPowered: no\n");
        let s = load(&r);
        assert!(s.available && !s.powered && s.devices.is_empty());
        assert_eq!(r.lines(), ["bluetoothctl show"]);
        // not installed / no adapter / hung daemon
        assert!(load(&FakeRunner::new()).error.unwrap().contains("not installed"));
        let r = FakeRunner::new().ok("bluetoothctl show", "No default controller available\n");
        assert!(load(&r).error.unwrap().contains("No Bluetooth adapter"));
        let r = FakeRunner::new().on("bluetoothctl show", Out { timed_out: true, ..Out::default() });
        assert!(load(&r).error.unwrap().contains("did not answer"));
    }

    #[test]
    fn items_offer_the_right_buttons() {
        let mut h = Hub::with(Arc::new(FakeRunner::lenient()), std::env::temp_dir().join("ms-bt-t"), Arc::new(|| {}));
        h.bt.data = Some(State {
            available: true,
            powered: true,
            devices: vec![parse_info("11:22:33:44:55:66", "Sony", INFO_SONY), parse_info("AA:BB:CC:DD:EE:FF", "KB", INFO_KB), parse_info("12:34:56:78:9A:BC", "New", "")],
            ..State::default()
        });
        let ids: Vec<String> = items(&h).iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().flat_map(|r| r.ctls.iter().map(|c| c.id.clone())).collect();
        for want in ["s:bt:power:", "s:bt:disc:", "s:bt:disconnect:11:22:33:44:55:66", "s:bt:connect:AA:BB:CC:DD:EE:FF", "s:bt:pair:12:34:56:78:9A:BC", "s:bt:forget:AA:BB:CC:DD:EE:FF", "s:bt:scan:"] {
            assert!(ids.contains(&want.to_string()), "{want} in {ids:?}");
        }
        assert!(!ids.contains(&"s:bt:forget:12:34:56:78:9A:BC".to_string()));
        h.bt.data.as_mut().unwrap().powered = false;
        let n = items(&h).len();
        assert_eq!(n, 2, "powered off: header + power card only");
    }
}
