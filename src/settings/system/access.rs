//! Privacy & Security (screen lock preferences) and Accessibility (cursor size, reduce motion, text size, contrast).
//! Both only change the session settings (`session-settings.json/.sh`); the lock preference is exported as
//! `DECEBAL_LOCK_CMD`, `DECEBAL_LOCK_SECS`, `DECEBAL_LOCK_ON_SUSPEND` for whatever runs the idle lock.

use super::run::Cmd;
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Opt, Row};

pub const LOCKERS: &[(&str, &str)] = &[
    ("Hyprlock", "hyprlock"),
    ("Swaylock", "swaylock"),
    ("i3lock", "i3lock"),
    ("XSecureLock", "xsecurelock"),
    ("slock", "slock"),
    ("betterlockscreen", "betterlockscreen -l"),
    ("Lock the session (loginctl)", "loginctl lock-session"),
];

pub fn installed_lockers(have: &dyn Fn(&str) -> bool) -> Vec<Opt> {
    LOCKERS.iter().filter(|(_, c)| have(c.split(' ').next().unwrap_or(c))).map(|(l, c)| (l.to_string(), c.to_string())).collect()
}

pub fn valid_lock_cmd(c: &str, have: &dyn Fn(&str) -> bool) -> Result<(), String> {
    let t = c.trim();
    if t.is_empty() || t.len() > 200 || t.chars().any(|ch| ch.is_control() || ";&|`$<>\n".contains(ch)) {
        return Err("The lock command must be a single program with arguments (no shell syntax)".into());
    }
    let prog = t.split(' ').next().unwrap_or("");
    if !have(prog) {
        return Err(format!("{prog} is not installed"));
    }
    Ok(())
}

pub fn lock_timeouts() -> Vec<Opt> {
    [(0, "Never"), (60, "1 minute"), (120, "2 minutes"), (300, "5 minutes"), (600, "10 minutes"), (900, "15 minutes"), (1800, "30 minutes")]
        .iter()
        .map(|(s, l)| (l.to_string(), s.to_string()))
        .collect()
}

fn have(p: &str) -> bool {
    super::run::which(p).is_some()
}

#[derive(Default)]
pub struct Ui {
    pub custom: String,
}

pub fn privacy_items(h: &Hub) -> Vec<Item> {
    let s = &h.session;
    let mut lockers = installed_lockers(&have);
    if let Some(c) = &s.lock_cmd {
        if !lockers.iter().any(|(_, v)| v == c) {
            lockers.push((c.clone(), c.clone()));
        }
    }
    let sel = s.lock_cmd.as_ref().and_then(|c| lockers.iter().position(|(_, v)| v == c));
    let mut v = vec![Item::Header("Screen lock".into())];
    let mut rows = Vec::new();
    if lockers.is_empty() {
        rows.push(Row::new("Lock screen command").detail("No screen locker found (hyprlock, swaylock, i3lock, ...)"));
    } else {
        rows.push(Row::new("Lock screen command").detail("Program that locks the screen").ctl(Ctl::new("s:prv:cmd:", Kind::Popup(lockers, sel))));
    }
    rows.push(Row::new("Other command").detail("Any program that locks the screen").ctl(Ctl::new("s:prv:custom:", Kind::Text { val: h.ui_access.custom.clone(), hint: "my-locker --flag".into() })));
    let o = lock_timeouts();
    let tsel = o.iter().position(|(_, v)| v.parse::<u32>().ok() == Some(s.lock_secs.unwrap_or(600)));
    rows.push(Row::new("Lock after").detail("Idle time before the screen locks").ctl(Ctl::new("s:prv:secs:", Kind::Popup(o, tsel))));
    rows.push(Row::new("Lock when suspending").ctl(Ctl::new("s:prv:suspend:", Kind::Toggle(s.lock_on_suspend))));
    v.push(Item::Card(rows));
    v.push(Item::Note { text: "decsettings saves these preferences and exports them at login (DECEBAL_LOCK_CMD, DECEBAL_LOCK_SECS, DECEBAL_LOCK_ON_SUSPEND). The program that watches for idle time and suspend (hypridle, xss-lock, ...) is part of your session setup.".into(), warn: false });
    v.push(Item::Header("Good to know".into()));
    let session = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "Wayland"
    } else if std::env::var_os("DISPLAY").is_some() {
        "X11"
    } else {
        "no graphical session detected"
    };
    v.push(Item::Card(vec![
        Row::new("Display server").ctl(Ctl::new("lbl:ds", Kind::Label(session.into()))),
        Row::new("Disk encryption and firewall").detail("Managed by the system (cryptsetup, nftables/ufw); they need administrator rights and are not changed here"),
    ]));
    v
}

