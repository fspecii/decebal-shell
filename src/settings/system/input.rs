//! Keyboard & Input: layout (setxkbmap / localectl), Caps Lock behaviour (XKB options), key repeat (xset r rate),
//! pointer speed / natural scrolling / tap-to-click (xinput + libinput properties). Everything is also written to
//! the session settings so it is applied again at login.

use super::run::{Cmd, Runner};
use super::session::{Session, XinputSet};
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Opt, Row};

pub const P_ACCEL: &str = "libinput Accel Speed";
pub const P_NATURAL: &str = "libinput Natural Scrolling Enabled";
pub const P_TAP: &str = "libinput Tapping Enabled";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pointer {
    pub id: u32,
    pub name: String,
    pub accel: Option<f32>,
    pub natural: Option<bool>,
    pub tap: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub has_xkb: bool,
    pub has_xset: bool,
    pub has_xinput: bool,
    pub layout: String,
    pub variant: String,
    pub options: Vec<String>,
    pub layouts: Vec<String>,
    pub repeat: Option<(u32, u32)>,
    pub pointers: Vec<Pointer>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------- parsers

/// `setxkbmap -query` -> (layout, variant, options).
pub fn parse_xkb_query(text: &str) -> (String, String, Vec<String>) {
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_string()).unwrap_or_default();
    let opts = get("options:");
    (get("layout:"), get("variant:"), opts.split(',').filter(|o| !o.is_empty()).map(String::from).collect())
}

/// `xset q` -> (delay ms, rate Hz).
pub fn parse_xset_repeat(text: &str) -> Option<(u32, u32)> {
    let l = text.lines().find(|l| l.contains("auto repeat delay"))?;
    let nums: Vec<u32> = l.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    (nums.len() >= 2).then(|| (nums[0], nums[1]))
}

/// `xset q` -> (blank timeout s, dpms enabled, dpms off s).
pub fn parse_xset_screen(text: &str) -> (Option<u32>, Option<bool>, Option<u32>) {
    let timeout = text.lines().find_map(|l| l.trim().strip_prefix("timeout:")).and_then(|r| r.split_whitespace().next()?.parse().ok());
    let dpms = text.lines().find_map(|l| l.trim().strip_prefix("DPMS is ")).map(|r| r.trim().starts_with("Enabled"));
    let off = text.lines().find_map(|l| l.trim().strip_prefix("Standby:")).and_then(|r| r.split("Off:").nth(1)?.split_whitespace().next()?.parse().ok());
    (timeout, dpms, off)
}

/// `xinput list`: slave pointers (id, name), skipping virtual XTEST devices.
pub fn parse_xinput_list(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .filter(|l| l.contains("slave") && l.contains("pointer") && l.contains("id="))
        .filter_map(|l| {
            let (left, right) = l.split_once("id=")?;
            let id: u32 = right.split_whitespace().next()?.parse().ok()?;
            let name = left.trim_start_matches(|c: char| !c.is_alphanumeric()).trim().to_string();
            (!name.contains("XTEST") && !name.is_empty()).then_some((id, name))
        })
        .collect()
}

/// `xinput list-props ID` -> libinput properties we care about.
pub fn parse_props(id: u32, name: &str, text: &str) -> Pointer {
    let val = |p: &str| text.lines().find(|l| l.trim_start().starts_with(p)).and_then(|l| l.rsplit(':').next()).map(|v| v.trim().to_string());
    Pointer { id, name: name.into(), accel: val(P_ACCEL).and_then(|v| v.parse().ok()), natural: val(P_NATURAL).map(|v| v.starts_with('1')), tap: val(P_TAP).map(|v| v.starts_with('1')) }
}

