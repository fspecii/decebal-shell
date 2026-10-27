//! Screen recording: `ffmpeg -f x11grab` on X11, `wf-recorder` on Wayland, region by `slop` / `slurp`.
//!
//! The recorder runs detached; its pid and the output file are kept in `$XDG_RUNTIME_DIR/macutil/record.json`.
//! Stopping sends SIGINT (both tools finish the file on it), waits for the process, and then notifies.

use super::{tilde, toast, Ctx, Sys};
use serde_json::{json, Value};
use std::time::Duration;

const STATE: &str = "record.json";
const LOG: &str = "record.log";
const FRAMERATE: u32 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rec {
    pub pid: u32,
    pub file: String,
    pub started: u64,
    pub tool: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Geometry {
    /// libx264 with yuv420p wants even sizes.
    pub fn even(self) -> Geometry {
        Geometry { w: (self.w & !1).max(2), h: (self.h & !1).max(2), ..self }
    }
}

/// `slop -f "%x %y %w %h"` output.
pub fn parse_slop(out: &str) -> Option<Geometry> {
    let n: Vec<i32> = out.split_whitespace().map(|t| t.parse().ok()).collect::<Option<_>>()?;
    match n[..] {
        [x, y, w, h] if w > 0 && h > 0 => Some(Geometry { x, y, w: w as u32, h: h as u32 }),
        _ => None,
    }
}

/// `slurp` output: `X,Y WxH`.
pub fn parse_slurp(out: &str) -> Option<Geometry> {
    let (pos, size) = out.trim().split_once(' ')?;
    let (x, y) = pos.split_once(',')?;
    let (w, h) = size.split_once('x')?;
    let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0).then_some(Geometry { x: x.parse().ok()?, y: y.parse().ok()?, w, h })
}

