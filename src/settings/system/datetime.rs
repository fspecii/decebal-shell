//! Date & Time: time zone (timedatectl), NTP, and how the menu bar writes the clock (12/24 h, date style),
//! stored in `macshell.json` (`clockHour12`, `clockDate`) which macbar reads.

use super::run::{Cmd, Out, Runner};
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Opt, Row};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub available: bool,
    pub error: Option<String>,
    pub timezone: String,
    pub ntp: bool,
    pub can_ntp: bool,
    pub synced: bool,
    pub zones: Vec<String>,
}

/// `timedatectl show` (Key=Value lines).
pub fn parse_show(text: &str) -> State {
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).map(|v| v.trim().to_string()).unwrap_or_default();
    State { available: true, error: None, timezone: get("Timezone"), ntp: get("NTP") == "yes", can_ntp: get("CanNTP") == "yes", synced: get("NTPSynchronized") == "yes", zones: vec![] }
}

pub fn load(r: &dyn Runner) -> State {
    if !r.exists("timedatectl") {
        return State { error: Some("timedatectl (systemd) is not installed.".into()), ..State::default() };
    }
    let o = r.run(&Cmd::new("timedatectl", &["show"]).timeout(5));
    if !o.ok {
        return State { error: Some(format!("timedatectl failed: {}", o.error_text("timedatectl"))), ..State::default() };
    }
    let mut st = parse_show(&o.stdout);
    let z = r.run(&Cmd::new("timedatectl", &["list-timezones"]).timeout(5));
    if z.ok {
        st.zones = z.stdout.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    }
    st
}

pub fn valid_zone(st: &State, z: &str) -> Result<(), String> {
    let syntactic = !z.is_empty() && z.len() < 64 && !z.contains("..") && !z.starts_with('-') && z.chars().all(|c| c.is_ascii_alphanumeric() || "_+-/".contains(c));
    if !syntactic || (!st.zones.is_empty() && !st.zones.iter().any(|x| x == z)) {
        return Err(format!("\"{z}\" is not a known time zone"));
    }
    Ok(())
}

pub fn set_zone_cmd(z: &str) -> Cmd {
    Cmd::new("timedatectl", &["set-timezone", z]).timeout(15)
}

pub fn set_ntp_cmd(on: bool) -> Cmd {
    Cmd::new("timedatectl", &["set-ntp", if on { "true" } else { "false" }]).timeout(15)
}

/// Makes polkit failures readable; never asks for a password.
pub fn explain(out: &Out) -> Option<String> {
    let t = format!("{}{}", out.stderr, out.stdout).to_lowercase();
    if t.contains("authentication") || t.contains("not authorized") || t.contains("access denied") || t.contains("permission") {
        Some("This needs administrator rights. Run it in a terminal instead: timedatectl set-timezone <zone> (with sudo).".into())
    } else {
        None
    }
}

pub fn date_formats() -> Vec<Opt> {
    vec![("Mon 26 Oct".into(), "weekday-day-month".into()), ("26 Oct".into(), "day-month".into()), ("Mon 26".into(), "weekday-day".into()), ("Time only".into(), "none".into())]
}

pub fn items(h: &Hub, hour12: bool, datefmt: &str) -> Vec<Item> {
    let mut v = Vec::new();
    let now = crate::menubar::clock::now().map(|t| crate::menubar::clock::format_with(&t, hour12, datefmt)).unwrap_or_default();
    match &h.dt.data {
        None => v.push(Item::Note { text: "Reading time settings...".into(), warn: false }),
        Some(st) if !st.available => v.extend(super::sound::unavailable("Time zone and network time", st.error.as_deref().unwrap_or("timedatectl is not available."))),
        Some(st) => {
            v.push(Item::Header("Time zone".into()));
            let opts: Vec<Opt> = if st.zones.is_empty() { vec![(st.timezone.clone(), st.timezone.clone())] } else { st.zones.iter().map(|z| (z.clone(), z.clone())).collect() };
            let sel = opts.iter().position(|(_, v)| *v == st.timezone);
            v.push(Item::Card(vec![
                Row::new("Time zone").detail("Needs administrator approval (polkit); a refusal is shown here").ctl(Ctl::new("s:dt:zone:", Kind::Popup(opts, sel))),
                Row::new("Set time automatically")
                    .detail(if !st.can_ntp {
                        "No network time service is available".to_string()
                    } else if st.synced {
                        "Synchronised".to_string()
                    } else {
                        "Not synchronised yet".to_string()
                    })
                    .ctl({
                        let mut c = Ctl::new("s:dt:ntp:", Kind::Toggle(st.ntp));
                        c.enabled = st.can_ntp;
                        c
                    }),
            ]));
        }
    }
    v.push(Item::Header("Menu bar clock".into()));
    let opts = date_formats();
    let sel = opts.iter().position(|(_, v)| v == datefmt);
    v.push(Item::Card(vec![
        Row::new("Now showing").ctl(Ctl::new("lbl:clock", Kind::Label(now))),
        Row::new("Use 12-hour clock").detail("Applies to the menu bar within a minute").ctl(Ctl::new("s:dt:hour12:", Kind::Toggle(hour12))),
        Row::new("Date").ctl(Ctl::new("s:dt:datefmt:", Kind::Popup(opts, sel))),
    ]));
    v
}

