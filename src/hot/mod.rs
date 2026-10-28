//! `dechot`: macOS-style Hot Corners for the X11 session.
//!
//! * this module (no windowing, fully testable): the corners, the actions and the commands they run, the
//!   `hot_corners` section of `decebal.json`, the corner geometry of a monitor layout and the dwell/cooldown state machine.
//! * `daemon` (X11): one tiny InputOnly window per configured corner of every monitor, EnterNotify/LeaveNotify, a
//!   `poll(2)` timeout for the dwell time (nothing runs while idle), config reloads through inotify.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[cfg(feature = "x11")]
pub mod daemon;

/// Name of the daemon's single-instance lock (`$XDG_RUNTIME_DIR/dechot.lock`).
pub const LOCK_NAME: &str = "dechot";
/// The key of `decebal.json` that holds the settings.
pub const SECTION: &str = "hot_corners";
/// Pointer must stay in the corner this long (ms) before it fires, unless `dwell_ms` says otherwise.
pub const DEFAULT_DWELL_MS: u64 = 150;
/// Upper bound of `dwell_ms` (larger values are clamped).
pub const MAX_DWELL_MS: u64 = 5000;
/// After a corner fired and the pointer left it, it stays quiet this long (ms).
pub const COOLDOWN_MS: u64 = 400;
/// Side of a corner window in pixels.
pub const CORNER_PX: i32 = 2;

// ---------------------------------------------------------------- corners

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

    /// The key in `decebal.json`.
    pub fn key(self) -> &'static str {
        match self {
            Corner::TopLeft => "top_left",
            Corner::TopRight => "top_right",
            Corner::BottomLeft => "bottom_left",
            Corner::BottomRight => "bottom_right",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Corner::TopLeft => "Top left",
            Corner::TopRight => "Top right",
            Corner::BottomLeft => "Bottom left",
            Corner::BottomRight => "Bottom right",
        }
    }

    pub fn index(self) -> usize {
        Corner::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    /// `top_left`, `top-left` and `TopLeft` (any case) all name the same corner.
    pub fn parse(s: &str) -> Option<Corner> {
        let norm: String = s.chars().filter(|c| !matches!(c, '-' | '_' | ' ')).flat_map(char::to_lowercase).collect();
        Corner::ALL.into_iter().find(|c| c.key().replace('_', "") == norm)
    }
}

// ---------------------------------------------------------------- actions

/// What a corner does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Action {
    #[default]
    None,
    Dashboard,
    Session,
    Spotlight,
    Launchpad,
    Overview,
    Lock,
    TileAll,
    ShowDesktop,
    Notifications,
    Custom(String),
}

/// The names of the built-in actions, in the order the settings app lists them.
pub const ACTION_NAMES: [&str; 10] = ["none", "dashboard", "session", "spotlight", "launchpad", "overview", "lock", "tile-all", "show-desktop", "notifications"];

/// A shell command line and the program it needs (checked in `PATH` right before it runs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub line: String,
    pub needs: Option<&'static str>,
}

