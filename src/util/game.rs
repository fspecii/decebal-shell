//! Game mode: stops the compositor (picom) and switches to the performance power profile, then restores both.
//! The Hyprland-only tweaks of the original (animations, blur, gaps) are the compositor's job here: no compositor, no effects.
//!
//! What was running before is kept in `$XDG_RUNTIME_DIR/macutil/game.json`; that file existing is the state.

use super::{toast, Ctx, Sys};
use serde_json::{json, Value};
use std::time::Duration;

const STATE: &str = "game.json";
const T: Duration = Duration::from_secs(3);
const PERFORMANCE: &str = "performance";

/// What game mode changed and must put back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Saved {
    /// picom's full command line (argv split on spaces), when it was running.
    pub picom: Option<Vec<String>>,
    /// The power profile before, when it was changed.
    pub profile: Option<String>,
}

/// First picom line of `pgrep -ax picom` (`PID cmdline...`): the command line to start it again.
pub fn parse_pgrep(out: &str) -> Option<Vec<String>> {
    let line = out.lines().find(|l| !l.trim().is_empty())?;
    let mut words = line.split_whitespace();
    words.next()?.parse::<u32>().ok()?;
    let argv: Vec<String> = words.map(String::from).collect();
    (!argv.is_empty()).then_some(argv)
}

/// `powerprofilesctl list` offers `performance`.
pub fn offers_performance(list: &str) -> bool {
    list.lines().any(|l| l.trim().trim_start_matches('*').trim().trim_end_matches(':') == PERFORMANCE)
}

fn save(ctx: &Ctx, s: &Saved) -> Result<(), String> {
    ctx.write_state(STATE, &json!({"picom": s.picom, "profile": s.profile}).to_string())
}

fn load(ctx: &Ctx) -> Option<Saved> {
    let v: Value = serde_json::from_str(&ctx.read_state(STATE)?).ok()?;
    Some(Saved {
        picom: v.get("picom").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|w| w.as_str().map(String::from)).collect()),
        profile: v.get("profile").and_then(Value::as_str).map(String::from),
    })
}

pub fn status(ctx: &Ctx) -> bool {
    ctx.file(STATE).exists()
}

