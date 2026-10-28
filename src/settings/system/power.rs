//! Power & Battery: power profile (powerprofilesctl), screen blank / display sleep (xset, persisted in the session
//! file), battery (sysfs, hidden on desktops), lid/suspend behaviour (read-only, from logind configuration).

use super::input::parse_xset_screen;
use super::run::{Cmd, Runner};
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Opt, Row};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub name: String,
    pub percent: u32,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub profiles: Vec<String>,
    pub profile: Option<String>,
    pub has_xset: bool,
    pub blank_secs: Option<u32>,
    pub dpms: Option<bool>,
    pub dpms_off_secs: Option<u32>,
    pub batteries: Vec<Battery>,
    pub on_ac: Option<bool>,
    pub lid: Option<String>,
    pub lid_ac: Option<String>,
    pub idle_action: Option<String>,
    pub ppd_missing: bool,
}

/// `powerprofilesctl list`: profile names (lines like `* performance:` / `  balanced:`).
pub fn parse_profiles(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let t = l.trim_start_matches(['*', ' ']);
            (!l.starts_with("    ") && !l.starts_with('\t')).then_some(())?;
            let name = t.strip_suffix(':')?;
            (!name.is_empty() && !name.contains(' ')).then(|| name.to_string())
        })
        .collect()
}

/// Reads batteries below a `power_supply` directory.
pub fn read_batteries(root: &Path) -> (Vec<Battery>, Option<bool>) {
    let mut bats = Vec::new();
    let mut ac = None;
    let Ok(rd) = std::fs::read_dir(root) else { return (bats, ac) };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let rd = |f: &str| std::fs::read_to_string(p.join(f)).ok().map(|s| s.trim().to_string());
        match rd("type").as_deref() {
            Some("Battery") if rd("scope").as_deref() != Some("Device") => {
                if let Some(c) = rd("capacity").and_then(|c| c.parse().ok()) {
                    bats.push(Battery { name: e.file_name().to_string_lossy().into_owned(), percent: c, status: rd("status").unwrap_or_default() });
                }
            }
            Some("Mains") => ac = ac.or(rd("online").map(|o| o == "1")),
            _ => {}
        }
    }
    (bats, ac)
}

/// Last `Key=value` of a logind configuration (main file then drop-ins in order).
pub fn logind_value(texts: &[String], key: &str) -> Option<String> {
    let mut v = None;
    for t in texts {
        for l in t.lines() {
            let l = l.trim();
            if let Some(r) = l.strip_prefix(key).and_then(|r| r.trim_start().strip_prefix('=')) {
                v = Some(r.trim().to_string());
            }
        }
    }
    v
}

fn logind_texts(etc: &Path) -> Vec<String> {
    let mut v = vec![std::fs::read_to_string(etc.join("logind.conf")).unwrap_or_default()];
    if let Ok(rd) = std::fs::read_dir(etc.join("logind.conf.d")) {
        let mut f: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "conf")).collect();
        f.sort();
        v.extend(f.iter().filter_map(|p| std::fs::read_to_string(p).ok()));
    }
    v
}

pub fn load(r: &dyn Runner) -> State {
    load_from(r, Path::new("/sys/class/power_supply"), Path::new("/etc/systemd"))
}

pub fn load_from(r: &dyn Runner, sys: &Path, etc: &Path) -> State {
    let mut st = State { has_xset: r.exists("xset"), ..State::default() };
    if r.exists("powerprofilesctl") {
        let l = r.run(&Cmd::new("powerprofilesctl", &["list"]).timeout(5));
        if l.ok {
            st.profiles = parse_profiles(&l.stdout);
            let g = r.run(&Cmd::new("powerprofilesctl", &["get"]).timeout(5));
            if g.ok {
                st.profile = Some(g.stdout.trim().to_string());
            }
        } else {
            st.ppd_missing = true;
        }
    } else {
        st.ppd_missing = true;
    }
    if st.has_xset {
        let q = r.run(&Cmd::new("xset", &["q"]).timeout(5));
        if q.ok {
            (st.blank_secs, st.dpms, st.dpms_off_secs) = parse_xset_screen(&q.stdout);
        }
    }
    (st.batteries, st.on_ac) = read_batteries(sys);
    let t = logind_texts(etc);
    st.lid = logind_value(&t, "HandleLidSwitch");
    st.lid_ac = logind_value(&t, "HandleLidSwitchExternalPower");
    st.idle_action = logind_value(&t, "IdleAction");
    st
}