pub fn privacy_apply(h: &mut Hub, act: &str, val: CtlVal) {
    let mut s = h.session.clone();
    let r: Result<String, String> = (|| match (act, &val) {
        ("cmd", CtlVal::Str(c)) => {
            valid_lock_cmd(c, &have)?;
            s.lock_cmd = Some(c.clone());
            Ok(format!("Lock command: {c}"))
        }
        ("custom", CtlVal::Str(c)) => {
            h.ui_access.custom = c.clone();
            if c.trim().is_empty() {
                h.dirty = true;
                return Err(String::new());
            }
            valid_lock_cmd(c, &have)?;
            s.lock_cmd = Some(c.trim().to_string());
            h.ui_access.custom.clear();
            Ok(format!("Lock command: {}", c.trim()))
        }
        ("secs", CtlVal::Str(n)) => {
            let n: u32 = n.parse().map_err(|_| "Invalid time")?;
            if n > 86400 {
                return Err("Time is too long".into());
            }
            s.lock_secs = Some(n);
            Ok("Lock timeout saved".into())
        }
        ("suspend", CtlVal::Bool(b)) => {
            s.lock_on_suspend = *b;
            Ok("Saved".into())
        }
        _ => Err("Unsupported privacy change".into()),
    })();
    match r {
        Ok(m) => h.run_session(Action::new(Dom::Privacy, "Screen lock", vec![]).msg(m), s),
        Err(e) if e.is_empty() => {}
        Err(e) => h.say(e, true),
    }
}

// ---------------------------------------------------------------- accessibility

pub fn cursor_sizes() -> Vec<Opt> {
    [16, 24, 32, 48, 64, 96].iter().map(|n| (format!("{n} px"), n.to_string())).collect()
}

pub fn cursor_cmd(size: u32) -> Result<Cmd, String> {
    if !(8..=256).contains(&size) {
        return Err("Cursor size must be between 8 and 256".into());
    }
    Ok(Cmd::owned("sh", vec!["-c".into(), "printf 'Xcursor.size: %s\\n' \"$1\" | xrdb -merge".into(), "sh".into(), size.to_string()]))
}

pub fn access_items(h: &Hub) -> Vec<Item> {
    let s = &h.session;
    let mut reset_font = Ctl::new("s:acc:font-theme:", Kind::Button { label: "Use theme".into(), danger: false });
    reset_font.enabled = s.font_scale.is_some();
    let o = cursor_sizes();
    let sel = s.cursor_size.and_then(|c| o.iter().position(|(_, v)| v.parse::<u32>().ok() == Some(c)));
    vec![
        Item::Header("Pointer".into()),
        Item::Card(vec![Row::new("Cursor size").detail("Larger pointer; apps pick it up when they start").ctl(Ctl::new("s:acc:cursor:", Kind::Popup(o, sel)))]),
        Item::Header("Motion and text".into()),
        Item::Card(vec![
            Row::new("Reduce motion").detail("Disables dock magnification and launch bounce").ctl(Ctl::new("s:acc:motion:", Kind::Toggle(s.reduce_motion))),
            Row::new("Text size")
                .detail("Overrides Decebal theme text size (80–200%)")
                .ctl(Ctl::new("s:acc:font:", Kind::Slider { v: (s.font_scale.unwrap_or(1.0) * 100.0).round() as f64, min: 80.0, max: 200.0, step: 5.0, unit: " %".into() }))
                .ctl(reset_font),
            Row::new("High contrast").detail("Uses opaque, high-contrast Decebal surfaces and text").ctl(Ctl::new("s:acc:contrast:", Kind::Toggle(s.high_contrast))),
        ]),
        Item::Note {
            text: "These are saved in ~/.config/decebal/session-settings.sh and take effect for programs started after your next login. Text size and contrast affect Decebal theme consumers; reduce motion affects the dock."
                .into(),
            warn: false,
        },
    ]
}