pub fn on(ctx: &Ctx, sys: &dyn Sys) -> Result<String, String> {
    if status(ctx) {
        return Ok("game mode already on".into());
    }
    let mut saved = Saved::default();
    let mut did = Vec::new();

    if sys.have("pgrep") {
        saved.picom = sys.run(&["pgrep", "-ax", "picom"], T).and_then(|o| parse_pgrep(&o));
    }
    if sys.have("powerprofilesctl") {
        let current = sys.run(&["powerprofilesctl", "get"], T).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let can = sys.run(&["powerprofilesctl", "list"], T).is_some_and(|l| offers_performance(&l));
        if let (Some(cur), true) = (current, can) {
            if cur != PERFORMANCE {
                saved.profile = Some(cur);
            }
        }
    }
    // Record what will be restored before changing anything.
    save(ctx, &saved)?;

    if saved.picom.is_some() && sys.have("pkill") {
        let _ = sys.run(&["pkill", "-x", "picom"], T);
        did.push("compositor stopped");
    } else {
        saved.picom = None;
    }
    if saved.profile.is_some() {
        if sys.run(&["powerprofilesctl", "set", PERFORMANCE], T).is_some() {
            did.push("performance power profile");
        } else {
            saved.profile = None;
        }
    }
    save(ctx, &saved)?;
    let body = if did.is_empty() { "Nothing needed changing".to_string() } else { capitalise(&did.join(", ")) };
    toast(ctx, sys, "Game mode enabled", &body, "input-gaming");
    Ok(format!("game mode on ({})", if did.is_empty() { "nothing to change".to_string() } else { did.join(", ") }))
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

pub fn off(ctx: &Ctx, sys: &dyn Sys) -> Result<String, String> {
    if !status(ctx) {
        return Ok("game mode already off".into());
    }
    let saved = load(ctx).unwrap_or_default();
    let mut err = None;
    if let Some(argv) = &saved.picom {
        // Someone (a supervisor, the user) may have started picom meanwhile: do not run two.
        let again = sys.have("pgrep") && sys.run(&["pgrep", "-x", "picom"], T).is_some();
        if !again {
            if let Err(e) = sys.spawn(argv, None) {
                err = Some(e);
            }
        }
    }
    if let Some(p) = &saved.profile {
        if sys.run(&["powerprofilesctl", "set", p], T).is_none() {
            err = Some(format!("`powerprofilesctl set {p}` failed"));
        }
    }
    ctx.clear_state(STATE);
    toast(ctx, sys, "Game mode disabled", "Compositor and power profile restored", "input-gaming");
    match err {
        Some(e) => Err(e),
        None => Ok("game mode off".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::mock::*;
    use super::super::{execute, Cmd, Feature, Switch};
    use super::*;

    fn cmd(switch: Switch) -> Cmd {
        Cmd::Do { feature: Feature::Game, switch, region: false, quiet: false }
    }

    const LIST: &str = "  performance:\n    CpuDriver:\tamd_pstate\n\n* balanced:\n    CpuDriver:\tamd_pstate\n\n  power-saver:\n";

    fn setup(dir: &std::path::Path) -> (Ctx, Mock) {
        let (c, m) = (ctx(dir), Mock::new(&["pgrep", "pkill", "powerprofilesctl"]));
        m.out("pgrep -ax picom", "812 picom --config /home/ann/.config/picom.conf -b\n");
        m.out("pkill -x picom", "");
        m.out("powerprofilesctl get", "balanced\n");
        m.out("powerprofilesctl list", LIST);
        m.out("powerprofilesctl set performance", "");
        m.out("powerprofilesctl set balanced", "");
        (c, m)
    }

    #[test]
    fn parses_pgrep_and_profile_lists() {
        assert_eq!(parse_pgrep("812 picom --config /x -b\n"), Some(vec!["picom".into(), "--config".into(), "/x".into(), "-b".into()]));
        assert_eq!(parse_pgrep(""), None);
        assert_eq!(parse_pgrep("812\n"), None);
        assert!(offers_performance(LIST));
        assert!(offers_performance("* performance:\n  balanced:\n"));
        assert!(!offers_performance("  balanced:\n  power-saver:\n"));
    }

    #[test]
    fn on_stops_picom_and_sets_performance_then_off_restores() {
        let dir = tmp("game");
        let (c, m) = setup(&dir);
        assert_eq!(execute(&cmd(Switch::Status), &c, &m).unwrap(), "off");
        execute(&cmd(Switch::On), &c, &m).unwrap();
        assert_eq!(execute(&cmd(Switch::Status), &c, &m).unwrap(), "on");
        let log = m.log();
        assert!(log.contains(&"pkill -x picom".to_string()));
        assert!(log.contains(&"powerprofilesctl set performance".to_string()));

        // idempotent
        let n = m.log().len();
        assert_eq!(execute(&cmd(Switch::On), &c, &m).unwrap(), "game mode already on");
        assert_eq!(m.log().len(), n);

        m.out("pgrep -x picom", ""); // pgrep finds nothing... but the mock returns Some("") for registered keys; remove it
        m.outputs.borrow_mut().remove("pgrep -x picom");
        execute(&cmd(Switch::Off), &c, &m).unwrap();
        let log = m.log();
        assert!(log.contains(&"spawn picom --config /home/ann/.config/picom.conf -b".to_string()), "{log:?}");
        assert!(log.contains(&"powerprofilesctl set balanced".to_string()));
        assert_eq!(execute(&cmd(Switch::Status), &c, &m).unwrap(), "off");
        let n = m.log().len();
        assert_eq!(execute(&cmd(Switch::Off), &c, &m).unwrap(), "game mode already off");
        assert_eq!(m.log().len(), n);
        let t: Vec<String> = m.toasts.borrow().iter().map(|t| t.0.clone()).collect();
        assert_eq!(t, ["Game mode enabled", "Game mode disabled"]);
    }

    #[test]
    fn off_does_not_start_a_second_picom() {
        let dir = tmp("game-twice");
        let (c, m) = setup(&dir);
        execute(&cmd(Switch::On), &c, &m).unwrap();
        m.out("pgrep -x picom", "900\n");
        execute(&cmd(Switch::Toggle), &c, &m).unwrap();
        assert!(!m.log().iter().any(|l| l.starts_with("spawn picom")));
    }

    #[test]
    fn works_with_neither_picom_nor_powerprofilesctl() {
        let dir = tmp("game-bare");
        let (c, m) = (ctx(&dir), Mock::new(&["pgrep", "pkill"]));
        let out = execute(&cmd(Switch::On), &c, &m).unwrap();
        assert!(out.contains("nothing to change"), "{out}");
        assert!(status(&c), "the state still flips so the tile and the toggle stay consistent");
        execute(&cmd(Switch::Off), &c, &m).unwrap();
        assert!(!status(&c));
        assert!(!m.log().iter().any(|l| l.starts_with("spawn")));
    }

    #[test]
    fn an_already_performance_profile_is_left_alone() {
        let dir = tmp("game-perf");
        let (c, m) = setup(&dir);
        m.out("powerprofilesctl get", "performance\n");
        execute(&cmd(Switch::On), &c, &m).unwrap();
        execute(&cmd(Switch::Off), &c, &m).unwrap();
        assert!(!m.log().iter().any(|l| l.starts_with("powerprofilesctl set")));
    }
}