pub fn profile_cmd(st: &State, p: &str) -> Result<Vec<Cmd>, String> {
    if !st.profiles.iter().any(|x| x == p) {
        return Err(format!("\"{p}\" is not an available power profile"));
    }
    Ok(vec![Cmd::new("powerprofilesctl", &["set", p])])
}

/// Timeout choices in seconds (0 = never).
pub fn timeouts() -> Vec<Opt> {
    [(0, "Never"), (60, "1 minute"), (120, "2 minutes"), (300, "5 minutes"), (600, "10 minutes"), (900, "15 minutes"), (1800, "30 minutes"), (3600, "1 hour")]
        .iter()
        .map(|(s, l)| (l.to_string(), s.to_string()))
        .collect()
}

pub fn blank_cmds(secs: u32) -> Result<Vec<Cmd>, String> {
    if secs > 24 * 3600 {
        return Err("Timeout is too long".into());
    }
    let s = secs.to_string();
    Ok(if secs == 0 { vec![Cmd::new("xset", &["s", "off"])] } else { vec![Cmd::new("xset", &["s", "on"]), Cmd::new("xset", &["s", &s, &s])] })
}

pub fn dpms_cmds(secs: u32) -> Result<Vec<Cmd>, String> {
    if secs > 24 * 3600 {
        return Err("Timeout is too long".into());
    }
    let s = secs.to_string();
    Ok(if secs == 0 { vec![Cmd::new("xset", &["-dpms"])] } else { vec![Cmd::new("xset", &["+dpms"]), Cmd::new("xset", &["dpms", &s, &s, &s])] })
}

fn secs_sel(opts: &[Opt], s: Option<u32>) -> Option<usize> {
    let s = s?;
    opts.iter().position(|(_, v)| v.parse::<u32>().ok() == Some(s))
}

pub fn items(h: &Hub) -> Vec<Item> {
    let Some(st) = &h.power.data else { return vec![Item::Note { text: "Reading power state...".into(), warn: false }] };
    let mut v = Vec::new();
    if !st.batteries.is_empty() {
        v.push(Item::Header("Battery".into()));
        let mut rows: Vec<Row> =
            st.batteries.iter().map(|b| Row::new(b.name.clone()).detail(b.status.clone()).ctl(Ctl::new(format!("lbl:bat:{}", b.name), Kind::Label(format!("{} %", b.percent))))).collect();
        if let Some(ac) = st.on_ac {
            rows.push(Row::new("Power source").ctl(Ctl::new("lbl:ac", Kind::Label(if ac { "Power adapter".into() } else { "Battery".into() }))));
        }
        v.push(Item::Card(rows));
    }
    v.push(Item::Header("Power mode".into()));
    if st.profiles.is_empty() {
        v.push(Item::Card(vec![Row::new("Power profile").detail("Not available: power-profiles-daemon (powerprofilesctl) is not running or not installed")]));
    } else {
        let label = |p: &str| match p {
            "power-saver" => "Power saver".to_string(),
            "balanced" => "Balanced".to_string(),
            "performance" => "Performance".to_string(),
            o => o.to_string(),
        };
        let opts: Vec<(String, String)> = st.profiles.iter().map(|p| (label(p), p.clone())).collect();
        let sel = opts.iter().position(|(_, v)| Some(v) == st.profile.as_ref());
        v.push(Item::Card(vec![Row::new("Power profile").detail("Trades speed against battery life and fan noise").ctl(Ctl::new("s:pwr:profile:", Kind::Seg(opts, sel)))]));
    }
    v.push(Item::Header("Screen".into()));
    if !st.has_xset {
        v.push(Item::Card(vec![Row::new("Screen blanking").detail("Not available: xset is not installed (or this is not an X11 session)")]));
    } else {
        let o = timeouts();
        let blank = secs_sel(&o, st.blank_secs.or(h.session.blank_secs));
        let off = if st.dpms == Some(false) { Some(0) } else { st.dpms_off_secs.or(h.session.dpms_secs) };
        v.push(Item::Card(vec![
            Row::new("Screen saver").detail("Blank the picture after no input").ctl(Ctl::new("s:pwr:blank:", Kind::Popup(o.clone(), blank))),
            Row::new("Turn display off").detail("Put the monitor to sleep (DPMS)").ctl(Ctl::new("s:pwr:dpms:", Kind::Popup(o.clone(), secs_sel(&o, off)))),
        ]));
    }
    v.push(Item::Header("Lid and suspend".into()));
    let show = |x: &Option<String>| x.clone().unwrap_or_else(|| "system default".into());
    v.push(Item::Card(vec![
        Row::new("Close the lid").ctl(Ctl::new("lbl:lid", Kind::Label(show(&st.lid)))),
        Row::new("Close the lid on external power").ctl(Ctl::new("lbl:lidac", Kind::Label(show(&st.lid_ac)))),
        Row::new("When idle").ctl(Ctl::new("lbl:idle", Kind::Label(show(&st.idle_action)))),
    ]));
    v.push(Item::Note {
        text: "These are set by systemd-logind (/etc/systemd/logind.conf and logind.conf.d). decsettings only reads them: changing them needs administrator rights.".into(),
        warn: false,
    });
    v
}

