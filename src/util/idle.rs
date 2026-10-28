//! Idle inhibitor for X11: switches the X screensaver and DPMS off (so `xss-lock`, which waits for the screensaver
//! timeout, never locks and the monitors never blank) and puts the previous values back on `off`.
//!
//! The values read from `xset q` are kept in `$XDG_RUNTIME_DIR/decutil/idle.json`; that file existing is the state.

use super::{toast, Ctx, Sys};
use serde_json::{json, Value};
use std::time::Duration;

const STATE: &str = "idle.json";
const T: Duration = Duration::from_secs(3);

/// What `xset q` reports about the screensaver and DPMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Xset {
    /// Screensaver timeout in seconds (0 = off) and its cycle.
    pub timeout: u32,
    pub cycle: u32,
    pub dpms: bool,
    pub standby: u32,
    pub suspend: u32,
    pub off: u32,
}

fn number_after(line: &str, key: &str) -> Option<u32> {
    let rest = &line[line.find(key)? + key.len()..];
    rest.split_whitespace().next()?.parse().ok()
}

pub fn parse_xset_q(out: &str) -> Option<Xset> {
    let line = |prefix: &str| out.lines().map(str::trim).find(|l| l.starts_with(prefix));
    let timeout_line = line("timeout:")?;
    let dpms_line = line("Standby:")?;
    Some(Xset {
        timeout: number_after(timeout_line, "timeout:")?,
        cycle: number_after(timeout_line, "cycle:")?,
        dpms: out.contains("DPMS is Enabled"),
        standby: number_after(dpms_line, "Standby:")?,
        suspend: number_after(dpms_line, "Suspend:")?,
        off: number_after(dpms_line, "Off:")?,
    })
}

fn save(ctx: &Ctx, x: &Xset) -> Result<(), String> {
    let v = json!({"timeout": x.timeout, "cycle": x.cycle, "dpms": x.dpms, "standby": x.standby, "suspend": x.suspend, "off": x.off});
    ctx.write_state(STATE, &v.to_string())
}

fn load(ctx: &Ctx) -> Option<Xset> {
    let v: Value = serde_json::from_str(&ctx.read_state(STATE)?).ok()?;
    let n = |k: &str| v.get(k)?.as_u64().map(|n| n as u32);
    Some(Xset { timeout: n("timeout")?, cycle: n("cycle")?, dpms: v.get("dpms")?.as_bool()?, standby: n("standby")?, suspend: n("suspend")?, off: n("off")? })
}

pub fn status(ctx: &Ctx) -> bool {
    ctx.file(STATE).exists()
}

/// The `xset` invocations that put `x` back.
pub fn restore_commands(x: &Xset) -> Vec<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let mut c = vec![s(&["xset", "s", &x.timeout.to_string(), &x.cycle.to_string()])];
    if x.dpms {
        c.push(s(&["xset", "dpms", &x.standby.to_string(), &x.suspend.to_string(), &x.off.to_string()]));
        c.push(s(&["xset", "+dpms"]));
    }
    c
}

fn xset(sys: &dyn Sys, args: &[&str]) -> Result<(), String> {
    let mut argv = vec!["xset"];
    argv.extend_from_slice(args);
    sys.run(&argv, T).map(|_| ()).ok_or_else(|| format!("`{}` failed", argv.join(" ")))
}

pub fn on(ctx: &Ctx, sys: &dyn Sys) -> Result<String, String> {
    if status(ctx) {
        return Ok("idle inhibit already on".into());
    }
    if ctx.display.is_none() {
        return Err("no X display (DISPLAY is unset)".into());
    }
    if !sys.have("xset") {
        return Err("xset is not installed".into());
    }
    let before = sys.run(&["xset", "q"], T).and_then(|o| parse_xset_q(&o)).ok_or("could not read the screensaver settings from `xset q`")?;
    // Remember first: if the state were written last and xset failed half way, the old values would be lost.
    save(ctx, &before)?;
    if let Err(e) = xset(sys, &["s", "off"]).and_then(|_| xset(sys, &["-dpms"])) {
        ctx.clear_state(STATE);
        let _ = restore(sys, &before);
        return Err(e);
    }
    toast(ctx, sys, "Idle inhibitor enabled", "The screen will not lock or blank", "preferences-desktop-screensaver");
    Ok("idle inhibit on".into())
}

fn restore(sys: &dyn Sys, x: &Xset) -> Result<(), String> {
    for cmd in restore_commands(x) {
        let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
        sys.run(&refs, T).ok_or_else(|| format!("`{}` failed", cmd.join(" ")))?;
    }
    Ok(())
}

pub fn off(ctx: &Ctx, sys: &dyn Sys) -> Result<String, String> {
    let Some(x) = ctx.read_state(STATE).map(|_| load(ctx)) else { return Ok("idle inhibit already off".into()) };
    if let Some(x) = x {
        restore(sys, &x)?;
    }
    ctx.clear_state(STATE);
    toast(ctx, sys, "Idle inhibitor disabled", "Screen lock and blanking restored", "preferences-desktop-screensaver");
    Ok("idle inhibit off".into())
}