impl Action {
    /// `None` when `s` is not an action. Case and surrounding blanks do not matter, except in a custom command.
    pub fn parse(s: &str) -> Option<Action> {
        let s = s.trim();
        if let Some(rest) = s.get(..7).filter(|p| p.eq_ignore_ascii_case("custom:")).map(|_| &s[7..]) {
            let cmd = rest.trim();
            return (!cmd.is_empty()).then(|| Action::Custom(cmd.to_string()));
        }
        Some(match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "" | "none" | "off" | "-" => Action::None,
            "dashboard" => Action::Dashboard,
            "session" => Action::Session,
            "spotlight" => Action::Spotlight,
            "launchpad" => Action::Launchpad,
            "overview" | "expose" | "mission-control" => Action::Overview,
            "lock" | "lock-screen" => Action::Lock,
            "tile-all" | "tileall" => Action::TileAll,
            "show-desktop" | "desktop" => Action::ShowDesktop,
            "notifications" | "notification-center" => Action::Notifications,
            _ => return Option::None,
        })
    }

    /// The canonical spelling written to `decebal.json`.
    pub fn name(&self) -> String {
        match self {
            Action::None => "none".into(),
            Action::Dashboard => "dashboard".into(),
            Action::Session => "session".into(),
            Action::Spotlight => "spotlight".into(),
            Action::Launchpad => "launchpad".into(),
            Action::Overview => "overview".into(),
            Action::Lock => "lock".into(),
            Action::TileAll => "tile-all".into(),
            Action::ShowDesktop => "show-desktop".into(),
            Action::Notifications => "notifications".into(),
            Action::Custom(c) => format!("custom:{c}"),
        }
    }

    /// The command to run, `None` for actions that do nothing. `show-desktop` is accepted but still does nothing: rdwm has no
    /// "show the desktop" action (viewing an empty tag needs knowledge of the tag layout), so it is skipped rather than guessed.
    pub fn command(&self) -> Option<Command> {
        let (line, needs): (&str, Option<&'static str>) = match self {
            Action::None | Action::ShowDesktop => return Option::None,
            Action::Dashboard => ("decdash", Some("decdash")),
            Action::Session => ("decsession", Some("decsession")),
            Action::Spotlight => ("decspot", Some("decspot")),
            Action::Launchpad => ("declaunch", Some("declaunch")),
            // decexpose may not be installed (yet): without it the corner does nothing.
            Action::Overview => ("decexpose", Some("decexpose")),
            Action::Lock => ("rdwm-lock", Some("rdwm-lock")),
            Action::TileAll => ("rdwmctl action tileall", Some("rdwmctl")),
            Action::Notifications => ("decnotify history", Some("decnotify")),
            Action::Custom(c) => return Some(Command { line: c.clone(), needs: Option::None }),
        };
        Some(Command { line: line.into(), needs })
    }
}

// ---------------------------------------------------------------- configuration

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotConfig {
    /// Indexed by [`Corner::index`].
    pub actions: [Action; 4],
    pub dwell_ms: u64,
}

impl Default for HotConfig {
    fn default() -> Self {
        HotConfig { actions: Default::default(), dwell_ms: DEFAULT_DWELL_MS }
    }
}

impl HotConfig {
    pub fn action(&self, c: Corner) -> &Action {
        &self.actions[c.index()]
    }

    /// Reads the `hot_corners` object of a parsed `decebal.json`. Anything that is not understood (an unknown action,
    /// a wrong type) counts as `none` / the default, so one bad key never disables the others.
    pub fn from_json(root: &Value) -> HotConfig {
        let mut cfg = HotConfig::default();
        let Some(sec) = root.get(SECTION).and_then(Value::as_object) else { return cfg };
        for c in Corner::ALL {
            if let Some(a) = sec.get(c.key()).and_then(Value::as_str).and_then(Action::parse) {
                cfg.actions[c.index()] = a;
            }
        }
        if let Some(ms) = sec.get("dwell_ms").and_then(|v| v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))) {
            cfg.dwell_ms = ms.min(MAX_DWELL_MS);
        }
        cfg
    }

    pub fn parse_text(text: &str) -> HotConfig {
        serde_json::from_str(text).map(|v: Value| HotConfig::from_json(&v)).unwrap_or_default()
    }

    /// Reads `decebal.json` in `dir`; a missing or broken file is the default (all corners off).
    pub fn load_from(dir: &Path) -> HotConfig {
        std::fs::read_to_string(dir.join(CONFIG_FILE)).map(|t| HotConfig::parse_text(&t)).unwrap_or_default()
    }

    pub fn load() -> HotConfig {
        HotConfig::load_from(&crate::config::config_dir())
    }

    /// The object stored under `hot_corners`.
    pub fn to_json(&self) -> Value {
        let mut o = serde_json::Map::new();
        for c in Corner::ALL {
            o.insert(c.key().into(), json!(self.action(c).name()));
        }
        o.insert("dwell_ms".into(), json!(self.dwell_ms));
        Value::Object(o)
    }

    pub fn any_active(&self) -> bool {
        self.actions.iter().any(|a| *a != Action::None)
    }
}

/// File name of the shell settings inside [`crate::config::config_dir`].
pub const CONFIG_FILE: &str = "decebal.json";

pub fn config_path() -> PathBuf {
    crate::config::config_dir().join(CONFIG_FILE)
}