pub fn apply(h: &mut Hub, act: &str, _arg: &str, val: CtlVal) {
    let Some(st) = h.power.data.clone() else { return };
    let r: Result<(), String> = (|| {
        match (act, &val) {
            ("profile", CtlVal::Str(p)) => {
                let cmds = profile_cmd(&st, p)?;
                let undo = st.profile.as_ref().map(|o| profile_cmd(&st, o)).transpose()?.unwrap_or_default();
                if let Some(s) = h.power.data.as_mut() {
                    s.profile = Some(p.clone());
                }
                h.run(Action::new(Dom::Power, format!("Power profile {p}"), cmds).undo(undo));
            }
            ("blank", CtlVal::Str(s)) => {
                let secs: u32 = s.parse().map_err(|_| "Invalid timeout")?;
                let cmds = blank_cmds(secs)?;
                let undo = blank_cmds(st.blank_secs.unwrap_or(600))?;
                let mut ses = h.session.clone();
                ses.blank_secs = Some(secs);
                h.run_session(Action::new(Dom::Power, "Screen saver", cmds).undo(undo), ses);
            }
            ("dpms", CtlVal::Str(s)) => {
                let secs: u32 = s.parse().map_err(|_| "Invalid timeout")?;
                let cmds = dpms_cmds(secs)?;
                let undo = dpms_cmds(if st.dpms == Some(false) { 0 } else { st.dpms_off_secs.unwrap_or(0) })?;
                let mut ses = h.session.clone();
                ses.dpms_secs = Some(secs);
                h.run_session(Action::new(Dom::Power, "Display sleep", cmds).undo(undo), ses);
            }
            _ => return Err("Unsupported power change".into()),
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

    const LIST: &str =
        "* performance:\n    CpuDriver:\tamd_pstate\n    Degraded:   no\n\n  balanced:\n    CpuDriver:\tamd_pstate\n    PlatformDriver:\tplaceholder\n\n  power-saver:\n    CpuDriver:\tamd_pstate\n";

    fn tmp(n: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ms-pwr-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_profiles() {
        assert_eq!(parse_profiles(LIST), ["performance", "balanced", "power-saver"]);
        assert!(parse_profiles("").is_empty());
    }

    #[test]
    fn reads_batteries_and_ac() {
        let d = tmp("bat");
        for (n, t, extra) in [
            ("BAT0", "Battery", &[("capacity", "73"), ("status", "Discharging")][..]),
            ("AC", "Mains", &[("online", "1")][..]),
            ("hidpp_battery_0", "Battery", &[("capacity", "50"), ("scope", "Device")][..]),
        ] {
            std::fs::create_dir_all(d.join(n)).unwrap();
            std::fs::write(d.join(n).join("type"), t).unwrap();
            for (k, v) in extra {
                std::fs::write(d.join(n).join(k), v).unwrap();
            }
        }
        let (b, ac) = read_batteries(&d);
        assert_eq!(b, [Battery { name: "BAT0".into(), percent: 73, status: "Discharging".into() }]);
        assert_eq!(ac, Some(true));
        // desktop: nothing there
        let e = tmp("nobat");
        assert!(read_batteries(&e).0.is_empty());
        assert!(read_batteries(&e.join("missing")).0.is_empty());
    }

    #[test]
    fn logind_last_value_wins() {
        let t = vec!["[Login]\n#HandleLidSwitch=suspend\nHandleLidSwitch=ignore\n".to_string(), "[Login]\nHandleLidSwitch = lock\nIdleAction=suspend\n".to_string()];
        assert_eq!(logind_value(&t, "HandleLidSwitch").as_deref(), Some("lock"));
        assert_eq!(logind_value(&t, "IdleAction").as_deref(), Some("suspend"));
        assert_eq!(logind_value(&t, "HandleLidSwitchDocked"), None);
    }

    #[test]
    fn load_reads_everything_and_hides_missing() {
        let sys = tmp("sys");
        let etc = tmp("etc");
        let r = FakeRunner::new()
            .ok("powerprofilesctl list", LIST)
            .ok("powerprofilesctl get", "balanced\n")
            .ok("xset q", "  timeout:  300    cycle:  300\nDPMS (Energy Star):\n  Standby: 0    Suspend: 0    Off: 600\n  DPMS is Enabled\n");
        let s = load_from(&r, &sys, &etc);
        assert_eq!(s.profile.as_deref(), Some("balanced"));
        assert_eq!(s.blank_secs, Some(300));
        assert_eq!(s.dpms_off_secs, Some(600));
        assert!(s.batteries.is_empty() && s.lid.is_none());
        let s = load_from(&FakeRunner::new(), &sys, &etc);
        assert!(s.ppd_missing && !s.has_xset);
    }

    #[test]
    fn commands() {
        let st = State { profiles: vec!["balanced".into(), "performance".into()], ..State::default() };
        assert_eq!(profile_cmd(&st, "performance").unwrap()[0].line(), "powerprofilesctl set performance");
        assert!(profile_cmd(&st, "turbo").is_err());
        assert!(profile_cmd(&st, "--help").is_err());
        let l = |v: Vec<Cmd>| v.iter().map(|c| c.line()).collect::<Vec<_>>();
        assert_eq!(l(blank_cmds(0).unwrap()), ["xset s off"]);
        assert_eq!(l(blank_cmds(600).unwrap()), ["xset s on", "xset s 600 600"]);
        assert_eq!(l(dpms_cmds(0).unwrap()), ["xset -dpms"]);
        assert_eq!(l(dpms_cmds(900).unwrap()), ["xset +dpms", "xset dpms 900 900 900"]);
        assert!(blank_cmds(10_000_000).is_err());
    }

    #[test]
    fn items_hide_battery_on_desktops_and_show_unavailable() {
        let mut h = Hub::with(std::sync::Arc::new(FakeRunner::lenient()), std::env::temp_dir().join("ms-pwr-t"), std::sync::Arc::new(|| {}));
        h.power.data = Some(State { ppd_missing: true, ..State::default() });
        let t = format!("{:?}", items(&h));
        assert!(!t.contains("Battery\""));
        assert!(t.contains("power-profiles-daemon") && t.contains("xset is not installed"));
        h.power.data = Some(State {
            profiles: vec!["balanced".into()],
            profile: Some("balanced".into()),
            has_xset: true,
            batteries: vec![Battery { name: "BAT0".into(), percent: 80, status: "Charging".into() }],
            ..State::default()
        });
        let t = format!("{:?}", items(&h));
        assert!(t.contains("BAT0") && t.contains("80 %") && t.contains("s:pwr:profile:") && t.contains("s:pwr:blank:"));
    }
}