pub fn load(r: &dyn Runner) -> State {
    let mut st = State { has_xkb: r.exists("setxkbmap"), has_xset: r.exists("xset"), has_xinput: r.exists("xinput"), ..State::default() };
    if st.has_xkb {
        let q = r.run(&Cmd::new("setxkbmap", &["-query"]).timeout(5));
        if q.ok {
            (st.layout, st.variant, st.options) = parse_xkb_query(&q.stdout);
        } else {
            st.error = Some(format!("No X server to talk to: {}", q.error_text("setxkbmap")));
        }
    }
    if r.exists("localectl") {
        let l = r.run(&Cmd::new("localectl", &["list-x11-keymap-layouts"]));
        if l.ok {
            st.layouts = l.stdout.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
    }
    if st.has_xset {
        let q = r.run(&Cmd::new("xset", &["q"]).timeout(5));
        if q.ok {
            st.repeat = parse_xset_repeat(&q.stdout);
        }
    }
    if st.has_xinput {
        let l = r.run(&Cmd::new("xinput", &["list"]).timeout(5));
        if l.ok {
            for (id, name) in parse_xinput_list(&l.stdout).into_iter().take(12) {
                let p = r.run(&Cmd::new("xinput", &["list-props", &id.to_string()]).timeout(5));
                let ptr = parse_props(id, &name, &p.stdout);
                if ptr.accel.is_some() || ptr.natural.is_some() || ptr.tap.is_some() {
                    st.pointers.push(ptr);
                }
            }
        }
    }
    st
}

// ---------------------------------------------------------------- setters

pub fn valid_layout(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ',')
}

pub fn valid_option(s: &str) -> bool {
    !s.is_empty() && s.len() <= 60 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '-')
}

pub fn setxkbmap_cmd(layout: &str, variant: &str, options: &[String]) -> Result<Cmd, String> {
    if !valid_layout(layout) {
        return Err(format!("\"{layout}\" is not a valid keyboard layout"));
    }
    if !variant.is_empty() && !valid_layout(variant) {
        return Err(format!("\"{variant}\" is not a valid layout variant"));
    }
    if let Some(o) = options.iter().find(|o| !valid_option(o)) {
        return Err(format!("\"{o}\" is not a valid keyboard option"));
    }
    let mut a: Vec<String> = vec!["-layout".into(), layout.into(), "-variant".into(), variant.into(), "-option".into(), "".into()];
    for o in options {
        a.push("-option".into());
        a.push(o.clone());
    }
    Ok(Cmd::owned("setxkbmap", a))
}

pub fn caps_options() -> Vec<Opt> {
    vec![
        ("Caps Lock (default)".into(), "".into()),
        ("Control".into(), "ctrl:nocaps".into()),
        ("Escape".into(), "caps:escape".into()),
        ("Swap with Control".into(), "ctrl:swapcaps".into()),
        ("Disabled".into(), "caps:none".into()),
    ]
}

/// Replaces the Caps Lock related option, keeps every other option.
pub fn with_caps(options: &[String], caps: &str) -> Vec<String> {
    let mut v: Vec<String> = options.iter().filter(|o| !(o.starts_with("ctrl:nocaps") || o.starts_with("ctrl:swapcaps") || o.starts_with("caps:"))).cloned().collect();
    if !caps.is_empty() {
        v.push(caps.to_string());
    }
    v
}

pub fn caps_of(options: &[String]) -> String {
    options.iter().find(|o| o.starts_with("ctrl:nocaps") || o.starts_with("ctrl:swapcaps") || o.starts_with("caps:")).cloned().unwrap_or_default()
}

pub fn repeat_cmd(delay: u32, rate: u32) -> Result<Cmd, String> {
    if !(100..=2000).contains(&delay) {
        return Err("Repeat delay must be between 100 and 2000 ms".into());
    }
    if !(1..=100).contains(&rate) {
        return Err("Repeat rate must be between 1 and 100 per second".into());
    }
    Ok(Cmd::new("xset", &["r", "rate", &delay.to_string(), &rate.to_string()]))
}

pub fn prop_cmd(id: u32, prop: &str, value: &str) -> Result<Cmd, String> {
    if ![P_ACCEL, P_NATURAL, P_TAP].contains(&prop) {
        return Err("Unsupported pointer setting".into());
    }
    match prop {
        P_ACCEL => {
            let v: f32 = value.parse().map_err(|_| "Pointer speed must be a number")?;
            if !(-1.0..=1.0).contains(&v) {
                return Err("Pointer speed must be between -1 and 1".into());
            }
        }
        _ => {
            if value != "0" && value != "1" {
                return Err("Invalid switch value".into());
            }
        }
    }
    Ok(Cmd::new("xinput", &["set-prop", &id.to_string(), prop, value]))
}

