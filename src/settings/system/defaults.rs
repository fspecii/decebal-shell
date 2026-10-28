//! Default applications: browser, mail, terminal, file manager, media player, image viewer, text editor, PDF viewer.
//! Candidates come from the installed `.desktop` files that claim the MIME type; the current choice from
//! `xdg-mime query default`; changes go through `xdg-settings` / `xdg-mime default`.
//! The terminal is not a MIME handler: `rdwm-terminal` honours `RDWM_TERMINAL`, which decsettings exports from
//! `session-settings.sh` at login.

use super::run::{Cmd, Runner};
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Opt, Row};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cat {
    pub id: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    /// The first MIME type is the one queried; all are set.
    pub mimes: &'static [&'static str],
}

pub const CATS: &[Cat] = &[
    Cat { id: "browser", label: "Web browser", detail: "Opens links and web pages", mimes: &["x-scheme-handler/https", "x-scheme-handler/http", "text/html"] },
    Cat { id: "mail", label: "Email", detail: "Opens mailto: links", mimes: &["x-scheme-handler/mailto"] },
    Cat { id: "files", label: "File manager", detail: "Opens folders", mimes: &["inode/directory"] },
    Cat { id: "video", label: "Video player", detail: "", mimes: &["video/mp4", "video/x-matroska", "video/webm"] },
    Cat { id: "music", label: "Music player", detail: "", mimes: &["audio/mpeg", "audio/flac", "audio/ogg"] },
    Cat { id: "image", label: "Image viewer", detail: "", mimes: &["image/png", "image/jpeg", "image/webp"] },
    Cat { id: "text", label: "Text editor", detail: "", mimes: &["text/plain"] },
    Cat { id: "pdf", label: "PDF viewer", detail: "", mimes: &["application/pdf"] },
];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Desktop {
    pub id: String,
    pub name: String,
    pub mimes: Vec<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    /// Per category: (current desktop id, candidates).
    pub cats: Vec<(Option<String>, Vec<Desktop>)>,
    pub terminals: Vec<String>,
    pub has_xdg: bool,
}

/// Parses the `[Desktop Entry]` group of a `.desktop` file.
pub fn parse_desktop(id: &str, text: &str) -> Option<Desktop> {
    let mut in_entry = false;
    let mut d = Desktop { id: id.into(), ..Desktop::default() };
    let mut kind_app = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_entry = l == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            match k.trim() {
                "Name" => d.name = v.trim().to_string(),
                "Type" => kind_app = v.trim() == "Application",
                "NoDisplay" | "Hidden" => d.hidden |= v.trim() == "true",
                "MimeType" => d.mimes = v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
                _ => {}
            }
        }
    }
    (kind_app && !d.name.is_empty()).then_some(d)
}

pub fn app_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter().chain(dirs.split(':').filter(|s| !s.is_empty()).map(PathBuf::from)).map(|p| p.join("applications")).collect()
}