/// Sets one corner (`top_left` ...) to `action`, or `dwell_ms` to a number of milliseconds, in `path`. Other keys of the
/// file and of the `hot_corners` object stay. Returns what was stored.
pub fn set_in(path: &Path, key: &str, value: &str) -> Result<String, String> {
    let existing: Value = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let mut sec = existing.get(SECTION).and_then(Value::as_object).cloned().unwrap_or_default();
    let stored = if key.eq_ignore_ascii_case("dwell_ms") || key.eq_ignore_ascii_case("dwell") {
        let ms: u64 = value.trim().parse().map_err(|_| format!("dwell_ms must be a number of milliseconds, not {value:?}"))?;
        let ms = ms.min(MAX_DWELL_MS);
        sec.insert("dwell_ms".into(), json!(ms));
        ms.to_string()
    } else {
        let corner = Corner::parse(key).ok_or_else(|| format!("unknown corner {key:?} (top-left, top-right, bottom-left, bottom-right, dwell_ms)"))?;
        let action = Action::parse(value).ok_or_else(|| format!("unknown action {value:?} (one of {}, or custom:<shell command>)", ACTION_NAMES.join(", ")))?;
        sec.insert(corner.key().into(), json!(action.name()));
        action.name()
    };
    crate::config::write_setting(path, SECTION, Value::Object(sec)).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(stored)
}

/// `which`: is there an executable called `name` in `PATH`?
pub fn in_path(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| std::fs::metadata(d.join(name)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)))
}

// ---------------------------------------------------------------- geometry

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// One corner window to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub corner: Corner,
    pub monitor: usize,
    pub rect: Rect,
}

/// The corner windows (`size` px square) for a monitor layout, only for the corners in `wanted`.
///
/// A corner that has another monitor right next to it (the pointer just walks over to that monitor there) is not a
/// corner of the desktop and is left out, otherwise crossing between two side-by-side monitors near the top edge would
/// trigger. Monitors that are an exact copy of an earlier one (mirroring) count once.
pub fn corner_spots(monitors: &[Rect], size: i32, wanted: &[Corner]) -> Vec<Spot> {
    let mut mons: Vec<(usize, Rect)> = Vec::new();
    for (i, m) in monitors.iter().enumerate() {
        if m.w > 0 && m.h > 0 && !mons.iter().any(|(_, o)| o == m) {
            mons.push((i, *m));
        }
    }
    let covered = |me: usize, x: i32, y: i32| mons.iter().any(|(i, r)| *i != me && r.contains(x, y));
    let mut out = Vec::new();
    for &(i, m) in &mons {
        let s = size.min(m.w).min(m.h).max(1);
        let (l, t, r, b) = (m.x, m.y, m.x + m.w - 1, m.y + m.h - 1);
        for &c in wanted {
            // The two pixels just outside the monitor, next to the corner pixel.
            let (outside, rect) = match c {
                Corner::TopLeft => ([(l - 1, t), (l, t - 1)], Rect::new(l, t, s, s)),
                Corner::TopRight => ([(r + 1, t), (r, t - 1)], Rect::new(r + 1 - s, t, s, s)),
                Corner::BottomLeft => ([(l - 1, b), (l, b + 1)], Rect::new(l, b + 1 - s, s, s)),
                Corner::BottomRight => ([(r + 1, b), (r, b + 1)], Rect::new(r + 1 - s, b + 1 - s, s, s)),
            };
            if outside.iter().any(|&(x, y)| covered(i, x, y)) {
                continue;
            }
            out.push(Spot { corner: c, monitor: i, rect });
        }
    }
    out
}

// ---------------------------------------------------------------- dwell / cooldown

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The pointer is not in the corner (or was not allowed to start a dwell).
    Out,
    /// Inside since a while: fires at `at` (ms).
    Dwell { at: u64 },
    /// Inside, and done (fired, or refused): nothing more until the pointer leaves.
    Spent,
}

#[derive(Debug, Clone, Copy)]
struct SlotState {
    phase: Phase,
    /// Entering before this time (ms) never starts a dwell.
    quiet_until: u64,
    /// The last time this slot fired (a cooldown starts when the pointer leaves after that).
    fired: bool,
}

/// What the daemon found out about the pointer when a dwell ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Still in the corner, no button down.
    Fire,
    /// A button is held (a drag): give up until the pointer leaves.
    Held,
    /// The pointer is not in the corner any more.
    Gone,
}