pub fn upsert_xinput(s: &mut Session, device: &str, prop: &str, value: &str) {
    match s.xinput.iter_mut().find(|x| x.device == device && x.prop == prop) {
        Some(x) => x.value = value.into(),
        None => s.xinput.push(XinputSet { device: device.into(), prop: prop.into(), value: value.into() }),
    }
}

// ---------------------------------------------------------------- pane

pub fn items(h: &Hub) -> Vec<Item> {
    let Some(st) = &h.input.data else { return vec![Item::Note { text: "Reading keyboard and pointer settings...".into(), warn: false }] };
    let mut v = Vec::new();
    if let Some(e) = &st.error {
        v.push(Item::Note { text: e.clone(), warn: true });
    }
    if super::run::wayland_session() {
        v.push(Item::Note {
            text: "This is a Wayland session: layout, key repeat and pointer settings below only reach X11 apps (XWayland). Configure the compositor for native apps.".into(),
            warn: false,
        });
    }
    v.push(Item::Header("Keyboard".into()));
    let mut rows = Vec::new();
    if st.has_xkb && st.error.is_none() {
        let opts: Vec<Opt> = if st.layouts.is_empty() { vec![(st.layout.clone(), st.layout.clone())] } else { st.layouts.iter().map(|l| (l.clone(), l.clone())).collect() };
        let sel = opts.iter().position(|(_, v)| *v == st.layout).or_else(|| {
            // "us,de" style: select the first
            let first = st.layout.split(',').next().unwrap_or("");
            opts.iter().position(|(_, v)| v == first)
        });
        rows.push(Row::new("Keyboard layout").detail(if st.variant.is_empty() { String::new() } else { format!("Variant {}", st.variant) }).ctl(Ctl::new("s:inp:layout:", Kind::Popup(opts, sel))));
        let co = caps_options();
        let cur = caps_of(&st.options);
        let csel = co.iter().position(|(_, v)| *v == cur);
        rows.push(Row::new("Caps Lock key acts as").detail("Applied as an XKB option").ctl(Ctl::new("s:inp:caps:", Kind::Popup(co, csel))));
    } else if !st.has_xkb {
        rows.push(Row::new("Layout and Caps Lock").detail("Not available: setxkbmap is not installed"));
    }
    if st.has_xset {
        match st.repeat {
            Some((d, r)) => {
                rows.push(
                    Row::new("Key repeat delay")
                        .detail("How long to hold a key before it repeats")
                        .ctl(Ctl::new("s:inp:delay:", Kind::Slider { v: d as f64, min: 150.0, max: 1000.0, step: 10.0, unit: " ms".into() })),
                );
                rows.push(Row::new("Key repeat rate").detail("Repeats per second").ctl(Ctl::new("s:inp:rate:", Kind::Slider { v: r as f64, min: 5.0, max: 60.0, step: 1.0, unit: "/s".into() })));
            }
            None => rows.push(Row::new("Key repeat").detail("Not available: xset could not read the X server")),
        }
    } else {
        rows.push(Row::new("Key repeat").detail("Not available: xset is not installed"));
    }
    v.push(Item::Card(rows));
    v.push(Item::Header("Pointer".into()));
    if !st.has_xinput {
        v.push(Item::Card(vec![Row::new("Pointer settings").detail("Not available: xinput is not installed")]));
    } else if st.pointers.is_empty() {
        v.push(Item::Card(vec![Row::new("No adjustable pointing devices").detail("Only devices driven by libinput expose these settings")]));
    }
    for p in &st.pointers {
        v.push(Item::Header(p.name.clone()));
        let mut rows = Vec::new();
        if let Some(a) = p.accel {
            rows.push(Row::new("Pointer speed").ctl(Ctl::new(format!("s:inp:accel:{}", p.id), Kind::Slider { v: (a * 100.0).round() as f64, min: -100.0, max: 100.0, step: 5.0, unit: "".into() })));
        }
        if let Some(n) = p.natural {
            rows.push(Row::new("Natural scrolling").detail("Content follows the finger").ctl(Ctl::new(format!("s:inp:natural:{}", p.id), Kind::Toggle(n))));
        }
        if let Some(t) = p.tap {
            rows.push(Row::new("Tap to click").ctl(Ctl::new(format!("s:inp:tap:{}", p.id), Kind::Toggle(t))));
        }
        v.push(Item::Card(rows));
    }
    v
}