pub fn apply(h: &mut Hub, act: &str, _arg: &str, val: CtlVal) {
    let Some(st) = h.dt.data.clone() else { return };
    let r: Result<(), String> = (|| {
        match (act, &val) {
            ("zone", CtlVal::Str(z)) => {
                valid_zone(&st, z)?;
                let mut a = Action::new(Dom::DateTime, "Time zone", vec![set_zone_cmd(z)]).undo(vec![set_zone_cmd(&st.timezone)]);
                a.ok_msg = Some(format!("Time zone is now {z}"));
                a.explain = true;
                h.run(a);
            }
            ("ntp", CtlVal::Bool(on)) => {
                let mut a = Action::new(Dom::DateTime, "Automatic time", vec![set_ntp_cmd(*on)]).undo(vec![set_ntp_cmd(!*on)]);
                a.explain = true;
                h.run(a);
            }
            _ => return Err("Unsupported date and time change".into()),
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

    const SHOW: &str = "Timezone=Europe/Bucharest\nLocalRTC=no\nCanNTP=yes\nNTP=yes\nNTPSynchronized=yes\nTimeUSec=Tue 2026-10-27 03:43:59 EET\n";

    #[test]
    fn parses_show() {
        let s = parse_show(SHOW);
        assert_eq!(s.timezone, "Europe/Bucharest");
        assert!(s.ntp && s.can_ntp && s.synced);
        let s = parse_show("Timezone=UTC\nCanNTP=no\nNTP=no\n");
        assert!(!s.ntp && !s.can_ntp && !s.synced);
    }

    #[test]
    fn load_and_degrade() {
        let r = FakeRunner::new().ok("timedatectl show", SHOW).ok("timedatectl list-timezones", "Africa/Accra\nEurope/Bucharest\nUTC\n");
        let s = load(&r);
        assert_eq!(s.zones.len(), 3);
        assert!(load(&FakeRunner::new()).error.unwrap().contains("not installed"));
    }

    #[test]
    fn zone_validation_and_commands() {
        let s = State { zones: vec!["UTC".into(), "Europe/Bucharest".into()], ..parse_show(SHOW) };
        assert!(valid_zone(&s, "UTC").is_ok());
        assert!(valid_zone(&s, "Mars/Olympus").is_err());
        assert!(valid_zone(&s, "../etc/passwd").is_err());
        assert!(valid_zone(&s, "-h").is_err());
        assert!(valid_zone(&s, "").is_err());
        let none = State::default();
        assert!(valid_zone(&none, "Europe/Paris").is_ok(), "without the list only the syntax is checked");
        assert_eq!(set_zone_cmd("UTC").line(), "timedatectl set-timezone UTC");
        assert_eq!(set_ntp_cmd(false).line(), "timedatectl set-ntp false");
    }

    #[test]
    fn polkit_failure_is_explained() {
        let o = Out::fail("Failed to set time zone: Interactive authentication required.");
        assert!(explain(&o).unwrap().contains("administrator"));
        assert!(explain(&Out::fail("Invalid time zone 'x'")).is_none());
    }

    #[test]
    fn items_clock_labels() {
        let mut h = Hub::with(std::sync::Arc::new(FakeRunner::lenient()), std::env::temp_dir().join("ms-dt-t"), std::sync::Arc::new(|| {}));
        h.dt.data = Some(State { zones: vec!["UTC".into()], ..parse_show(SHOW) });
        let t = format!("{:?}", items(&h, true, "weekday-day-month"));
        assert!(t.contains("s:dt:zone:") && t.contains("s:dt:hour12:") && t.contains("AM") || t.contains("PM"));
        h.dt.data = Some(State { error: Some("x".into()), ..State::default() });
        assert!(format!("{:?}", items(&h, false, "none")).contains("Not available"));
    }
}