/// `recording-2026-10-27_14-03-09.mp4` from broken-down local time.
pub fn file_name(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> String {
    format!("recording-{y:04}-{mo:02}-{d:02}_{h:02}-{mi:02}-{s:02}.mp4")
}

fn local_file_name(unix: u64) -> String {
    // SAFETY: localtime_r fills the struct we pass; it keeps no pointer to it.
    let tm = unsafe {
        let t = unix as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    file_name(tm.tm_year + 1900, (tm.tm_mon + 1) as u32, tm.tm_mday as u32, tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    WfRecorder,
}

/// Which recorder fits the session and is installed.
pub fn pick_tool(ctx: &Ctx, sys: &dyn Sys) -> Result<Tool, String> {
    if ctx.wayland && sys.have("wf-recorder") {
        Ok(Tool::WfRecorder)
    } else if ctx.display.is_some() && sys.have("ffmpeg") {
        Ok(Tool::Ffmpeg)
    } else if ctx.display.is_none() && !ctx.wayland {
        Err("no display to record (DISPLAY and WAYLAND_DISPLAY are unset)".into())
    } else if ctx.wayland {
        Err("wf-recorder is not installed".into())
    } else {
        Err("ffmpeg is not installed".into())
    }
}

/// The recorder's command line.
pub fn build_command(tool: Tool, display: &str, region: Option<Geometry>, file: &str) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match tool {
        Tool::Ffmpeg => {
            let mut c = s(&["ffmpeg", "-y", "-loglevel", "error", "-f", "x11grab", "-framerate", &FRAMERATE.to_string()]);
            let input = match region.map(Geometry::even) {
                Some(g) => {
                    c.extend(s(&["-video_size", &format!("{}x{}", g.w, g.h)]));
                    format!("{display}+{},{}", g.x.max(0), g.y.max(0))
                }
                None => display.to_string(),
            };
            c.extend(s(&["-i", &input, "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23", "-pix_fmt", "yuv420p", file]));
            c
        }
        Tool::WfRecorder => {
            let mut c = s(&["wf-recorder", "-f", file]);
            if let Some(g) = region.map(Geometry::even) {
                c.extend(s(&["-g", &format!("{},{} {}x{}", g.x, g.y, g.w, g.h)]));
            }
            c
        }
    }
}

fn load(ctx: &Ctx) -> Option<Rec> {
    let v: Value = serde_json::from_str(&ctx.read_state(STATE)?).ok()?;
    Some(Rec { pid: v.get("pid")?.as_u64()? as u32, file: v.get("file")?.as_str()?.into(), started: v.get("started")?.as_u64()?, tool: v.get("tool")?.as_str()?.into() })
}

fn save(ctx: &Ctx, r: &Rec) -> Result<(), String> {
    ctx.write_state(STATE, &json!({"pid": r.pid, "file": r.file, "started": r.started, "tool": r.tool}).to_string())
}

/// The running recording, if any. A state file whose process is gone (crash, killed) is removed.
pub fn status(ctx: &Ctx, sys: &dyn Sys) -> Option<Rec> {
    let rec = load(ctx)?;
    // The pid could have been reused by now: its command line must still name our output file.
    if sys.alive(rec.pid) && sys.cmdline_has(rec.pid, &rec.file) {
        Some(rec)
    } else {
        ctx.clear_state(STATE);
        None
    }
}

pub fn start(ctx: &Ctx, sys: &dyn Sys, region: bool) -> Result<String, String> {
    if let Some(r) = status(ctx, sys) {
        return Ok(format!("already recording to {}", r.file));
    }
    let tool = pick_tool(ctx, sys)?;
    let geometry = if region { Some(select_region(ctx, sys, tool)?) } else { None };
    std::fs::create_dir_all(&ctx.videos).map_err(|e| format!("{}: {e}", ctx.videos.display()))?;
    let file = ctx.videos.join(local_file_name(ctx.now)).to_string_lossy().into_owned();
    let argv = build_command(tool, ctx.display.as_deref().unwrap_or(":0"), geometry, &file);
    let log = ctx.file(LOG);
    let _ = std::fs::create_dir_all(&ctx.state);
    let pid = sys.spawn(&argv, Some(&log))?;
    // A recorder that cannot reach the display or encode dies at once: say so instead of pretending.
    sys.sleep(ctx.settle);
    if !sys.alive(pid) {
        let why = std::fs::read_to_string(&log).ok().and_then(|t| t.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string)).unwrap_or_else(|| "it exited immediately".into());
        toast(ctx, sys, "Recording failed", &why, "dialog-error");
        return Err(format!("{} failed: {why}", argv[0]));
    }
    save(ctx, &Rec { pid, file: file.clone(), started: ctx.now, tool: argv[0].clone() })?;
    Ok(format!("recording to {file}"))
}

fn select_region(ctx: &Ctx, sys: &dyn Sys, tool: Tool) -> Result<Geometry, String> {
    let wait = Duration::from_secs(600);
    let picked = match tool {
        Tool::WfRecorder if sys.have("slurp") => sys.run(&["slurp", "-f", "%x,%y %wx%h"], wait).and_then(|o| parse_slurp(&o)),
        Tool::Ffmpeg if sys.have("slop") => sys.run(&["slop", "-f", "%x %y %w %h"], wait).and_then(|o| parse_slop(&o)),
        Tool::WfRecorder => return Err("--region needs slurp".into()),
        Tool::Ffmpeg => return Err("--region needs slop".into()),
    };
    picked.ok_or_else(|| {
        toast(ctx, sys, "Recording cancelled", "No region was selected", "dialog-information");
        "no region selected".into()
    })
}

pub fn stop(ctx: &Ctx, sys: &dyn Sys) -> Result<String, String> {
    let Some(rec) = status(ctx, sys) else { return Ok("not recording".into()) };
    // SIGINT makes both tools write the end of the file; give them time, then insist.
    sys.signal(rec.pid, libc::SIGINT);
    let step = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while sys.alive(rec.pid) && waited < ctx.grace {
        sys.sleep(step);
        waited += step;
    }
    if sys.alive(rec.pid) {
        sys.signal(rec.pid, libc::SIGTERM);
        sys.sleep(Duration::from_millis(500));
        if sys.alive(rec.pid) {
            sys.signal(rec.pid, libc::SIGKILL);
        }
    }
    ctx.clear_state(STATE);
    let size = std::fs::metadata(&rec.file).map_or(0, |m| m.len());
    if size > 0 {
        toast(ctx, sys, "Recording saved", &tilde(&rec.file), "media-record");
        Ok(format!("saved {}", rec.file))
    } else {
        toast(ctx, sys, "Recording failed", "The file is empty", "dialog-error");
        Err(format!("{} is empty", rec.file))
    }
}

#[cfg(test)]
mod tests {
    use super::super::mock::*;
    use super::super::{execute, Cmd, Feature, Switch};
    use super::*;

    fn rec_cmd(switch: Switch, region: bool) -> Cmd {
        Cmd::Do { feature: Feature::Record, switch, region, quiet: false }
    }

    #[test]
    fn parses_slop_and_slurp() {
        assert_eq!(parse_slop("10 20 300 200\n"), Some(Geometry { x: 10, y: 20, w: 300, h: 200 }));
        assert_eq!(parse_slop("10 20 0 200"), None);
        assert_eq!(parse_slop(""), None);
        assert_eq!(parse_slop("a b c d"), None);
        assert_eq!(parse_slurp("10,20 300x200\n"), Some(Geometry { x: 10, y: 20, w: 300, h: 200 }));
        assert_eq!(parse_slurp("garbage"), None);
    }

    #[test]
    fn file_names_are_dated() {
        assert_eq!(file_name(2026, 10, 7, 4, 3, 9), "recording-2026-10-07_04-03-09.mp4");
        assert!(local_file_name(1_700_000_000).starts_with("recording-2023-1"));
    }

    #[test]
    fn ffmpeg_command_for_the_full_screen_and_a_region() {
        let full = build_command(Tool::Ffmpeg, ":1", None, "/v/a.mp4");
        assert_eq!(&full[..9], ["ffmpeg", "-y", "-loglevel", "error", "-f", "x11grab", "-framerate", "30", "-i"]);
        assert_eq!(full[9], ":1");
        assert_eq!(full.last().unwrap(), "/v/a.mp4");
        assert!(full.windows(2).any(|w| w == ["-c:v", "libx264"]) && full.windows(2).any(|w| w == ["-pix_fmt", "yuv420p"]));
        let reg = build_command(Tool::Ffmpeg, ":1", Some(Geometry { x: 11, y: 22, w: 301, h: 200 }), "/v/a.mp4");
        assert!(reg.windows(2).any(|w| w == ["-video_size", "300x200"]), "odd widths are made even: {reg:?}");
        assert!(reg.windows(2).any(|w| w == ["-i", ":1+11,22"]));
        let wf = build_command(Tool::WfRecorder, ":1", Some(Geometry { x: 5, y: 6, w: 100, h: 50 }), "/v/a.mp4");
        assert_eq!(wf, ["wf-recorder", "-f", "/v/a.mp4", "-g", "5,6 100x50"]);
        assert_eq!(build_command(Tool::WfRecorder, ":1", None, "/v/a.mp4"), ["wf-recorder", "-f", "/v/a.mp4"]);
    }

    #[test]
    fn tool_follows_the_session() {
        let dir = tmp("tool");
        let mut c = ctx(&dir);
        assert_eq!(pick_tool(&c, &Mock::new(&["ffmpeg", "wf-recorder"])), Ok(Tool::Ffmpeg));
        assert!(pick_tool(&c, &Mock::new(&["wf-recorder"])).unwrap_err().contains("ffmpeg"));
        c.wayland = true;
        assert_eq!(pick_tool(&c, &Mock::new(&["ffmpeg", "wf-recorder"])), Ok(Tool::WfRecorder));
        assert_eq!(pick_tool(&c, &Mock::new(&["ffmpeg"])), Ok(Tool::Ffmpeg), "XWayland display still works with ffmpeg");
        c.display = None;
        assert!(pick_tool(&c, &Mock::new(&["ffmpeg"])).unwrap_err().contains("wf-recorder"));
        c.wayland = false;
        assert!(pick_tool(&c, &Mock::new(&["ffmpeg"])).unwrap_err().contains("no display"));
    }

    fn fake_output(c: &Ctx, bytes: usize) {
        std::fs::create_dir_all(&c.videos).unwrap();
        std::fs::write(c.videos.join(local_file_name(c.now)), vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn start_stop_flow_with_a_toast_on_stop() {
        let dir = tmp("flow");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg"]));
        let out = execute(&rec_cmd(Switch::On, false), &c, &m).unwrap();
        assert!(out.starts_with("recording to ") && out.ends_with(".mp4"), "{out}");
        let spawned: Vec<_> = m.log().into_iter().filter(|l| l.starts_with("spawn ffmpeg")).collect();
        assert_eq!(spawned.len(), 1);
        assert!(spawned[0].contains("-f x11grab") && spawned[0].contains("-i :9"));
        assert_eq!(execute(&rec_cmd(Switch::Status, false), &c, &m).unwrap(), "on");
        assert!(m.toasts.borrow().is_empty(), "no toast while it starts");

        // starting again is a no-op
        assert!(execute(&rec_cmd(Switch::On, false), &c, &m).unwrap().starts_with("already recording"));
        assert_eq!(m.log().iter().filter(|l| l.starts_with("spawn")).count(), 1);

        fake_output(&c, 1000);
        let out = execute(&rec_cmd(Switch::Off, false), &c, &m).unwrap();
        assert!(out.starts_with("saved "), "{out}");
        assert!(m.log().iter().any(|l| l == &format!("signal 4000 {}", libc::SIGINT)));
        let toasts = m.toasts.borrow();
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts[0].0, "Recording saved");
        assert!(toasts[0].1.ends_with(".mp4"));
        drop(toasts);
        assert_eq!(execute(&rec_cmd(Switch::Status, false), &c, &m).unwrap(), "off");
        // stopping again is a no-op without a second toast
        assert_eq!(execute(&rec_cmd(Switch::Off, false), &c, &m).unwrap(), "not recording");
        assert_eq!(m.toasts.borrow().len(), 1);
    }

    #[test]
    fn toggle_flips_and_quiet_suppresses_the_toast() {
        let dir = tmp("toggle");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg"]));
        execute(&rec_cmd(Switch::Toggle, false), &c, &m).unwrap();
        assert_eq!(execute(&rec_cmd(Switch::Status, false), &c, &m).unwrap(), "on");
        fake_output(&c, 10);
        let quiet = Cmd::Do { feature: Feature::Record, switch: Switch::Toggle, region: false, quiet: true };
        execute(&quiet, &c, &m).unwrap();
        assert_eq!(execute(&rec_cmd(Switch::Status, false), &c, &m).unwrap(), "off");
        assert!(m.toasts.borrow().is_empty());
    }

    #[test]
    fn a_stale_state_file_is_cleaned_up() {
        let dir = tmp("stale");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg"]));
        save(&c, &Rec { pid: 777, file: "/x.mp4".into(), started: 1, tool: "ffmpeg".into() }).unwrap();
        assert!(status(&c, &m).is_none(), "pid 777 is not alive");
        assert!(c.read_state(STATE).is_none());
        // a garbage file counts as not recording
        c.write_state(STATE, "{nope").unwrap();
        assert!(status(&c, &m).is_none());
    }

    #[test]
    fn a_recorder_that_dies_at_once_is_reported() {
        let dir = tmp("dies");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg"]));
        m.dies_at_once.set(true);
        std::fs::create_dir_all(&c.state).unwrap();
        std::fs::write(c.file(LOG), "Cannot open display :9\n").unwrap();
        let err = execute(&rec_cmd(Switch::On, false), &c, &m).unwrap_err();
        assert!(err.contains("Cannot open display"), "{err}");
        assert_eq!(m.toasts.borrow()[0].0, "Recording failed");
        assert!(status(&c, &m).is_none());
    }

    #[test]
    fn stop_escalates_when_the_recorder_ignores_sigint() {
        let dir = tmp("stubborn");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg"]));
        execute(&rec_cmd(Switch::On, false), &c, &m).unwrap();
        m.dies_on_int.set(false);
        fake_output(&c, 10);
        execute(&rec_cmd(Switch::Off, false), &c, &m).unwrap();
        let log = m.log();
        assert!(log.contains(&format!("signal 4000 {}", libc::SIGINT)));
        assert!(log.contains(&format!("signal 4000 {}", libc::SIGTERM)));
    }

    #[test]
    fn stopping_with_no_output_file_says_failed() {
        let dir = tmp("empty");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg"]));
        execute(&rec_cmd(Switch::On, false), &c, &m).unwrap();
        assert!(execute(&rec_cmd(Switch::Off, false), &c, &m).is_err());
        assert_eq!(m.toasts.borrow()[0].0, "Recording failed");
    }

    #[test]
    fn region_uses_slop_and_cancelling_records_nothing() {
        let dir = tmp("region");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg", "slop"]));
        m.out("slop -f %x %y %w %h", "100 200 640 480\n");
        execute(&rec_cmd(Switch::On, true), &c, &m).unwrap();
        let spawn = m.log().into_iter().find(|l| l.starts_with("spawn ffmpeg")).unwrap();
        assert!(spawn.contains("-video_size 640x480") && spawn.contains("-i :9+100,200"), "{spawn}");

        let dir = tmp("region2");
        let (c, m) = (ctx(&dir), Mock::new(&["ffmpeg", "slop"]));
        // slop outputs nothing (Escape): nothing starts, a toast says so
        assert!(execute(&rec_cmd(Switch::On, true), &c, &m).is_err());
        assert!(!m.log().iter().any(|l| l.starts_with("spawn")));
        assert_eq!(m.toasts.borrow()[0].0, "Recording cancelled");
        // without slop the region request fails clearly
        let m = Mock::new(&["ffmpeg"]);
        assert!(execute(&rec_cmd(Switch::On, true), &c, &m).unwrap_err().contains("slop"));
    }
}