pub fn access_apply(h: &mut Hub, act: &str, val: CtlVal) {
    if act == "font-theme" && val == CtlVal::Click && h.session.font_scale.is_none() {
        return;
    }
    let mut s = h.session.clone();
    let mut cmds = vec![];
    let r: Result<(), String> = (|| {
        match (act, &val) {
            ("cursor", CtlVal::Str(n)) => {
                let n: u32 = n.parse().map_err(|_| "Invalid size")?;
                cmds.push(cursor_cmd(n)?);
                s.cursor_size = Some(n);
            }
            ("motion", CtlVal::Bool(b)) => s.reduce_motion = *b,
            ("contrast", CtlVal::Bool(b)) => s.high_contrast = *b,
            ("font-theme", CtlVal::Click) => {
                s.font_scale = None;
            }
            ("font", CtlVal::Num(n)) => {
                if !(80.0..=200.0).contains(n) {
                    return Err("Text size must be between 80 % and 200 %".into());
                }
                s.font_scale = Some((*n / 100.0) as f32);
            }
            _ => return Err("Unsupported accessibility change".into()),
        }
        Ok(())
    })();
    match r {
        Ok(()) => h.run_session(Action::new(Dom::Access, "Accessibility", cmds).msg("Saved"), s),
        Err(e) => h.say(e, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::FakeRunner;
    use std::sync::Arc;

    #[test]
    fn lock_command_validation() {
        let have = |p: &str| p == "hyprlock" || p == "my-lock";
        assert!(valid_lock_cmd("hyprlock", &have).is_ok());
        assert!(valid_lock_cmd("my-lock --fast", &have).is_ok());
        assert!(valid_lock_cmd("hyprlock; rm -rf ~", &have).is_err());
        assert!(valid_lock_cmd("$(evil)", &have).is_err());
        assert!(valid_lock_cmd("nothere", &have).is_err());
        assert!(valid_lock_cmd("  ", &have).is_err());
        assert_eq!(installed_lockers(&|p| p == "swaylock" || p == "loginctl").len(), 2);
    }

    #[test]
    fn cursor_command() {
        let c = cursor_cmd(32).unwrap();
        assert_eq!(c.args.last().unwrap(), "32");
        assert!(cursor_cmd(2).is_err() && cursor_cmd(9999).is_err());
    }

    #[test]
    fn text_size_control_can_return_to_theme_without_losing_pending_preferences() {
        use super::super::session::Session;
        use crate::settings::model::tests_support::model_with;
        use std::time::{Duration, Instant};

        fn control(h: &Hub, id: &str) -> Ctl {
            access_items(h)
                .into_iter()
                .filter_map(|i| match i {
                    Item::Card(rows) => Some(rows),
                    _ => None,
                })
                .flatten()
                .flat_map(|r| r.ctls)
                .find(|c| c.id == id)
                .unwrap()
        }
        fn saved(h: &mut Hub, d: &std::path::Path, expected: &Session) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let (_, msgs) = h.drain();
                assert!(msgs.iter().all(|(_, error)| !error), "{msgs:?}");
                if Session::load(d) == *expected && Session::sh_path(d).exists() {
                    // The JSON rename precedes the shell rename; wait for both outputs.
                    let sh = std::fs::read_to_string(Session::sh_path(d)).unwrap();
                    if sh == expected.to_sh() {
                        break;
                    }
                }
                assert!(Instant::now() < deadline, "session save timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        let (mut model, cfg) = model_with("");
        let d = cfg.parent().unwrap().join("session-test");
        let fr = Arc::new(FakeRunner::lenient());
        model.sys = Hub::with(fr.clone(), d.clone(), Arc::new(|| {}));
        let reset = control(&model.sys, "s:acc:font-theme:");
        assert_eq!(reset.kind, Kind::Button { label: "Use theme".into(), danger: false });
        assert!(!reset.enabled);
        model.sys_apply(&reset.id, CtlVal::Click);
        assert!(!Session::json_path(&d).exists());

        // Queue other preferences without waiting for their async saves.
        model.sys_apply("s:prv:secs:", CtlVal::Str("300".into()));
        model.sys_apply("s:prv:suspend:", CtlVal::Bool(true));
        model.sys_apply("s:acc:cursor:", CtlVal::Str("48".into()));
        model.sys_apply("s:acc:motion:", CtlVal::Bool(true));
        model.sys_apply("s:acc:contrast:", CtlVal::Bool(true));
        let font = control(&model.sys, "s:acc:font:");
        model.sys_apply(&font.id, CtlVal::Num(125.0));
        let expected_override = model.sys.session.clone();
        assert_eq!(expected_override.font_scale, Some(1.25));
        saved(&mut model.sys, &d, &expected_override);
        assert!(std::fs::read_to_string(Session::sh_path(&d)).unwrap().contains("export DECEBAL_FONT_SCALE=1.25\n"));
        let commands_before_reset = fr.lines();
        let reset = control(&model.sys, "s:acc:font-theme:");
        assert!(reset.enabled);
        model.sys_apply(&reset.id, CtlVal::Click);
        let mut expected_theme = expected_override;
        expected_theme.font_scale = None;
        assert_eq!(model.sys.session, expected_theme);
        saved(&mut model.sys, &d, &expected_theme);
        assert!(std::fs::read_to_string(Session::sh_path(&d)).unwrap().contains("export DECEBAL_FONT_SCALE=''\n"));
        assert_eq!(fr.lines(), commands_before_reset);
        assert!(!control(&model.sys, &reset.id).enabled);
        model.sys_apply(&reset.id, CtlVal::Click);
        assert_eq!(model.sys.session, expected_theme);
        // All command execution above goes through FakeRunner, including cursor xrdb.
        assert_eq!(fr.lines().len(), 1);
        let _ = std::fs::remove_dir_all(cfg.parent().unwrap());
    }

    #[test]
    fn privacy_and_access_persist_to_session_files() {
        let d = std::env::temp_dir().join(format!("ms-acc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let fr = Arc::new(FakeRunner::lenient());
        let mut h = Hub::with(fr.clone(), d.clone(), Arc::new(|| {}));
        privacy_apply(&mut h, "secs", CtlVal::Str("300".into()));
        privacy_apply(&mut h, "suspend", CtlVal::Bool(true));
        privacy_apply(&mut h, "cmd", CtlVal::Str("nothere-xyz".into()));
        assert!(h.msgs.last().unwrap().1);
        access_apply(&mut h, "cursor", CtlVal::Str("48".into()));
        access_apply(&mut h, "motion", CtlVal::Bool(true));
        access_apply(&mut h, "font", CtlVal::Num(125.0));
        access_apply(&mut h, "font", CtlVal::Num(500.0));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let sh = std::fs::read_to_string(super::super::session::Session::sh_path(&d)).unwrap();
        for want in ["DECEBAL_LOCK_SECS=300", "DECEBAL_LOCK_ON_SUSPEND=1", "XCURSOR_SIZE=48", "DECEBAL_REDUCE_MOTION=1", "DECEBAL_FONT_SCALE=1.25"] {
            assert!(sh.contains(want), "{want}\n{sh}");
        }
        assert!(fr.lines().iter().any(|l| l.contains("xrdb")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn race_returning_to_accessibility_while_a_session_save_is_held_keeps_the_newer_state() {
        use crate::settings::persist::Persister;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        let dir = std::env::temp_dir().join(format!("decebal-race-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = crate::settings::system::Hub::with(Arc::new(crate::settings::system::run::FakeRunner::default()), dir.clone(), Arc::new(|| {}));
        let (gate, calls) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let (g, c) = (gate.clone(), calls.clone());
        let p = Persister::spawn_with_sync_hook(
            Arc::new(Mutex::new(Default::default())),
            Arc::new(|| {}),
            Arc::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                while g.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Ok(())
            }),
        );
        *h.sh.persist.lock().unwrap() = p.handle();
        access_apply(&mut h, "motion", crate::settings::model::CtlVal::Bool(true)); // its save is held
        let t0 = std::time::Instant::now();
        while calls.load(Ordering::SeqCst) == 0 && t0.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        h.activate("sound");
        h.activate("accessibility");
        access_apply(&mut h, "contrast", crate::settings::model::CtlVal::Bool(true));
        std::thread::sleep(std::time::Duration::from_millis(100));
        gate.store(false, Ordering::SeqCst);
        let t0 = std::time::Instant::now();
        let saved = loop {
            let _ = h.drain();
            let s = crate::settings::system::session::Session::load(&dir);
            if (s.high_contrast && p.backlog() == 0) || t0.elapsed() > std::time::Duration::from_secs(10) {
                break s;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(saved.reduce_motion && saved.high_contrast, "reduce motion must not be clobbered: {saved:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