#[cfg(test)]
mod tests {
    use super::super::mock::*;
    use super::super::{execute, Cmd, Feature, Switch};
    use super::*;

    const Q: &str = "Keyboard Control:\n  auto repeat:  on\nScreen Saver:\n  prefer blanking:  yes    allow exposures:  yes\n  timeout:  600    cycle:  600\nDPMS (Energy Star):\n  Standby: 0    Suspend: 0    Off: 900\n  DPMS is Enabled\n";

    fn cmd(switch: Switch) -> Cmd {
        Cmd::Do { feature: Feature::Idle, switch, region: false, quiet: false }
    }

    #[test]
    fn parses_xset_q() {
        assert_eq!(parse_xset_q(Q), Some(Xset { timeout: 600, cycle: 600, dpms: true, standby: 0, suspend: 0, off: 900 }));
        let off = Q.replace("DPMS is Enabled", "DPMS is Disabled").replace("timeout:  600", "timeout:  0");
        let x = parse_xset_q(&off).unwrap();
        assert!(!x.dpms && x.timeout == 0);
        assert_eq!(parse_xset_q("nothing useful"), None);
    }

    #[test]
    fn restore_commands_put_back_the_old_values() {
        let x = parse_xset_q(Q).unwrap();
        assert_eq!(restore_commands(&x), [vec!["xset", "s", "600", "600"], vec!["xset", "dpms", "0", "0", "900"], vec!["xset", "+dpms"]]);
        let none = Xset { dpms: false, ..x };
        assert_eq!(restore_commands(&none).len(), 1, "DPMS stays off when it was off");
    }

    #[test]
    fn on_off_cycle_runs_xset_and_restores() {
        let dir = tmp("idle");
        let (c, m) = (ctx(&dir), Mock::new(&["xset"]));
        m.out("xset q", Q);
        m.out("xset s off", "");
        m.out("xset -dpms", "");
        m.out("xset s 600 600", "");
        m.out("xset dpms 0 0 900", "");
        m.out("xset +dpms", "");
        assert_eq!(execute(&cmd(Switch::Status), &c, &m).unwrap(), "off");
        assert_eq!(execute(&cmd(Switch::On), &c, &m).unwrap(), "idle inhibit on");
        assert_eq!(execute(&cmd(Switch::Status), &c, &m).unwrap(), "on");
        assert_eq!(m.log(), ["xset q", "xset s off", "xset -dpms"]);
        // on again: nothing runs
        assert_eq!(execute(&cmd(Switch::On), &c, &m).unwrap(), "idle inhibit already on");
        assert_eq!(m.log().len(), 3);
        assert_eq!(execute(&cmd(Switch::Off), &c, &m).unwrap(), "idle inhibit off");
        assert_eq!(&m.log()[3..], ["xset s 600 600", "xset dpms 0 0 900", "xset +dpms"]);
        assert_eq!(execute(&cmd(Switch::Status), &c, &m).unwrap(), "off");
        // off again: nothing runs
        assert_eq!(execute(&cmd(Switch::Off), &c, &m).unwrap(), "idle inhibit already off");
        assert_eq!(m.log().len(), 6);
        assert_eq!(m.toasts.borrow().iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["Idle inhibitor enabled", "Idle inhibitor disabled"]);
    }

    #[test]
    fn toggle_flips() {
        let dir = tmp("idle-toggle");
        let (c, m) = (ctx(&dir), Mock::new(&["xset"]));
        for l in ["xset q", "xset s off", "xset -dpms", "xset s 600 600", "xset dpms 0 0 900", "xset +dpms"] {
            m.out(l, if l == "xset q" { Q } else { "" });
        }
        execute(&cmd(Switch::Toggle), &c, &m).unwrap();
        assert!(status(&c));
        execute(&cmd(Switch::Toggle), &c, &m).unwrap();
        assert!(!status(&c));
    }

    #[test]
    fn failures_leave_no_state_and_missing_xset_is_clear() {
        let dir = tmp("idle-fail");
        let (c, m) = (ctx(&dir), Mock::new(&["xset"]));
        m.out("xset q", Q);
        // `xset s off` fails (no output registered): the saved state is dropped again
        assert!(execute(&cmd(Switch::On), &c, &m).is_err());
        assert!(!status(&c));
        let m = Mock::new(&[]);
        assert!(execute(&cmd(Switch::On), &c, &m).unwrap_err().contains("xset"));
        let mut c2 = ctx(&dir);
        c2.display = None;
        assert!(execute(&cmd(Switch::On), &c2, &Mock::new(&["xset"])).unwrap_err().contains("DISPLAY"));
    }
}