/// Dwell and cooldown of a set of corner windows, with time passed in (milliseconds on any monotonic clock).
#[derive(Debug, Clone)]
pub struct Machine {
    dwell: u64,
    cooldown: u64,
    slots: Vec<SlotState>,
}

impl Machine {
    pub fn new(slots: usize, dwell_ms: u64, cooldown_ms: u64) -> Machine {
        Machine { dwell: dwell_ms, cooldown: cooldown_ms, slots: vec![SlotState { phase: Phase::Out, quiet_until: 0, fired: false }; slots] }
    }

    /// The pointer is already in `slot` (its window was just built under it): nothing starts until it leaves and comes back.
    pub fn suppress(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.phase = Phase::Spent;
        }
    }

    pub fn set_dwell(&mut self, dwell_ms: u64) {
        self.dwell = dwell_ms;
    }

    /// The pointer entered `slot`. `button_down`: a button was held then (dragging a window must not trigger).
    pub fn enter(&mut self, slot: usize, now: u64, button_down: bool) {
        let dwell = self.dwell;
        let Some(s) = self.slots.get_mut(slot) else { return };
        if matches!(s.phase, Phase::Dwell { .. } | Phase::Spent) {
            return;
        }
        s.phase = if button_down || now < s.quiet_until { Phase::Spent } else { Phase::Dwell { at: now + dwell } };
    }

    /// The pointer left `slot`: a dwell in progress is cancelled; after a fire the cooldown starts.
    pub fn leave(&mut self, slot: usize, now: u64) {
        let cooldown = self.cooldown;
        let Some(s) = self.slots.get_mut(slot) else { return };
        if s.fired {
            s.quiet_until = now + cooldown;
            s.fired = false;
        }
        s.phase = Phase::Out;
    }

    /// The earliest time (ms) something is due, `None` when nothing is pending (the daemon then sleeps without a timeout).
    pub fn deadline(&self) -> Option<u64> {
        self.slots.iter().filter_map(|s| if let Phase::Dwell { at } = s.phase { Some(at) } else { None }).min()
    }

    /// Slots whose dwell has run out at `now`; `check` looks at the pointer for each one. Returns the ones to fire.
    pub fn poll(&mut self, now: u64, mut check: impl FnMut(usize) -> Check) -> Vec<usize> {
        let mut fire = Vec::new();
        for (i, s) in self.slots.iter_mut().enumerate() {
            let Phase::Dwell { at } = s.phase else { continue };
            if now < at {
                continue;
            }
            match check(i) {
                Check::Fire => {
                    s.phase = Phase::Spent;
                    s.fired = true;
                    fire.push(i);
                }
                Check::Held => s.phase = Phase::Spent,
                Check::Gone => s.phase = Phase::Out,
            }
        }
        fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_names_parse_in_every_spelling() {
        assert_eq!(Corner::parse("top_left"), Some(Corner::TopLeft));
        assert_eq!(Corner::parse("Top-Right"), Some(Corner::TopRight));
        assert_eq!(Corner::parse("bottomleft"), Some(Corner::BottomLeft));
        assert_eq!(Corner::parse("BOTTOM_RIGHT"), Some(Corner::BottomRight));
        assert_eq!(Corner::parse("middle"), None);
        for c in Corner::ALL {
            assert_eq!(Corner::parse(c.key()), Some(c));
            assert_eq!(Corner::ALL[c.index()], c);
        }
    }

    #[test]
    fn config_defaults_to_everything_off() {
        let c = HotConfig::parse_text("");
        assert_eq!(c, HotConfig::default());
        assert!(!c.any_active());
        assert_eq!(c.dwell_ms, 150);
        assert_eq!(HotConfig::parse_text("{\"bar\": {}}"), HotConfig::default());
        assert_eq!(HotConfig::parse_text("not json"), HotConfig::default());
    }

    #[test]
    fn config_reads_the_section() {
        let c = HotConfig::parse_text(r#"{"workspaces": true, "hot_corners": {"top_left": "dashboard", "top_right": "custom:notify-send hi", "bottom_left": "Tile-All", "dwell_ms": 300}}"#);
        assert_eq!(*c.action(Corner::TopLeft), Action::Dashboard);
        assert_eq!(*c.action(Corner::TopRight), Action::Custom("notify-send hi".into()));
        assert_eq!(*c.action(Corner::BottomLeft), Action::TileAll);
        assert_eq!(*c.action(Corner::BottomRight), Action::None);
        assert_eq!(c.dwell_ms, 300);
        assert!(c.any_active());
    }

    #[test]
    fn bad_values_only_spoil_themselves() {
        let c = HotConfig::parse_text(r#"{"hot_corners": {"top_left": "teleport", "top_right": 5, "bottom_left": "lock", "dwell_ms": "soon"}}"#);
        assert_eq!(*c.action(Corner::TopLeft), Action::None);
        assert_eq!(*c.action(Corner::TopRight), Action::None);
        assert_eq!(*c.action(Corner::BottomLeft), Action::Lock);
        assert_eq!(c.dwell_ms, 150);
        assert_eq!(HotConfig::parse_text(r#"{"hot_corners": {"dwell_ms": 999999}}"#).dwell_ms, MAX_DWELL_MS);
        assert_eq!(HotConfig::parse_text(r#"{"hot_corners": {"dwell_ms": 0}}"#).dwell_ms, 0);
        assert_eq!(HotConfig::parse_text(r#"{"hot_corners": "all"}"#), HotConfig::default());
    }

    #[test]
    fn actions_resolve_to_commands() {
        let line = |a: &str| Action::parse(a).and_then(|a| a.command()).map(|c| (c.line, c.needs));
        assert_eq!(line("dashboard"), Some(("decdash".into(), Some("decdash"))));
        assert_eq!(line("session"), Some(("decsession".into(), Some("decsession"))));
        assert_eq!(line("spotlight"), Some(("decspot".into(), Some("decspot"))));
        assert_eq!(line("launchpad"), Some(("declaunch".into(), Some("declaunch"))));
        assert_eq!(line("overview"), Some(("decexpose".into(), Some("decexpose"))));
        assert_eq!(line("lock"), Some(("rdwm-lock".into(), Some("rdwm-lock"))));
        assert_eq!(line("tile-all"), Some(("rdwmctl action tileall".into(), Some("rdwmctl"))));
        assert_eq!(line("notifications"), Some(("decnotify history".into(), Some("decnotify"))));
        assert_eq!(line("custom:  touch /tmp/x && echo 'a b' "), Some(("touch /tmp/x && echo 'a b'".into(), None)));
        assert_eq!(line("none"), None);
        assert_eq!(line("show-desktop"), None, "accepted, but rdwm has nothing to run for it");
        assert_eq!(Action::parse("show-desktop"), Some(Action::ShowDesktop));
    }

    #[test]
    fn action_parsing_is_forgiving_but_not_blind() {
        assert_eq!(Action::parse("  Spotlight "), Some(Action::Spotlight));
        assert_eq!(Action::parse("tile_all"), Some(Action::TileAll));
        assert_eq!(Action::parse(""), Some(Action::None));
        assert_eq!(Action::parse("custom:"), None);
        assert_eq!(Action::parse("custom:   "), None);
        assert_eq!(Action::parse("rm -rf /"), None);
        assert_eq!(Action::parse("CUSTOM:Echo Hi"), Some(Action::Custom("Echo Hi".into())), "case of a command is kept");
        // names written by name() parse back
        for n in ACTION_NAMES {
            assert_eq!(Action::parse(n).map(|a| a.name()).as_deref(), Some(n));
        }
        assert_eq!(Action::parse("custom:a b").map(|a| a.name()).as_deref(), Some("custom:a b"));
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dechot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn set_writes_one_key_and_keeps_the_rest() {
        let d = tmp("set");
        let p = d.join("decebal.json");
        std::fs::write(&p, r#"{"workspaces": "auto", "hot_corners": {"top_left": "lock"}}"#).unwrap();
        assert_eq!(set_in(&p, "top-right", "Spotlight").unwrap(), "spotlight");
        assert_eq!(set_in(&p, "dwell_ms", "250").unwrap(), "250");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["workspaces"], "auto");
        assert_eq!(v["hot_corners"]["top_left"], "lock");
        assert_eq!(v["hot_corners"]["top_right"], "spotlight");
        assert_eq!(v["hot_corners"]["dwell_ms"], 250);
        let c = HotConfig::from_json(&v);
        assert_eq!(*c.action(Corner::TopRight), Action::Spotlight);
        assert_eq!(c.dwell_ms, 250);
        assert!(set_in(&p, "centre", "lock").is_err());
        assert!(set_in(&p, "top_left", "explode").is_err());
        assert!(set_in(&p, "dwell_ms", "fast").is_err());
        assert!(set_in(&p, "top_left", "custom:").is_err());
        // a missing file is created
        let q = d.join("new/decebal.json");
        set_in(&q, "bottom_left", "custom:true").unwrap();
        assert_eq!(HotConfig::load_from(q.parent().unwrap()).actions[Corner::BottomLeft.index()], Action::Custom("true".into()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn to_json_round_trips() {
        let mut c = HotConfig::default();
        c.actions[Corner::BottomRight.index()] = Action::Custom("a b".into());
        c.actions[Corner::TopLeft.index()] = Action::Overview;
        c.dwell_ms = 90;
        assert_eq!(HotConfig::from_json(&json!({ SECTION: c.to_json() })), c);
    }

    const ALL: [Corner; 4] = Corner::ALL;

    #[test]
    fn one_monitor_has_four_corners() {
        let s = corner_spots(&[Rect::new(0, 0, 1920, 1080)], 2, &ALL);
        let r = |c: Corner| s.iter().find(|x| x.corner == c).map(|x| x.rect);
        assert_eq!(r(Corner::TopLeft), Some(Rect::new(0, 0, 2, 2)));
        assert_eq!(r(Corner::TopRight), Some(Rect::new(1918, 0, 2, 2)));
        assert_eq!(r(Corner::BottomLeft), Some(Rect::new(0, 1078, 2, 2)));
        assert_eq!(r(Corner::BottomRight), Some(Rect::new(1918, 1078, 2, 2)));
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn only_wanted_corners_get_a_window() {
        let s = corner_spots(&[Rect::new(0, 0, 100, 100)], 2, &[Corner::TopRight]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].corner, Corner::TopRight);
        assert!(corner_spots(&[Rect::new(0, 0, 100, 100)], 2, &[]).is_empty());
    }

    #[test]
    fn side_by_side_monitors_lose_the_seam_corners() {
        let m = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 2560, 1440)];
        let s = corner_spots(&m, 2, &ALL);
        let has = |mon: usize, c: Corner| s.iter().any(|x| x.monitor == mon && x.corner == c);
        assert!(has(0, Corner::TopLeft) && has(0, Corner::BottomLeft));
        assert!(!has(0, Corner::TopRight), "the seam is not a corner");
        assert!(!has(0, Corner::BottomRight), "the seam is not a corner (the right monitor is taller)");
        assert!(!has(1, Corner::TopLeft));
        assert!(has(1, Corner::TopRight) && has(1, Corner::BottomRight) && has(1, Corner::BottomLeft), "the lower-left of the taller monitor is a dead end");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn stacked_monitors_lose_the_seam_corners() {
        let m = [Rect::new(0, 0, 1000, 500), Rect::new(0, 500, 1000, 500)];
        let s = corner_spots(&m, 2, &ALL);
        assert_eq!(s.len(), 4);
        assert!(s.iter().all(|x| (x.monitor == 0) == matches!(x.corner, Corner::TopLeft | Corner::TopRight)));
    }

    #[test]
    fn mirrored_and_empty_monitors_count_once_or_never() {
        let m = [Rect::new(0, 0, 800, 600), Rect::new(0, 0, 800, 600), Rect::new(0, 0, 0, 0)];
        assert_eq!(corner_spots(&m, 2, &ALL).len(), 4);
    }

    #[test]
    fn a_tiny_monitor_shrinks_the_window() {
        let s = corner_spots(&[Rect::new(10, 20, 1, 1)], 2, &[Corner::BottomRight]);
        assert_eq!(s[0].rect, Rect::new(10, 20, 1, 1));
    }

    fn always(c: Check) -> impl FnMut(usize) -> Check {
        move |_| c
    }

    #[test]
    fn fires_once_after_the_dwell() {
        let mut m = Machine::new(2, 150, 400);
        m.enter(0, 1000, false);
        assert_eq!(m.deadline(), Some(1150));
        assert!(m.poll(1100, always(Check::Fire)).is_empty(), "too early");
        assert_eq!(m.poll(1150, always(Check::Fire)), vec![0]);
        assert_eq!(m.deadline(), None);
        assert!(m.poll(5000, always(Check::Fire)).is_empty(), "once per visit");
    }

    #[test]
    fn a_quick_pass_through_does_not_fire() {
        let mut m = Machine::new(1, 150, 400);
        m.enter(0, 0, false);
        m.leave(0, 100);
        assert_eq!(m.deadline(), None);
        assert!(m.poll(1000, always(Check::Fire)).is_empty());
        // and it is armed again at once: no cooldown after a pass-through
        m.enter(0, 110, false);
        assert_eq!(m.poll(260, always(Check::Fire)), vec![0]);
    }

    #[test]
    fn a_button_held_on_entry_never_fires() {
        let mut m = Machine::new(1, 150, 400);
        m.enter(0, 0, true);
        assert_eq!(m.deadline(), None);
        assert!(m.poll(10_000, always(Check::Fire)).is_empty());
        m.leave(0, 10_000);
        m.enter(0, 10_001, false);
        assert_eq!(m.poll(10_200, always(Check::Fire)), vec![0], "released and came back");
    }

    #[test]
    fn a_button_pressed_during_the_dwell_cancels_it() {
        let mut m = Machine::new(1, 150, 400);
        m.enter(0, 0, false);
        assert!(m.poll(150, always(Check::Held)).is_empty());
        assert!(m.poll(10_000, always(Check::Fire)).is_empty(), "stays spent until it leaves");
        m.leave(0, 10_000);
        m.enter(0, 10_001, false);
        assert_eq!(m.poll(10_151, always(Check::Fire)), vec![0], "a refused dwell starts no cooldown");
    }

    #[test]
    fn a_pointer_that_is_gone_resets_the_slot() {
        let mut m = Machine::new(1, 100, 400);
        m.enter(0, 0, false);
        assert!(m.poll(100, always(Check::Gone)).is_empty());
        m.enter(0, 101, false);
        assert_eq!(m.poll(201, always(Check::Fire)), vec![0]);
    }

    #[test]
    fn needs_a_leave_and_a_cooldown_to_fire_again() {
        let mut m = Machine::new(1, 150, 400);
        m.enter(0, 0, false);
        assert_eq!(m.poll(150, always(Check::Fire)), vec![0]);
        m.enter(0, 200, false); // already inside: ignored
        assert!(m.poll(5000, always(Check::Fire)).is_empty(), "staying does not repeat");
        m.leave(0, 5000);
        m.enter(0, 5200, false); // back within the cooldown
        assert_eq!(m.deadline(), None, "ignored while cooling down");
        assert!(m.poll(9000, always(Check::Fire)).is_empty());
        m.leave(0, 9000);
        m.enter(0, 9500, false); // 500 ms after the last leave: cool again
        assert_eq!(m.poll(9650, always(Check::Fire)), vec![0]);
    }

    #[test]
    fn slots_are_independent_and_the_deadline_is_the_earliest() {
        let mut m = Machine::new(3, 100, 400);
        m.enter(1, 50, false);
        m.enter(2, 20, false);
        assert_eq!(m.deadline(), Some(120));
        assert_eq!(m.poll(130, always(Check::Fire)), vec![2]);
        assert_eq!(m.deadline(), Some(150));
        m.leave(1, 140);
        assert_eq!(m.deadline(), None);
        m.enter(7, 0, false); // unknown slot: ignored
        m.leave(7, 0);
    }

    #[test]
    fn a_suppressed_slot_waits_for_a_leave_and_has_no_cooldown() {
        let mut m = Machine::new(1, 100, 400);
        m.suppress(0);
        m.enter(0, 0, false); // the EnterNotify of the freshly mapped window
        assert_eq!(m.deadline(), None);
        m.leave(0, 10);
        m.enter(0, 20, false);
        assert_eq!(m.poll(120, always(Check::Fire)), vec![0]);
    }

    #[test]
    fn zero_dwell_fires_on_the_next_poll() {
        let mut m = Machine::new(1, 0, 400);
        m.enter(0, 10, false);
        assert_eq!(m.deadline(), Some(10));
        assert_eq!(m.poll(10, always(Check::Fire)), vec![0]);
    }
}
