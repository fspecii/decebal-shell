//! What a widget click, scroll or button can do: a fixed allowlist. This is the whole security boundary for theme widgets.
//!
//! ```text
//! open dashboard|launcher|spotlight|session|notifications|overview
//! rdwm <action> [arg]        action from SAFE_RDWM (no spawn, quit, kill, scratchpads, mouse grabs)
//! workspace <n>|next|prev
//! dnd toggle
//! volume up|down|mute
//! brightness up|down
//! media play-pause|next|prev
//! exec <command line>        user widgets only, and only with widgets.allow_exec = true
//! ```
//! Every action maps to a fixed argument vector run directly (never through a shell), except `exec`.

use super::Trust;

/// rdwm actions a widget may trigger. Anything that starts a program (`spawn`, `togglescratch`), ends the session (`quit`,
/// `kill`), or grabs the pointer is left out.
pub const SAFE_RDWM: &[&str] = &[
    "view",
    "toggleview",
    "viewnext",
    "viewprev",
    "focusstack",
    "focusmon",
    "cyclelayout",
    "cyclelayout_back",
    "setlayout",
    "togglefloating",
    "togglefullscreen",
    "togglemaximize",
    "minimize",
    "unminimize",
    "zoom",
    "togglegaps",
    "togglebar",
    "tileall",
    "snap_left",
    "snap_right",
    "snap_up",
    "snap_down",
    "snap_restore",
    "focus_left",
    "focus_right",
    "focus_up",
    "focus_down",
    "reload",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    Dashboard,
    Launcher,
    Spotlight,
    Session,
    Notifications,
    Overview,
}