pub fn scan(dirs: &[PathBuf]) -> Vec<Desktop> {
    let mut out: Vec<Desktop> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".desktop") || out.iter().any(|x| x.id == name) {
                continue;
            }
            if let Some(t) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_desktop(&name, &t)) {
                out.push(t);
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

pub const TERMINALS: &[&str] = &["alacritty", "kitty", "ghostty", "wezterm", "foot", "st", "xterm", "urxvt", "konsole", "gnome-terminal", "terminator", "xfce4-terminal"];

pub fn load(r: &dyn Runner) -> State {
    load_from(r, &app_dirs(), &|p| super::run::which(p).is_some())
}

pub fn load_from(r: &dyn Runner, dirs: &[PathBuf], have: &dyn Fn(&str) -> bool) -> State {
    let all = scan(dirs);
    let has_xdg = r.exists("xdg-mime");
    let cats = CATS
        .iter()
        .map(|c| {
            let cands: Vec<Desktop> = all.iter().filter(|d| !d.hidden && c.mimes.iter().any(|m| d.mimes.iter().any(|x| x == m))).cloned().collect();
            let cur = if has_xdg {
                let o = r.run(&Cmd::new("xdg-mime", &["query", "default", c.mimes[0]]).timeout(5));
                Some(o.stdout.trim().to_string()).filter(|s| o.ok && !s.is_empty())
            } else {
                None
            };
            (cur, cands)
        })
        .collect();
    State { cats, terminals: TERMINALS.iter().filter(|t| have(t)).map(|t| t.to_string()).collect(), has_xdg }
}

pub fn set_cmds(c: &Cat, desktop_id: &str, cands: &[Desktop], have_xdg_settings: bool) -> Result<Vec<Cmd>, String> {
    if !cands.iter().any(|d| d.id == desktop_id) || desktop_id.contains('/') || !desktop_id.ends_with(".desktop") {
        return Err("That application cannot open this kind of file".into());
    }
    if c.id == "browser" && have_xdg_settings {
        return Ok(vec![Cmd::new("xdg-settings", &["set", "default-web-browser", desktop_id])]);
    }
    let mut a = vec!["default".to_string(), desktop_id.to_string()];
    a.extend(c.mimes.iter().map(|m| m.to_string()));
    Ok(vec![Cmd::owned("xdg-mime", a)])
}

pub fn items(h: &Hub) -> Vec<Item> {
    let Some(st) = &h.defaults.data else { return vec![Item::Note { text: "Looking for installed applications...".into(), warn: false }] };
    let mut v = vec![Item::Header("Applications".into())];
    let mut rows = Vec::new();
    if !st.has_xdg {
        v.push(Item::Note { text: "xdg-mime (xdg-utils) is not installed, so the current defaults cannot be read or changed.".into(), warn: true });
    }
    for (c, (cur, cands)) in CATS.iter().zip(&st.cats) {
        if cands.is_empty() {
            rows.push(Row::new(c.label).detail("No installed application handles this"));
            continue;
        }
        let opts: Vec<Opt> = cands.iter().map(|d| (d.name.clone(), d.id.clone())).collect();
        let sel = cur.as_ref().and_then(|c| opts.iter().position(|(_, v)| v == c));
        let mut ctl = Ctl::new(format!("s:def:set:{}", c.id), Kind::Popup(opts, sel));
        ctl.enabled = st.has_xdg;
        rows.push(Row::new(c.label).detail(c.detail).ctl(ctl));
    }
    v.push(Item::Card(rows));
    v.push(Item::Header("Terminal".into()));
    if st.terminals.is_empty() {
        v.push(Item::Card(vec![Row::new("Terminal").detail("No known terminal emulator found on PATH")]));
    } else {
        let mut opts: Vec<Opt> = vec![("Automatic".into(), "".into())];
        opts.extend(st.terminals.iter().map(|t| (t.clone(), t.clone())));
        let cur = h.session.terminal.clone().or_else(|| std::env::var("RDWM_TERMINAL").ok().filter(|s| !s.is_empty()));
        let sel = Some(cur.as_ref().and_then(|c| opts.iter().position(|(_, v)| v == c)).unwrap_or(0));
        v.push(Item::Card(vec![Row::new("Terminal").detail("Used by rdwm-terminal; saved as RDWM_TERMINAL for your next login").ctl(Ctl::new("s:def:term:", Kind::Popup(opts, sel)))]));
        v.push(Item::Note { text: "Automatic picks the first installed terminal that works in this session. A fixed choice is exported as RDWM_TERMINAL by ~/.config/decebal/session-settings.sh, which the login session sources.".into(), warn: false });
    }
    v
}

pub fn apply(h: &mut Hub, act: &str, arg: &str, val: CtlVal) {
    let Some(st) = h.defaults.data.clone() else { return };
    let r: Result<(), String> = (|| {
        match (act, &val) {
            ("set", CtlVal::Str(id)) => {
                let i = CATS.iter().position(|c| c.id == arg).ok_or("Unknown category")?;
                let cmds = set_cmds(&CATS[i], id, &st.cats[i].1, h.sh.runner.exists("xdg-settings"))?;
                let undo = st.cats[i].0.as_ref().map(|old| set_cmds(&CATS[i], old, &st.cats[i].1, h.sh.runner.exists("xdg-settings"))).transpose().unwrap_or(None).unwrap_or_default();
                let name = st.cats[i].1.iter().find(|d| d.id == *id).map(|d| d.name.clone()).unwrap_or_default();
                if let Some(s) = h.defaults.data.as_mut() {
                    s.cats[i].0 = Some(id.clone());
                }
                h.run(Action::new(Dom::Defaults, CATS[i].label, cmds).undo(undo).msg(format!("{name} is now the default {}", CATS[i].label.to_lowercase())));
            }
            ("term", CtlVal::Str(t)) => {
                if !t.is_empty() && !st.terminals.contains(t) {
                    return Err(format!("{t} is not installed"));
                }
                let mut s = h.session.clone();
                s.terminal = (!t.is_empty()).then(|| t.clone());
                h.run_session(
                    Action::new(Dom::Defaults, "Terminal", vec![]).msg(if t.is_empty() {
                        "Terminal: automatic (from the next login)".to_string()
                    } else {
                        format!("Terminal set to {t} (from the next login)")
                    }),
                    s,
                );
            }
            _ => return Err("Unsupported change".into()),
        }
        Ok(())
    })();
    if let Err(e) = r {
        h.say(e, true);
    }
}

pub fn dir_exists(p: &Path) -> bool {
    p.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::FakeRunner;

    const FF: &str = "[Desktop Entry]\nType=Application\nName=Firefox\nName[de]=Feuerfuchs\nMimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;\nExec=firefox %u\n[Desktop Action new-window]\nName=New Window\nMimeType=ignored/me;\n";
    const HID: &str = "[Desktop Entry]\nType=Application\nName=Helper\nNoDisplay=true\nMimeType=text/html;\n";
    const MPV: &str = "[Desktop Entry]\nType=Application\nName=mpv\nMimeType=video/mp4;audio/mpeg;\n";

    fn apps() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ms-def-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("firefox.desktop"), FF).unwrap();
        std::fs::write(d.join("helper.desktop"), HID).unwrap();
        std::fs::write(d.join("mpv.desktop"), MPV).unwrap();
        std::fs::write(d.join("notes.txt"), "x").unwrap();
        d
    }

    #[test]
    fn parses_desktop_entries() {
        let d = parse_desktop("firefox.desktop", FF).unwrap();
        assert_eq!(d.name, "Firefox");
        assert_eq!(d.mimes, ["text/html", "x-scheme-handler/http", "x-scheme-handler/https"]);
        assert!(parse_desktop("h.desktop", HID).unwrap().hidden);
        assert!(parse_desktop("x.desktop", "[Desktop Entry]\nType=Link\nName=x\n").is_none());
        assert!(parse_desktop("x.desktop", "garbage").is_none());
    }

    #[test]
    fn load_finds_candidates_and_current() {
        let d = apps();
        let r = FakeRunner::new().ok("xdg-mime query default x-scheme-handler/https", "firefox.desktop\n").ok("xdg-mime query default video/mp4", "mpv.desktop\n");
        let s = load_from(&r, std::slice::from_ref(&d), &|t| t == "alacritty" || t == "xterm");
        let b = &s.cats[0];
        assert_eq!(b.0.as_deref(), Some("firefox.desktop"));
        assert_eq!(b.1.len(), 1, "hidden entries are not offered");
        assert_eq!(s.cats[3].1[0].name, "mpv");
        assert!(s.cats[1].1.is_empty());
        assert_eq!(s.terminals, ["alacritty", "xterm"]);
        let s = load_from(&FakeRunner::new(), &[d], &|_| false);
        assert!(!s.has_xdg && s.cats[0].0.is_none());
    }

    #[test]
    fn set_commands() {
        let cands = vec![Desktop { id: "firefox.desktop".into(), name: "Firefox".into(), ..Desktop::default() }];
        assert_eq!(set_cmds(&CATS[0], "firefox.desktop", &cands, true).unwrap()[0].line(), "xdg-settings set default-web-browser firefox.desktop");
        assert_eq!(set_cmds(&CATS[0], "firefox.desktop", &cands, false).unwrap()[0].line(), "xdg-mime default firefox.desktop x-scheme-handler/https x-scheme-handler/http text/html");
        assert_eq!(set_cmds(&CATS[1], "firefox.desktop", &cands, true).unwrap()[0].line(), "xdg-mime default firefox.desktop x-scheme-handler/mailto");
        assert!(set_cmds(&CATS[0], "evil.desktop", &cands, true).is_err());
        assert!(set_cmds(&CATS[0], "../x.desktop", &[Desktop { id: "../x.desktop".into(), ..Desktop::default() }], true).is_err());
    }

    #[test]
    fn terminal_choice_is_persisted_not_executed() {
        let d = std::env::temp_dir().join(format!("ms-def-term-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let fr = std::sync::Arc::new(FakeRunner::lenient());
        let mut h = Hub::with(fr.clone(), d.clone(), std::sync::Arc::new(|| {}));
        h.defaults.data = Some(State { terminals: vec!["alacritty".into()], has_xdg: true, ..State::default() });
        apply(&mut h, "term", "", CtlVal::Str("alacritty".into()));
        apply(&mut h, "term", "", CtlVal::Str("rm".into()));
        assert!(h.msgs.iter().any(|m| m.1));
        assert_eq!(h.session.terminal.as_deref(), Some("alacritty"));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(fr.lines().is_empty());
        assert!(std::fs::read_to_string(super::super::session::Session::sh_path(&d)).unwrap().contains("export RDWM_TERMINAL=alacritty"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