pub fn apply(h: &mut Hub, act: &str, arg: &str, val: CtlVal) {
    let Some(st) = h.input.data.clone() else { return };
    let r: Result<(), String> = (|| {
        match (act, &val) {
            ("layout", CtlVal::Str(l)) => {
                if !st.layouts.is_empty() && !st.layouts.contains(l) {
                    return Err(format!("Unknown layout \"{l}\""));
                }
                let cmd = setxkbmap_cmd(l, "", &st.options)?;
                let undo = setxkbmap_cmd(&st.layout, &st.variant, &st.options)?;
                let mut s = h.session.clone();
                s.kbd_layout = Some(l.clone());
                s.kbd_variant = Some(String::new());
                s.kbd_options = st.options.clone();
                h.run_session(Action::new(Dom::Input, format!("Keyboard layout {l}"), vec![cmd]).undo(vec![undo]), s);
            }
            ("caps", CtlVal::Str(c)) => {
                let opts = with_caps(&st.options, c);
                let cmd = setxkbmap_cmd(&st.layout, &st.variant, &opts)?;
                let undo = setxkbmap_cmd(&st.layout, &st.variant, &st.options)?;
                let mut s = h.session.clone();
                s.kbd_layout = Some(st.layout.clone());
                s.kbd_variant = Some(st.variant.clone());
                s.kbd_options = opts;
                h.run_session(Action::new(Dom::Input, "Caps Lock behaviour", vec![cmd]).undo(vec![undo]), s);
            }
            ("delay" | "rate", CtlVal::Num(n)) => {
                let (d0, r0) = st.repeat.unwrap_or((660, 25));
                let (d, r) = if act == "delay" { (*n as u32, r0) } else { (d0, *n as u32) };
                let cmd = repeat_cmd(d, r)?;
                let undo = repeat_cmd(d0, r0)?;
                let mut s = h.session.clone();
                s.repeat_delay = Some(d);
                s.repeat_rate = Some(r);
                if let Some(i) = h.input.data.as_mut() {
                    i.repeat = Some((d, r));
                }
                h.run_session(Action::new(Dom::Input, "Key repeat", vec![cmd]).undo(vec![undo]), s);
            }
            ("accel" | "natural" | "tap", _) => {
                let id: u32 = arg.parse().map_err(|_| "Unknown device")?;
                let p = st.pointers.iter().find(|p| p.id == id).ok_or("That device is gone")?;
                let (prop, new, old) = match (act, &val) {
                    ("accel", CtlVal::Num(n)) => (P_ACCEL, format!("{:.2}", n / 100.0), format!("{:.2}", p.accel.unwrap_or(0.0))),
                    ("natural", CtlVal::Bool(b)) => (P_NATURAL, (*b as u8).to_string(), (p.natural.unwrap_or(false) as u8).to_string()),
                    ("tap", CtlVal::Bool(b)) => (P_TAP, (*b as u8).to_string(), (p.tap.unwrap_or(false) as u8).to_string()),
                    _ => return Err("Unsupported pointer change".into()),
                };
                let cmd = prop_cmd(id, prop, &new)?;
                let undo = prop_cmd(id, prop, &old)?;
                let mut s = h.session.clone();
                upsert_xinput(&mut s, &p.name, prop, &new);
                let name = p.name.clone();
                if let Some(i) = h.input.data.as_mut() {
                    if let Some(pp) = i.pointers.iter_mut().find(|p| p.id == id) {
                        match prop {
                            P_ACCEL => pp.accel = new.parse().ok(),
                            P_NATURAL => pp.natural = Some(new == "1"),
                            _ => pp.tap = Some(new == "1"),
                        }
                    }
                }
                h.run_session(Action::new(Dom::Input, format!("{name}: {}", prop.trim_start_matches("libinput ").to_lowercase()), vec![cmd]).undo(vec![undo]), s);
            }
            _ => return Err("Unsupported input change".into()),
        }
        Ok(())
    })();
    if let Err(e) = r {
        h.say(e, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::FakeRunner;

    const XKB: &str = "rules:      evdev\nmodel:      pc105\nlayout:     us,de\nvariant:    ,nodeadkeys\noptions:    ctrl:nocaps,compose:ralt\n";
    const XSET: &str = "Keyboard Control:\n  auto repeat:  on    key click percent:  0    LED mask:  00000000\n  auto repeat delay:  660    repeat rate:  25\nScreen Saver:\n  prefer blanking:  yes    allow exposures:  yes\n  timeout:  600    cycle:  600\nDPMS (Energy Star):\n  Standby: 0    Suspend: 0    Off: 900\n  DPMS is Enabled\n";
    const XLIST: &str = "⎡ Virtual core pointer                    \tid=2\t[master pointer  (3)]\n⎜   ↳ Virtual core XTEST pointer              \tid=4\t[slave  pointer  (2)]\n⎜   ↳ Logitech MX Master                      \tid=10\t[slave  pointer  (2)]\n⎜   ↳ SYNA Touchpad                           \tid=11\t[slave  pointer  (2)]\n⎣ Virtual core keyboard                   \tid=3\t[master keyboard (2)]\n    ↳ AT keyboard                             \tid=12\t[slave  keyboard (3)]\n";
    const PROPS: &str =
        "Device 'SYNA Touchpad':\n\tDevice Enabled (178):\t1\n\tlibinput Tapping Enabled (320):\t1\n\tlibinput Natural Scrolling Enabled (330):\t0\n\tlibinput Accel Speed (340):\t-0.250000\n";

    #[test]
    fn parses_xkb_and_xset() {
        let (l, v, o) = parse_xkb_query(XKB);
        assert_eq!(l, "us,de");
        assert_eq!(v, ",nodeadkeys");
        assert_eq!(o, ["ctrl:nocaps", "compose:ralt"]);
        assert_eq!(parse_xset_repeat(XSET), Some((660, 25)));
        assert_eq!(parse_xset_screen(XSET), (Some(600), Some(true), Some(900)));
        assert_eq!(parse_xset_repeat("garbage"), None);
        assert_eq!(parse_xkb_query("layout: us\n").2.len(), 0);
    }

    #[test]
    fn parses_xinput() {
        let l = parse_xinput_list(XLIST);
        assert_eq!(l, [(10, "Logitech MX Master".into()), (11, "SYNA Touchpad".into())]);
        let p = parse_props(11, "SYNA Touchpad", PROPS);
        assert_eq!(p.accel, Some(-0.25));
        assert_eq!(p.natural, Some(false));
        assert_eq!(p.tap, Some(true));
        let none = parse_props(1, "x", "Device 'x':\n\tDevice Enabled (1):\t1\n");
        assert_eq!((none.accel, none.natural, none.tap), (None, None, None));
    }

    #[test]
    fn caps_option_replaces_only_its_group() {
        let o = vec!["ctrl:nocaps".to_string(), "compose:ralt".to_string()];
        assert_eq!(with_caps(&o, "caps:escape"), ["compose:ralt", "caps:escape"]);
        assert_eq!(with_caps(&o, ""), ["compose:ralt"]);
        assert_eq!(caps_of(&o), "ctrl:nocaps");
        assert_eq!(caps_of(&["compose:ralt".to_string()]), "");
        assert_eq!(with_caps(&["ctrl:swapcaps".to_string()], "ctrl:nocaps"), ["ctrl:nocaps"]);
    }

    #[test]
    fn setxkbmap_command_line_and_validation() {
        assert_eq!(setxkbmap_cmd("us", "", &["ctrl:nocaps".into()]).unwrap().line(), "setxkbmap -layout us -variant  -option  -option ctrl:nocaps");
        assert_eq!(setxkbmap_cmd("us,de", "intl", &[]).unwrap().args, ["-layout", "us,de", "-variant", "intl", "-option", ""]);
        assert!(setxkbmap_cmd("us; rm -rf", "", &[]).is_err());
        assert!(setxkbmap_cmd("", "", &[]).is_err());
        assert!(setxkbmap_cmd("us", "bad variant", &[]).is_err());
        assert!(setxkbmap_cmd("us", "", &["a b".into()]).is_err());
    }

    #[test]
    fn repeat_and_prop_commands() {
        assert_eq!(repeat_cmd(250, 40).unwrap().line(), "xset r rate 250 40");
        assert!(repeat_cmd(50, 40).is_err() && repeat_cmd(250, 0).is_err() && repeat_cmd(250, 101).is_err());
        assert_eq!(prop_cmd(11, P_TAP, "1").unwrap().line(), "xinput set-prop 11 libinput Tapping Enabled 1");
        assert_eq!(prop_cmd(10, P_ACCEL, "0.50").unwrap().args[3], "0.50");
        assert!(prop_cmd(10, P_ACCEL, "3").is_err());
        assert!(prop_cmd(10, P_ACCEL, "abc").is_err());
        assert!(prop_cmd(10, P_TAP, "2").is_err());
        assert!(prop_cmd(10, "Device Enabled", "0").is_err(), "only whitelisted properties");
    }

    #[test]
    fn session_xinput_upsert() {
        let mut s = Session::default();
        upsert_xinput(&mut s, "Pad", P_TAP, "1");
        upsert_xinput(&mut s, "Pad", P_TAP, "0");
        upsert_xinput(&mut s, "Pad", P_ACCEL, "0.5");
        assert_eq!(s.xinput.len(), 2);
        assert_eq!(s.xinput[0].value, "0");
    }

    #[test]
    fn load_full_and_degraded() {
        let r = FakeRunner::new()
            .ok("setxkbmap -query", XKB)
            .ok("localectl list-x11-keymap-layouts", "us\nde\nfr\n")
            .ok("xset q", XSET)
            .ok("xinput list", XLIST)
            .ok("xinput list-props 10", "Device 'Logitech':\n\tDevice Enabled (1):\t1\n")
            .ok("xinput list-props 11", PROPS);
        let s = load(&r);
        assert_eq!(s.layouts, ["us", "de", "fr"]);
        assert_eq!(s.repeat, Some((660, 25)));
        assert_eq!(s.pointers.len(), 1, "devices without libinput properties are hidden");
        assert_eq!(s.pointers[0].name, "SYNA Touchpad");
        let s = load(&FakeRunner::new());
        assert!(!s.has_xkb && !s.has_xset && !s.has_xinput);
        let mut h = Hub::with(std::sync::Arc::new(FakeRunner::lenient()), std::env::temp_dir().join("ms-inp-t"), std::sync::Arc::new(|| {}));
        h.input.data = Some(s);
        let txt = format!("{:?}", items(&h));
        assert!(txt.contains("setxkbmap is not installed") && txt.contains("xset is not installed") && txt.contains("xinput is not installed"));
    }

    #[test]
    fn apply_writes_session_and_runs_commands() {
        let d = std::env::temp_dir().join(format!("ms-inp-apply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let fr = std::sync::Arc::new(FakeRunner::lenient());
        let mut h = Hub::with(fr.clone(), d.clone(), std::sync::Arc::new(|| {}));
        h.input.data = Some(State {
            has_xkb: true,
            has_xset: true,
            has_xinput: true,
            layout: "us".into(),
            options: vec!["compose:ralt".into()],
            layouts: vec!["us".into(), "de".into()],
            repeat: Some((660, 25)),
            pointers: vec![parse_props(11, "SYNA Touchpad", PROPS)],
            ..State::default()
        });
        apply(&mut h, "caps", "", CtlVal::Str("ctrl:nocaps".into()));
        apply(&mut h, "delay", "", CtlVal::Num(250.0));
        apply(&mut h, "tap", "11", CtlVal::Bool(false));
        apply(&mut h, "layout", "", CtlVal::Str("xx".into()));
        assert!(h.msgs.iter().any(|m| m.1 && m.0.contains("Unknown layout")));
        for _ in 0..200 {
            if fr.lines().len() >= 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut l = fr.lines();
        l.sort();
        assert_eq!(l, ["setxkbmap -layout us -variant  -option  -option compose:ralt -option ctrl:nocaps", "xinput set-prop 11 libinput Tapping Enabled 0", "xset r rate 250 25"]);
        // last session write wins but every change is in the in-memory session
        assert_eq!(h.session.kbd_options, ["compose:ralt", "ctrl:nocaps"]);
        assert_eq!(h.session.repeat_delay, Some(250));
        assert_eq!(h.session.xinput[0].value, "0");
        let on_disk = Session::load(&d);
        assert!(on_disk.repeat_delay.is_some() || on_disk.xinput.len() == 1 || on_disk.kbd_layout.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}