impl Open {
    pub const ALL: [(Open, &'static str); 6] = [
        (Open::Dashboard, "dashboard"),
        (Open::Launcher, "launcher"),
        (Open::Spotlight, "spotlight"),
        (Open::Session, "session"),
        (Open::Notifications, "notifications"),
        (Open::Overview, "overview"),
    ];

    /// The program (and its argument) that shows it.
    pub fn argv(self) -> &'static [&'static str] {
        match self {
            Open::Dashboard => &["macdash"],
            Open::Launcher => &["maclaunch"],
            Open::Spotlight => &["macspot"],
            Open::Session => &["macsession"],
            Open::Notifications => &["macnotify", "history"],
            Open::Overview => &["macexpose"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ws {
    N(u32),
    Next,
    Prev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Media {
    PlayPause,
    Next,
    Prev,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Open(Open),
    Rdwm {
        action: String,
        arg: Option<String>,
    },
    Workspace(Ws),
    DndToggle,
    Volume(Option<Step>),
    Brightness(Step),
    Media(Media),
    /// A shell command line. Only produced for [`Trust::UserExec`].
    Exec(String),
}

impl Action {
    /// Parses an action line. `trust` decides whether `exec` is accepted.
    pub fn parse(s: &str, trust: Trust) -> Result<Action, String> {
        let s = s.trim();
        let (head, rest) = s.split_once(char::is_whitespace).map(|(h, r)| (h, r.trim())).unwrap_or((s, ""));
        let word = |r: &str| r.to_ascii_lowercase();
        let bad = |what: &str| Err(format!("`{s}`: {what}"));
        match head {
            "open" => Open::ALL
                .iter()
                .find(|(_, n)| *n == word(rest))
                .map(|(o, _)| Action::Open(*o))
                .ok_or_else(|| format!("`{s}`: open what? one of {}", Open::ALL.iter().map(|x| x.1).collect::<Vec<_>>().join(", "))),
            "rdwm" => {
                let mut it = rest.split_whitespace();
                let Some(name) = it.next() else { return bad("rdwm needs an action name") };
                if !SAFE_RDWM.contains(&name) {
                    return bad(&format!("rdwm action `{name}` is not allowed in widgets (allowed: {})", SAFE_RDWM.join(", ")));
                }
                let arg = it.next().map(String::from);
                if it.next().is_some() {
                    return bad("at most one argument");
                }
                if let Some(a) = &arg {
                    if a.len() > 24 || !a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-')) {
                        return bad("the argument may only contain letters, digits and _ . + -");
                    }
                }
                Ok(Action::Rdwm { action: name.into(), arg })
            }
            "workspace" => match word(rest).as_str() {
                "next" => Ok(Action::Workspace(Ws::Next)),
                "prev" | "previous" => Ok(Action::Workspace(Ws::Prev)),
                n => n.parse::<u32>().ok().filter(|n| (1..=32).contains(n)).map(|n| Action::Workspace(Ws::N(n))).ok_or_else(|| format!("`{s}`: workspace takes 1..32, next or prev")),
            },
            "dnd" if word(rest) == "toggle" => Ok(Action::DndToggle),
            "volume" => match word(rest).as_str() {
                "up" => Ok(Action::Volume(Some(Step::Up))),
                "down" => Ok(Action::Volume(Some(Step::Down))),
                "mute" => Ok(Action::Volume(None)),
                _ => bad("volume takes up, down or mute"),
            },
            "brightness" => match word(rest).as_str() {
                "up" => Ok(Action::Brightness(Step::Up)),
                "down" => Ok(Action::Brightness(Step::Down)),
                _ => bad("brightness takes up or down"),
            },
            "media" => match word(rest).as_str() {
                "play-pause" | "playpause" | "toggle" => Ok(Action::Media(Media::PlayPause)),
                "next" => Ok(Action::Media(Media::Next)),
                "prev" | "previous" => Ok(Action::Media(Media::Prev)),
                _ => bad("media takes play-pause, next or prev"),
            },
            "exec" => match trust {
                Trust::UserExec if !rest.is_empty() && rest.len() <= 400 && !rest.contains('\0') => Ok(Action::Exec(rest.into())),
                Trust::UserExec => bad("exec needs a command line (at most 400 characters)"),
                Trust::User => bad("`exec` needs `widgets.allow_exec = true` in macshell.json"),
                Trust::Theme => bad("widgets that come with a theme cannot run commands; use one of the allowed actions (open, rdwm, workspace, dnd, volume, brightness, media)"),
            },
            "" => bad("empty action"),
            _ => bad("unknown action; use open, rdwm, workspace, dnd, volume, brightness, media (or exec in your own widgets)"),
        }
    }

    /// The fixed argument vector this action runs, when it is a plain program invocation.
    pub fn argv(&self) -> Option<Vec<String>> {
        let s = |v: &[&str]| Some(v.iter().map(|x| x.to_string()).collect());
        match self {
            Action::Open(o) => s(o.argv()),
            Action::Rdwm { action, arg } => {
                let mut v = vec!["rdwmctl".to_string(), "action".into(), action.clone()];
                v.extend(arg.clone());
                Some(v)
            }
            Action::Workspace(Ws::N(n)) => Some(vec!["rdwmctl".into(), "action".into(), "view".into(), n.to_string()]),
            Action::Workspace(Ws::Next) => s(&["rdwmctl", "action", "viewnext"]),
            Action::Workspace(Ws::Prev) => s(&["rdwmctl", "action", "viewprev"]),
            _ => None,
        }
    }
}

/// Runs the action. Fire-and-forget; never blocks the UI thread for long.
pub fn run(a: &Action) {
    use crate::controlcenter::sys;
    match a {
        Action::Exec(cmd) => {
            let cmd = cmd.clone();
            std::thread::spawn(move || {
                let _ = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map(|mut c| c.wait());
            });
        }
        Action::DndToggle => sys::toggle_dnd(),
        Action::Volume(None) => sys::toggle_mute(),
        Action::Volume(Some(step)) => {
            let step = *step;
            std::thread::spawn(move || {
                if let Some((v, _)) = sys::probe_volume() {
                    sys::set_volume(v.level + if step == Step::Up { 0.05 } else { -0.05 });
                }
            });
        }
        Action::Brightness(step) => {
            let step = *step;
            std::thread::spawn(move || {
                if let Some(b) = sys::probe_brightness() {
                    sys::set_brightness((b as i32 + if step == Step::Up { 5 } else { -5 }).clamp(1, 100) as u32);
                }
            });
        }
        Action::Media(m) => sys::media_command(match m {
            Media::PlayPause => "play-pause",
            Media::Next => "next",
            Media::Prev => "previous",
        }),
        other => {
            if let Some(argv) = other.argv() {
                let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                sys::spawn(&refs);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowlist_parses() {
        for (line, want) in [
            ("open dashboard", Action::Open(Open::Dashboard)),
            ("open Launcher", Action::Open(Open::Launcher)),
            ("workspace 3", Action::Workspace(Ws::N(3))),
            ("workspace next", Action::Workspace(Ws::Next)),
            ("dnd toggle", Action::DndToggle),
            ("volume up", Action::Volume(Some(Step::Up))),
            ("volume mute", Action::Volume(None)),
            ("brightness down", Action::Brightness(Step::Down)),
            ("media next", Action::Media(Media::Next)),
            ("rdwm cyclelayout", Action::Rdwm { action: "cyclelayout".into(), arg: None }),
            ("rdwm view 4", Action::Rdwm { action: "view".into(), arg: Some("4".into()) }),
        ] {
            assert_eq!(Action::parse(line, Trust::Theme).unwrap(), want, "{line}");
        }
    }

    #[test]
    fn theme_widgets_cannot_run_commands_or_dangerous_rdwm_actions() {
        for bad in [
            "exec rm -rf ~",
            "exec",
            "rdwm spawn xterm",
            "rdwm quit",
            "rdwm kill",
            "rdwm togglescratch",
            "rdwm view 1;ls",
            "rdwm view $(id)",
            "rdwm view 1 2",
            "open /bin/sh",
            "sh -c id",
            "",
            "spawn foo",
            "workspace 0",
            "workspace 99",
            "volume 100",
            "dnd on",
        ] {
            assert!(Action::parse(bad, Trust::Theme).is_err(), "{bad:?} must be rejected");
        }
        let e = Action::parse("exec echo hi", Trust::Theme).unwrap_err();
        assert!(e.contains("cannot run commands"), "{e}");
    }

    #[test]
    fn exec_needs_a_user_widget_and_the_switch() {
        assert!(Action::parse("exec echo hi", Trust::User).unwrap_err().contains("allow_exec"));
        assert_eq!(Action::parse("exec echo hi", Trust::UserExec).unwrap(), Action::Exec("echo hi".into()));
        assert!(Action::parse("exec ", Trust::UserExec).is_err());
        // the allowlist is the same either way
        assert!(Action::parse("rdwm spawn x", Trust::UserExec).is_err());
    }

    #[test]
    fn actions_map_to_fixed_argument_vectors() {
        assert_eq!(Action::parse("open notifications", Trust::Theme).unwrap().argv().unwrap(), ["macnotify", "history"]);
        assert_eq!(Action::parse("workspace 3", Trust::Theme).unwrap().argv().unwrap(), ["rdwmctl", "action", "view", "3"]);
        assert_eq!(Action::parse("rdwm view 8", Trust::Theme).unwrap().argv().unwrap(), ["rdwmctl", "action", "view", "8"]);
        for a in SAFE_RDWM {
            assert!(!matches!(*a, "spawn" | "quit" | "kill" | "togglescratch" | "movemouse" | "resizemouse" | "mode"), "{a}");
        }
    }
}
