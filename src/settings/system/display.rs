//! Displays: resolution, refresh rate, rotation, position, primary, mirroring via `xrandr`, with a
//! 15 second "Keep changes?" confirmation that reverts by itself; scale (Xft.dpi + rdwm `[decoration] scale`);
//! night shift (gammastep/redshift) when installed.

pub mod wayland;

use super::confirm::{self, CONFIRM_SECS};
use super::run::{Cmd, Runner};
use super::{Action, Dom, Done, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Opt, Row};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rot {
    #[default]
    Normal,
    Left,
    Inverted,
    Right,
}

impl Rot {
    pub fn arg(self) -> &'static str {
        match self {
            Rot::Normal => "normal",
            Rot::Left => "left",
            Rot::Inverted => "inverted",
            Rot::Right => "right",
        }
    }
    pub fn parse(s: &str) -> Option<Rot> {
        Some(match s {
            "normal" => Rot::Normal,
            "left" => Rot::Left,
            "inverted" => Rot::Inverted,
            "right" => Rot::Right,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Rot::Normal => "Standard",
            Rot::Left => "90° counter-clockwise",
            Rot::Inverted => "180°",
            Rot::Right => "90° clockwise",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    pub w: u32,
    pub h: u32,
    pub rates: Vec<f32>,
    /// Rate marked `*`.
    pub current: Option<f32>,
    /// Rate marked `+`.
    pub preferred: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub name: String,
    pub connected: bool,
    pub primary: bool,
    /// Active geometry after rotation: (w, h, x, y).
    pub geom: Option<(u32, u32, i32, i32)>,
    pub rot: Rot,
    pub modes: Vec<Mode>,
}

impl Output {
    pub fn active(&self) -> bool {
        self.geom.is_some()
    }
    pub fn current_mode(&self) -> Option<(&Mode, f32)> {
        self.modes.iter().find_map(|m| m.current.map(|r| (m, r)))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    pub wayland: Option<wayland::Snapshot>,
    /// Worker request start, including failed Wayland queries (never X11).
    pub wayland_query_started: Option<Instant>,
    pub available: bool,
    pub error: Option<String>,
    pub outputs: Vec<Output>,
    pub night_tool: Option<String>,
}

impl State {
    pub fn connected(&self) -> Vec<&Output> {
        self.outputs.iter().filter(|o| o.connected).collect()
    }
    pub fn active(&self) -> Vec<&Output> {
        self.outputs.iter().filter(|o| o.connected && o.active()).collect()
    }
    fn out(&self, n: &str) -> Option<&Output> {
        self.outputs.iter().find(|o| o.name == n && o.connected)
    }
}

// ---------------------------------------------------------------- parser

fn parse_geom(tok: &str) -> Option<(u32, u32, i32, i32)> {
    // 1920x1080+0+0 (offsets may be negative: +-1920+0 is printed as -1920+0)
    let (size, rest) = tok.split_once(['+', '-']).map(|(s, _)| (s, &tok[s.len()..]))?;
    let (w, h) = size.split_once('x')?;
    let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
    let r = rest;
    let split = r[1..].find(['+', '-'])? + 1;
    let (xs, ys) = (&r[..split], &r[split..]);
    let num = |s: &str| s.replace("+-", "-").parse::<i32>().ok();
    Some((w, h, num(xs)?, num(ys)?))
}

pub fn parse_xrandr(text: &str) -> Vec<Output> {
    let mut outs: Vec<Output> = Vec::new();
    for line in text.lines() {
        if line.starts_with("Screen ") {
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('\t') {
            let mut it = line.split_whitespace();
            let (Some(name), Some(state)) = (it.next(), it.next()) else { continue };
            let toks: Vec<&str> = it.collect();
            let connected = state == "connected";
            let primary = toks.first() == Some(&"primary");
            let geom = toks.iter().find(|t| t.contains('x') && t.chars().next().is_some_and(|c| c.is_ascii_digit()) && (t.contains('+') || t.contains('-'))).and_then(|t| parse_geom(t));
            let rot = toks.iter().take_while(|t| !t.starts_with('(')).find_map(|t| Rot::parse(t)).unwrap_or_default();
            outs.push(Output { name: name.into(), connected, primary, geom: geom.filter(|_| connected), rot, modes: vec![] });
            continue;
        }
        let Some(o) = outs.last_mut() else { continue };
        let mut it = line.split_whitespace();
        let Some(first) = it.next() else { continue };
        let Some((w, h)) = first.trim_end_matches('i').split_once('x') else { continue };
        let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) else { continue };
        let mut rates: Vec<f32> = Vec::new();
        let (mut current, mut preferred) = (None, None);
        for t in it {
            let flags_only = t.chars().all(|c| c == '*' || c == '+');
            if flags_only {
                if let Some(&last) = rates.last() {
                    if t.contains('*') {
                        current = Some(last);
                    }
                    if t.contains('+') {
                        preferred = Some(last);
                    }
                }
                continue;
            }
            let num: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let Ok(r) = num.parse::<f32>() else { continue };
            rates.push(r);
            if t.contains('*') {
                current = Some(r);
            }
            if t.contains('+') {
                preferred = Some(r);
            }
        }
        if let Some(m) = o.modes.iter_mut().find(|m| m.w == w && m.h == h) {
            // interlaced duplicate of a progressive mode: merge rates
            for r in rates {
                if !m.rates.contains(&r) {
                    m.rates.push(r);
                }
            }
            m.current = m.current.or(current);
            m.preferred = m.preferred.or(preferred);
        } else {
            o.modes.push(Mode { w, h, rates, current, preferred });
        }
    }
    for o in outs.iter_mut() {
        o.modes.sort_by(|a, b| (b.w * b.h).cmp(&(a.w * a.h)).then(b.w.cmp(&a.w)));
        for m in o.modes.iter_mut() {
            m.rates.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        }
    }
    outs
}

/// Physical outputs follow the session compositor, even if this window uses XWayland.
pub fn physical_wayland_session() -> bool {
    physical_wayland_env(|key| std::env::var_os(key))
}
fn physical_wayland_env(mut env: impl FnMut(&str) -> Option<std::ffi::OsString>) -> bool {
    env("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) || env("XDG_SESSION_TYPE").is_some_and(|v| v == "wayland")
}
pub fn load(r: &dyn Runner) -> State {
    load_env(r, physical_wayland_session())
}

pub fn load_env(r: &dyn Runner, wayland: bool) -> State {
    if wayland {
        return wayland::load(r);
    }
    let night = ["gammastep", "redshift"].iter().find(|t| r.exists(t)).map(|t| t.to_string());
    if !r.exists("xrandr") {
        return State { error: Some("xrandr is not installed.".into()), night_tool: night, ..State::default() };
    }
    let o = r.run(&Cmd::new("xrandr", &["--query"]).timeout(6));
    if !o.ok {
        return State { error: Some(format!("xrandr could not talk to the X server: {}", o.error_text("xrandr"))), night_tool: night, ..State::default() };
    }
    State { wayland: None, wayland_query_started: None, available: true, error: None, outputs: parse_xrandr(&o.stdout), night_tool: night }
}

// ---------------------------------------------------------------- planning

#[derive(Debug, Clone, PartialEq)]
pub enum Rel {
    RightOf,
    LeftOf,
    Above,
    Below,
    Same,
}

impl Rel {
    pub fn flag(&self) -> &'static str {
        match self {
            Rel::RightOf => "--right-of",
            Rel::LeftOf => "--left-of",
            Rel::Above => "--above",
            Rel::Below => "--below",
            Rel::Same => "--same-as",
        }
    }
    pub fn parse(s: &str) -> Option<(Rel, &str)> {
        let (k, o) = s.split_once('|')?;
        Some((
            match k {
                "right" => Rel::RightOf,
                "left" => Rel::LeftOf,
                "above" => Rel::Above,
                "below" => Rel::Below,
                "same" => Rel::Same,
                _ => return None,
            },
            o,
        ))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Mode(u32, u32),
    Rate(f32),
    Rotate(Rot),
    Place(Rel, String),
    Primary,
    Enable(bool),
    Mirror(bool),
}

fn rate_arg(r: f32) -> String {
    format!("{r:.2}")
}

fn valid_output_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 40 && !n.starts_with('-') && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// xrandr arguments that put every output back exactly as `st` shows it (used to revert and to persist).
pub fn layout_args(st: &State) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    for o in st.outputs.iter().filter(|o| o.connected) {
        a.push("--output".into());
        a.push(o.name.clone());
        match (o.geom, o.current_mode()) {
            (Some((_, _, x, y)), Some((m, r))) => {
                a.extend(["--mode".into(), format!("{}x{}", m.w, m.h)]);
                if r > 0.5 {
                    a.extend(["--rate".into(), rate_arg(r)]);
                }
                a.extend(["--pos".into(), format!("{x}x{y}"), "--rotate".into(), o.rot.arg().into()]);
                if o.primary {
                    a.push("--primary".into());
                }
            }
            (Some((_, _, x, y)), None) => {
                a.extend(["--auto".into(), "--pos".into(), format!("{x}x{y}"), "--rotate".into(), o.rot.arg().into()]);
                if o.primary {
                    a.push("--primary".into());
                }
            }
            _ => a.push("--off".into()),
        }
    }
    a
}

fn xr(args: Vec<String>) -> Cmd {
    Cmd::owned("xrandr", args).timeout(12)
}

/// (apply, revert). Errors are user-facing sentences.
pub fn plan(st: &State, name: &str, ch: &Change) -> Result<(Vec<Cmd>, Vec<Cmd>), String> {
    if !st.available {
        return Err("Display control is not available".into());
    }
    let revert = vec![xr(layout_args(st))];
    let o = st.out(name).filter(|_| valid_output_name(name)).ok_or_else(|| format!("{name} is not connected"))?;
    let base = |n: &str| vec!["--output".to_string(), n.to_string()];
    let apply = match ch {
        Change::Mode(w, h) => {
            let m = o.modes.iter().find(|m| m.w == *w && m.h == *h).ok_or_else(|| format!("{name} does not support {w}x{h}"))?;
            let keep = o.current_mode().map(|(_, r)| r).filter(|r| m.rates.iter().any(|x| (x - r).abs() < 0.01));
            let rate = keep.or(m.preferred).or_else(|| m.rates.first().copied());
            let mut a = base(name);
            a.extend(["--mode".into(), format!("{w}x{h}")]);
            if let Some(r) = rate.filter(|r| *r > 0.5) {
                a.extend(["--rate".into(), rate_arg(r)]);
            }
            vec![xr(a)]
        }
        Change::Rate(r) => {
            let (m, _) = o.current_mode().ok_or("This display is off")?;
            if !m.rates.iter().any(|x| (x - r).abs() < 0.01) {
                return Err(format!("{name} does not support {r:.2} Hz at {}x{}", m.w, m.h));
            }
            let mut a = base(name);
            a.extend(["--mode".into(), format!("{}x{}", m.w, m.h), "--rate".into(), rate_arg(*r)]);
            vec![xr(a)]
        }
        Change::Rotate(rot) => {
            if !o.active() {
                return Err("This display is off".into());
            }
            let mut a = base(name);
            a.extend(["--rotate".into(), rot.arg().into()]);
            vec![xr(a)]
        }
        Change::Place(rel, other) => {
            if other == name {
                return Err("Choose a different display".into());
            }
            let p = st.out(other).filter(|p| p.active() && valid_output_name(other)).ok_or_else(|| format!("{other} is not active"))?;
            if !o.active() {
                return Err("Turn this display on first".into());
            }
            let mut a = base(name);
            a.extend([rel.flag().into(), p.name.clone()]);
            vec![xr(a)]
        }
        Change::Primary => {
            if !o.active() {
                return Err("Turn this display on first".into());
            }
            let mut a = base(name);
            a.push("--primary".into());
            vec![xr(a)]
        }
        Change::Enable(true) => {
            let mut a = base(name);
            a.push("--auto".into());
            if let Some(last) = st.active().last() {
                a.extend(["--right-of".into(), last.name.clone()]);
            }
            vec![xr(a)]
        }
        Change::Enable(false) => {
            if st.active().len() <= 1 {
                return Err("At least one display must stay on".into());
            }
            let mut a = base(name);
            a.push("--off".into());
            if o.primary {
                // keep a primary somewhere
                if let Some(n) = st.active().iter().find(|x| x.name != name) {
                    let mut p = base(&n.name);
                    p.push("--primary".into());
                    a.extend(p);
                }
            }
            vec![xr(a)]
        }
        Change::Mirror(true) => {
            let act = st.active();
            if act.len() < 2 {
                return Err("Mirroring needs two active displays".into());
            }
            let main = act.iter().find(|o| o.primary).unwrap_or(&act[0]);
            // largest resolution every active display supports
            let common = main.modes.iter().find(|m| act.iter().all(|a| a.modes.iter().any(|x| x.w == m.w && x.h == m.h))).ok_or("These displays share no resolution, so they cannot be mirrored")?;
            let mut a = Vec::new();
            for d in &act {
                a.extend(["--output".into(), d.name.clone(), "--mode".into(), format!("{}x{}", common.w, common.h)]);
                if d.name == main.name {
                    a.extend(["--pos".into(), "0x0".into(), "--rotate".into(), "normal".into()]);
                } else {
                    a.extend(["--same-as".into(), main.name.clone(), "--rotate".into(), "normal".into()]);
                }
            }
            vec![xr(a)]
        }
        Change::Mirror(false) => {
            let act = st.active();
            if act.len() < 2 {
                return Err("Extending needs two active displays".into());
            }
            let mut a = Vec::new();
            for (i, d) in act.iter().enumerate() {
                a.extend(["--output".into(), d.name.clone()]);
                if i == 0 {
                    a.extend(["--pos".into(), "0x0".into()]);
                } else {
                    a.extend(["--right-of".into(), act[i - 1].name.clone()]);
                }
            }
            vec![xr(a)]
        }
    };
    Ok((apply, revert))
}

/// Are the active displays currently mirrored (same origin)?
pub fn mirrored(st: &State) -> bool {
    let a = st.active();
    a.len() > 1 && a.iter().all(|o| o.geom.map(|g| (g.2, g.3)) == a[0].geom.map(|g| (g.2, g.3)))
}

pub fn scale_options() -> Vec<Opt> {
    let mut v = vec![("Automatic".to_string(), "auto".to_string())];
    for p in [100, 125, 150, 175, 200, 250, 300] {
        v.push((format!("{p}%"), p.to_string()));
    }
    v
}

pub fn night_cmd(tool: &str, on: bool, kelvin: u32) -> Result<Vec<Cmd>, String> {
    if tool != "gammastep" && tool != "redshift" {
        return Err("Unsupported night shift tool".into());
    }
    if !(1000..=10000).contains(&kelvin) {
        return Err("Colour temperature must be between 1000 and 10000 K".into());
    }
    Ok(if on { vec![Cmd::new(tool, &["-P", "-O", &kelvin.to_string()])] } else { vec![Cmd::new(tool, &["-x"])] })
}

pub fn dpi_cmd(dpi: u32) -> Result<Cmd, String> {
    if !(48..=480).contains(&dpi) {
        return Err("DPI must be between 48 and 480".into());
    }
    Ok(Cmd::owned("sh", vec!["-c".into(), "printf 'Xft.dpi: %s\\n' \"$1\" | xrdb -merge".into(), "sh".into(), dpi.to_string()]))
}

// ---------------------------------------------------------------- Hub glue (confirmation + watchdog)

fn flag_path(dir: &std::path::Path) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(|| dir.to_path_buf());
    let _ = std::fs::create_dir_all(&base);
    base.join(format!("decsettings-keep-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

impl Hub {
    /// Apply a display change and start the 15 s confirmation.
    pub fn display_change(&mut self, name: &str, ch: Change) {
        let Some(st) = self.disp.data.clone() else { return };
        match plan(&st, name, &ch) {
            Ok((apply, revert)) => self.display_apply(apply, revert, "Display"),
            Err(e) => self.say(e, true),
        }
    }

    pub fn display_apply(&mut self, apply: Vec<Cmd>, revert: Vec<Cmd>, label: &str) {
        let secs = confirm_secs();
        let already = self.sh.confirm.lock().unwrap().active();
        let flag = flag_path(&self.sh.dir);
        let _ = std::fs::remove_file(&flag);
        {
            let mut c = self.sh.confirm.lock().unwrap();
            c.begin(Instant::now(), secs, revert.clone(), vec![], Some(flag.clone()));
        }
        if !already {
            // backstop that works even if this window dies: after the countdown the old layout comes back
            // unless the flag file shows the change was kept
            if let Some(r) = revert.first() {
                let mut args = vec!["-c".to_string(), r#"sleep "$1"; f="$2"; shift 2; [ -e "$f" ] || xrandr "$@"; rm -f "$f""#.into(), "sh".into(), (secs + 3).to_string(), flag.display().to_string()];
                args.extend(r.args.clone());
                self.sh.runner.spawn(&Cmd::owned("sh", args));
            }
        }
        let mut a = Action::new(Dom::Display, label, apply);
        a.refresh = true;
        self.run(a);
        self.sh.wake_poller();
        self.dirty = true;
    }

    pub fn display_keep(&mut self) {
        if let Some(p) = self.sh.confirm.lock().unwrap().keep() {
            if let Some(f) = &p.flag {
                let _ = std::fs::write(f, b"kept");
            }
            // persist the layout as it is now
            let dir = self.sh.dir.clone();
            let before = self.session.clone();
            self.sh.spawn(move |r| {
                let st = load(r);
                let mut d = Done { refresh: Some(Dom::Display), ..Done::default() };
                if st.available {
                    let mut s = before;
                    s.xrandr = layout_args(&st);
                    match s.save(&dir) {
                        Ok(()) => {
                            d.session = Some(s);
                            d.msg = Some(("Display settings kept".into(), false));
                        }
                        Err(e) => d.msg = Some((format!("Could not save display layout for next login: {e}"), true)),
                    }
                }
                d
            });
        }
        self.dirty = true;
    }

    pub fn display_revert_now(&mut self) {
        let p = self.sh.confirm.lock().unwrap().revert();
        if let Some(p) = p {
            display_revert(&self.sh, p);
        }
        self.dirty = true;
    }
}

pub fn confirm_secs() -> u64 {
    std::env::var("DECSETTINGS_CONFIRM_SECS").ok().and_then(|v| v.parse().ok()).filter(|v| (1..=120).contains(v)).unwrap_or(CONFIRM_SECS)
}

pub fn display_revert(sh: &Arc<super::Shared>, p: confirm::Pending) {
    let sh2 = sh.clone();
    sh.spawn(move |r| {
        if let Some(f) = &p.flag {
            let _ = std::fs::write(f, b"reverted");
        }
        let mut d = Done { refresh: Some(Dom::Display), ..Done::default() };
        for c in &p.revert {
            let o = r.run(c);
            if !o.ok {
                d.msg = Some((format!("Could not restore the previous display settings: {}", o.error_text("xrandr")), true));
                let _ = &sh2;
                return d;
            }
        }
        d.msg = Some(("Display settings reverted".into(), false));
        d
    });
}

// ---------------------------------------------------------------- pane

fn mode_label(m: &Mode) -> String {
    format!("{} × {}", m.w, m.h)
}

pub fn items(h: &Hub, scale_now: Option<f64>) -> Vec<Item> {
    if let Some(s) = h.disp.data.as_ref().and_then(|s| s.wayland.as_ref()) {
        return wayland::items(h, s);
    }
    if physical_wayland_session() {
        return super::sound::unavailable("Wayland display settings", h.disp.data.as_ref().and_then(|s| s.error.as_deref()).unwrap_or("Reading compositor display capabilities…"));
    }
    let mut v = Vec::new();
    // confirmation banner
    if let Some(left) = h.sh.confirm.lock().unwrap().seconds_left(Instant::now()) {
        v.push(Item::Header("Keep these display settings?".into()));
        v.push(Item::Card(vec![Row::new(format!("Reverting in {left} s"))
            .detail("The previous layout comes back automatically unless you keep this one")
            .ctl(Ctl::new("s:disp:revert:", Kind::Button { label: "Revert".into(), danger: true }))
            .ctl(Ctl::new("s:disp:keep:", Kind::Button { label: "Keep changes".into(), danger: false }))]));
    }
    match &h.disp.data {
        None => v.push(Item::Note { text: "Reading display layout...".into(), warn: false }),
        Some(st) if !st.available => {
            v.extend(super::sound::unavailable("Display settings", st.error.as_deref().unwrap_or("xrandr is not available (Wayland sessions configure displays in the compositor).")))
        }
        Some(st) => {
            let conn = st.connected();
            let multi = conn.len() > 1;
            if multi {
                v.push(Item::Header("Arrangement".into()));
                v.push(Item::Card(vec![Row::new("Mirror displays").detail("Show the same picture on every display").ctl(Ctl::new("s:disp:mirror:", Kind::Toggle(mirrored(st))))]));
            }
            for o in conn {
                let title = format!("{}{}", o.name, if o.primary { " (primary)" } else { "" });
                v.push(Item::Header(title));
                let mut rows = Vec::new();
                if multi {
                    rows.push(Row::new("Enabled").ctl(Ctl::new(format!("s:disp:enable:{}", o.name), Kind::Toggle(o.active()))));
                }
                if o.active() {
                    let cur = o.current_mode();
                    let opts: Vec<Opt> = o.modes.iter().map(|m| (mode_label(m), format!("{}x{}", m.w, m.h))).collect();
                    let sel = cur.and_then(|(c, _)| o.modes.iter().position(|m| m.w == c.w && m.h == c.h));
                    rows.push(Row::new("Resolution").ctl(Ctl::new(format!("s:disp:mode:{}", o.name), Kind::Popup(opts, sel))));
                    if let Some((m, r)) = cur {
                        let rates: Vec<Opt> = m.rates.iter().filter(|r| **r > 0.5).map(|r| (format!("{r:.2} Hz"), rate_arg(*r))).collect();
                        if !rates.is_empty() {
                            let sel = m.rates.iter().filter(|r| **r > 0.5).position(|x| (x - r).abs() < 0.01);
                            rows.push(Row::new("Refresh rate").ctl(Ctl::new(format!("s:disp:rate:{}", o.name), Kind::Popup(rates, sel))));
                        }
                    }
                    let rots: Vec<Opt> = [Rot::Normal, Rot::Left, Rot::Inverted, Rot::Right].iter().map(|r| (r.label().to_string(), r.arg().to_string())).collect();
                    let sel = [Rot::Normal, Rot::Left, Rot::Inverted, Rot::Right].iter().position(|r| *r == o.rot);
                    rows.push(Row::new("Rotation").ctl(Ctl::new(format!("s:disp:rot:{}", o.name), Kind::Popup(rots, sel))));
                    if multi {
                        let mut pos: Vec<Opt> = Vec::new();
                        for other in st.active().iter().filter(|x| x.name != o.name) {
                            for (k, l) in [("right", "Right of"), ("left", "Left of"), ("above", "Above"), ("below", "Below"), ("same", "Mirror of")] {
                                pos.push((format!("{l} {}", other.name), format!("{k}|{}", other.name)));
                            }
                        }
                        let (x, y) = o.geom.map(|g| (g.2, g.3)).unwrap_or((0, 0));
                        rows.push(Row::new("Position").detail(format!("Now at {x}, {y}")).ctl(Ctl::new(format!("s:disp:pos:{}", o.name), Kind::Popup(pos, None))));
                    }
                    if !o.primary {
                        rows.push(
                            Row::new("Make primary")
                                .detail("Where the menu bar and dock go")
                                .ctl(Ctl::new(format!("s:disp:primary:{}", o.name), Kind::Button { label: "Make primary".into(), danger: false })),
                        );
                    }
                } else {
                    rows.push(Row::new("This display is off"));
                }
                v.push(Item::Card(rows));
            }
            if st.connected().is_empty() {
                v.push(Item::Card(vec![Row::new("No connected displays reported")]));
            }
        }
    }
    // scale
    v.push(Item::Header("Scale".into()));
    let opts = scale_options();
    let sel = match scale_now {
        None => Some(0),
        Some(s) => opts.iter().position(|(_, v)| v.parse::<f64>().is_ok_and(|p| (p / 100.0 - s).abs() < 0.005)),
    };
    v.push(Item::Card(vec![Row::new("Interface scale").detail("Size of text and controls in rdwm (titlebars) and decebal apps (Xft.dpi)").ctl(Ctl::new("s:disp:scale:", Kind::Popup(opts, sel)))]));
    // night shift
    if let Some(Some(tool)) = h.disp.data.as_ref().map(|s| s.night_tool.clone()) {
        let (on, k) = h.session.night.clone().map(|(_, k)| (true, k)).unwrap_or((false, 4000));
        v.push(Item::Header("Night Shift".into()));
        v.push(Item::Card(vec![
            Row::new("Warmer colours").detail(format!("Uses {tool}; reduces blue light")).ctl(Ctl::new("s:disp:night:", Kind::Toggle(on))),
            Row::new("Colour temperature").ctl(Ctl::new("s:disp:kelvin:", Kind::Slider { v: k as f64, min: 2500.0, max: 6000.0, step: 100.0, unit: " K".into() })),
        ]));
    }
    v
}

pub fn apply(h: &mut Hub, act: &str, arg: &str, val: CtlVal) {
    if act.starts_with("wl-") {
        return wayland::apply(h, act, arg, val);
    }
    if physical_wayland_session() {
        h.say("X11 display controls are unavailable in a Wayland session", true);
        return;
    }
    let r: Result<(), String> = (|| {
        match (act, &val) {
            ("keep", _) => h.display_keep(),
            ("revert", _) => h.display_revert_now(),
            ("mode", CtlVal::Str(s)) => {
                let (w, hh) = s.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))).ok_or("Invalid resolution")?;
                h.display_change(arg, Change::Mode(w, hh));
            }
            ("rate", CtlVal::Str(s)) => h.display_change(arg, Change::Rate(s.parse().map_err(|_| "Invalid refresh rate")?)),
            ("rot", CtlVal::Str(s)) => h.display_change(arg, Change::Rotate(Rot::parse(s).ok_or("Invalid rotation")?)),
            ("pos", CtlVal::Str(s)) => {
                let (rel, other) = Rel::parse(s).ok_or("Invalid position")?;
                h.display_change(arg, Change::Place(rel, other.to_string()));
            }
            ("primary", _) => h.display_change(arg, Change::Primary),
            ("enable", CtlVal::Bool(b)) => h.display_change(arg, Change::Enable(*b)),
            ("mirror", CtlVal::Bool(b)) => {
                let Some(st) = h.disp.data.clone() else { return Ok(()) };
                let (ap, rv) = plan(&st, &st.active().first().map(|o| o.name.clone()).unwrap_or_default(), &Change::Mirror(*b))?;
                h.display_apply(ap, rv, "Mirroring");
            }
            ("night", CtlVal::Bool(on)) => {
                let tool = h.disp.data.as_ref().and_then(|s| s.night_tool.clone()).ok_or("Night shift needs gammastep or redshift")?;
                let k = h.session.night.as_ref().map(|n| n.1).unwrap_or(4000);
                let cmds = night_cmd(&tool, *on, k)?;
                let mut s = h.session.clone();
                s.night = on.then(|| (tool.clone(), k));
                let undo = night_cmd(&tool, !*on, k)?;
                h.run_session(Action::new(Dom::Display, "Night Shift", cmds).undo(undo), s);
            }
            ("kelvin", CtlVal::Num(k)) => {
                let tool = h.disp.data.as_ref().and_then(|s| s.night_tool.clone()).ok_or("Night shift needs gammastep or redshift")?;
                let k = *k as u32;
                let cmds = night_cmd(&tool, true, k)?;
                let mut s = h.session.clone();
                s.night = Some((tool, k));
                h.run_session(Action::new(Dom::Display, "Colour temperature", cmds), s);
            }
            _ => return Err("Unsupported display change".into()),
        }
        Ok(())
    })();
    if let Err(e) = r {
        h.say(e, true);
    }
}

/// Session part of the scale change (Xft.dpi). `percent` None = automatic (clears the override).
pub fn apply_scale_dpi(h: &mut Hub, percent: Option<u32>) -> Result<(), String> {
    let mut s = h.session.clone();
    match percent {
        None => {
            s.xft_dpi = None;
            h.run_session(Action::new(Dom::Display, "Scale", vec![]), s);
        }
        Some(p) => {
            if !(50..=400).contains(&p) {
                return Err("Scale must be between 50 % and 400 %".into());
            }
            let dpi = 96 * p / 100;
            let cmd = dpi_cmd(dpi)?;
            s.xft_dpi = Some(dpi);
            h.run_session(Action::new(Dom::Display, "Scale", vec![cmd]).msg("Scale saved; apps pick it up when they start"), s);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::{FakeRunner, Out};

    pub const TWO: &str = r#"Screen 0: minimum 320 x 200, current 4480 x 1440, maximum 16384 x 16384
DP-1 connected primary 2560x1440+0+0 (normal left inverted right x axis y axis) 597mm x 336mm
   2560x1440     143.97*+ 120.00    59.95  
   1920x1080     144.00   119.88    60.00    50.00  
   1280x720      60.00  
HDMI-1 connected 1920x1080+2560+0 (normal left inverted right x axis y axis) 527mm x 296mm
   1920x1080     60.00*+  50.00    59.94  
   1920x1080i    60.00    50.00  
   1280x720      60.00    50.00  
   640x480       60.00  
DP-2 disconnected (normal left inverted right x axis y axis)
HDMI-2 connected (normal left inverted right x axis y axis)
   1024x768      60.00 +
"#;
    const ROTATED: &str = r#"Screen 0: minimum 320 x 200, current 3000 x 1920, maximum 16384 x 16384
eDP-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 344mm x 193mm
   1920x1080     60.00*+
DP-3 connected 1080x1920+1920+0 left (normal left inverted right x axis y axis) 527mm x 296mm
   1920x1080     60.00*+
"#;
    const XVFB: &str = "Screen 0: minimum 1 x 1, current 1600 x 1000, maximum 1600 x 1000\nscreen connected 1600x1000+0+0 0mm x 0mm\n   1600x1000      0.00* \n";

    #[test]
    fn parses_outputs_modes_and_flags() {
        let o = parse_xrandr(TWO);
        assert_eq!(o.len(), 4);
        assert!(o[0].primary && o[0].active() && !o[1].primary);
        assert_eq!(o[0].geom, Some((2560, 1440, 0, 0)));
        assert_eq!(o[1].geom, Some((1920, 1080, 2560, 0)));
        assert_eq!(o[0].modes.len(), 3);
        assert_eq!(o[0].modes[0].rates, [143.97, 120.0, 59.95]);
        assert_eq!(o[0].modes[0].current, Some(143.97));
        assert_eq!(o[0].modes[0].preferred, Some(143.97));
        assert_eq!(o[1].current_mode().unwrap().1, 60.0);
        assert!(!o[2].connected);
        assert!(o[3].connected && !o[3].active(), "connected but off");
        assert_eq!(o[3].modes[0].preferred, Some(60.0), "a lone + after the rate marks it preferred");
    }

    #[test]
    fn interlaced_duplicates_merge() {
        let o = parse_xrandr(TWO);
        let m = o[1].modes.iter().find(|m| m.w == 1920).unwrap();
        assert_eq!(m.rates, [60.0, 59.94, 50.0]);
        assert_eq!(o[1].modes.iter().filter(|m| m.w == 1920).count(), 1);
    }

    #[test]
    fn rotation_and_negative_offsets() {
        let o = parse_xrandr(ROTATED);
        assert_eq!(o[1].rot, Rot::Left);
        assert_eq!(o[1].geom, Some((1080, 1920, 1920, 0)));
        assert_eq!(parse_geom("1920x1080-1920+0"), Some((1920, 1080, -1920, 0)));
        assert_eq!(parse_geom("1920x1080+0-200"), Some((1920, 1080, 0, -200)));
        assert_eq!(parse_geom("garbage"), None);
    }

    #[test]
    fn xvfb_style_zero_rate() {
        let o = parse_xrandr(XVFB);
        assert_eq!(o[0].name, "screen");
        assert_eq!(o[0].geom, Some((1600, 1000, 0, 0)));
        assert_eq!(o[0].modes[0].current, Some(0.0));
    }

    fn st() -> State {
        State { wayland: None, wayland_query_started: None, available: true, error: None, outputs: parse_xrandr(TWO), night_tool: None }
    }
    fn lines(v: &[Cmd]) -> Vec<String> {
        v.iter().map(|c| c.line()).collect()
    }

    #[test]
    fn layout_args_roundtrip_current_state() {
        let a = layout_args(&st());
        assert_eq!(
            a.join(" "),
            "--output DP-1 --mode 2560x1440 --rate 143.97 --pos 0x0 --rotate normal --primary --output HDMI-1 --mode 1920x1080 --rate 60.00 --pos 2560x0 --rotate normal --output HDMI-2 --off"
        );
    }

    #[test]
    fn mode_change_keeps_rate_when_possible() {
        let (ap, rv) = plan(&st(), "DP-1", &Change::Mode(1920, 1080)).unwrap();
        // 143.97 is not offered at 1920x1080, so the preferred/highest rate is used
        assert_eq!(lines(&ap), ["xrandr --output DP-1 --mode 1920x1080 --rate 144.00"]);
        assert!(lines(&rv)[0].starts_with("xrandr --output DP-1 --mode 2560x1440 --rate 143.97 --pos 0x0"));
        let (ap, _) = plan(&st(), "HDMI-1", &Change::Mode(1280, 720)).unwrap();
        assert_eq!(lines(&ap), ["xrandr --output HDMI-1 --mode 1280x720 --rate 60.00"]);
    }

    #[test]
    fn rate_rotate_place_primary() {
        let s = st();
        assert_eq!(lines(&plan(&s, "DP-1", &Change::Rate(120.0)).unwrap().0), ["xrandr --output DP-1 --mode 2560x1440 --rate 120.00"]);
        assert_eq!(lines(&plan(&s, "HDMI-1", &Change::Rotate(Rot::Right)).unwrap().0), ["xrandr --output HDMI-1 --rotate right"]);
        assert_eq!(lines(&plan(&s, "HDMI-1", &Change::Place(Rel::LeftOf, "DP-1".into())).unwrap().0), ["xrandr --output HDMI-1 --left-of DP-1"]);
        assert_eq!(lines(&plan(&s, "HDMI-1", &Change::Primary).unwrap().0), ["xrandr --output HDMI-1 --primary"]);
        assert_eq!(lines(&plan(&s, "HDMI-2", &Change::Enable(true)).unwrap().0), ["xrandr --output HDMI-2 --auto --right-of HDMI-1"]);
        assert_eq!(lines(&plan(&s, "DP-1", &Change::Enable(false)).unwrap().0), ["xrandr --output DP-1 --off --output HDMI-1 --primary"]);
    }

    #[test]
    fn planning_validates() {
        let s = st();
        assert!(plan(&s, "DP-1", &Change::Mode(1234, 567)).is_err());
        assert!(plan(&s, "DP-1", &Change::Rate(75.0)).is_err());
        assert!(plan(&s, "DP-2", &Change::Primary).is_err(), "disconnected");
        assert!(plan(&s, "nope", &Change::Primary).is_err());
        assert!(plan(&s, "--help", &Change::Primary).is_err());
        assert!(plan(&s, "DP-1", &Change::Place(Rel::Same, "DP-1".into())).is_err());
        assert!(plan(&s, "DP-1", &Change::Place(Rel::RightOf, "HDMI-2".into())).is_err(), "other display is off");
        assert!(plan(&s, "HDMI-2", &Change::Rotate(Rot::Left)).is_err(), "off");
        let one = State { available: true, outputs: parse_xrandr(XVFB), ..State::default() };
        assert!(plan(&one, "screen", &Change::Enable(false)).is_err(), "cannot switch off the last display");
        assert!(plan(&State::default(), "DP-1", &Change::Primary).is_err());
    }

    #[test]
    fn mirror_and_extend() {
        let s = st();
        let (ap, _) = plan(&s, "DP-1", &Change::Mirror(true)).unwrap();
        assert_eq!(lines(&ap), ["xrandr --output DP-1 --mode 1920x1080 --pos 0x0 --rotate normal --output HDMI-1 --mode 1920x1080 --same-as DP-1 --rotate normal"]);
        let mut m = s.clone();
        m.outputs[1].geom = Some((1920, 1080, 0, 0));
        assert!(mirrored(&m));
        assert!(!mirrored(&s));
        let (ap, _) = plan(&m, "DP-1", &Change::Mirror(false)).unwrap();
        assert_eq!(lines(&ap), ["xrandr --output DP-1 --pos 0x0 --output HDMI-1 --right-of DP-1"]);
        // no common resolution
        let mut x = s.clone();
        x.outputs[1].modes = vec![Mode { w: 800, h: 600, rates: vec![60.0], current: Some(60.0), preferred: None }];
        assert!(plan(&x, "DP-1", &Change::Mirror(true)).is_err());
    }

    #[test]
    fn night_and_dpi_commands() {
        assert_eq!(night_cmd("gammastep", true, 3500).unwrap()[0].line(), "gammastep -P -O 3500");
        assert_eq!(night_cmd("redshift", false, 3500).unwrap()[0].line(), "redshift -x");
        assert!(night_cmd("rm", true, 3500).is_err());
        assert!(night_cmd("gammastep", true, 50).is_err());
        let c = dpi_cmd(120).unwrap();
        assert_eq!(c.prog, "sh");
        assert_eq!(c.args.last().unwrap(), "120");
        assert!(dpi_cmd(5).is_err());
    }

    #[test]
    fn physical_wayland_detection_ignores_xwayland_rendering_and_tool_overrides() {
        use std::ffi::OsString;
        for (display, session, expected) in [(Some("wayland-private"), Some("x11"), true), (None, Some("wayland"), true), (Some(""), Some("x11"), false), (None, None, false)] {
            for (backend, forced) in [("x11", "1"), ("wayland", ""), ("auto", "1")] {
                let values =
                    std::collections::HashMap::from([("WAYLAND_DISPLAY", display), ("XDG_SESSION_TYPE", session), ("DECEBAL_BACKEND", Some(backend)), ("DECSETTINGS_FORCE_X11_TOOLS", Some(forced))]);
                let actual = physical_wayland_env(|key| values.get(key).copied().flatten().map(OsString::from));
                assert_eq!(actual, expected);
                let runner = FakeRunner::new().ok("xrandr --query", XVFB);
                let state = load_env(&runner, actual);
                if expected {
                    assert!(!state.available);
                    assert!(runner.lines().iter().all(|line| !line.contains("xrandr")));
                } else {
                    assert!(state.available);
                    assert_eq!(runner.lines(), ["xrandr --query"]);
                }
            }
        }
    }

    #[test]
    fn load_degrades() {
        let r = FakeRunner::new().ok("xrandr --query", XVFB);
        let s = load_env(&r, false);
        assert!(s.available && s.night_tool.is_none());
        let r = FakeRunner::new().ok("xrandr --query", XVFB).have("redshift");
        assert_eq!(load_env(&r, false).night_tool.as_deref(), Some("redshift"));
        assert!(load_env(&FakeRunner::new(), false).error.unwrap().contains("not installed"));
        let r = FakeRunner::new().on("xrandr --query", Out::fail("Can't open display"));
        assert!(load_env(&r, false).error.unwrap().contains("Can't open display"));
        assert!(load_env(&FakeRunner::new().ok("xrandr --query", XVFB), true).error.unwrap().contains("Wayland"));
    }

    /// Read-only run of the real xrandr against a private Xvfb. Opt in with
    /// `DECSETTINGS_REAL_XRANDR=1 DISPLAY=:148 cargo test real_xrandr` (only displays :145-:149 are accepted).
    #[test]
    fn real_xrandr_reads_a_private_display() {
        if std::env::var("DECSETTINGS_REAL_XRANDR").is_err() {
            return;
        }
        let d = std::env::var("DISPLAY").unwrap_or_default();
        assert!((145..=149).any(|n| d == format!(":{n}")), "refusing to run against DISPLAY={d}");
        let st = load_env(&super::super::run::RealRunner::default(), false);
        assert!(st.available, "{:?}", st.error);
        let o = st.connected();
        assert!(!o.is_empty(), "{st:?}");
        assert!(o[0].active() && o[0].current_mode().is_some(), "{:?}", o[0]);
        // planning against the real state builds a command but never runs it
        let (ap, rv) = plan(&st, &o[0].name, &Change::Rotate(Rot::Left)).unwrap();
        assert!(ap[0].line().contains("--rotate left") && rv[0].line().contains("--mode"));
    }

    #[test]
    fn items_cover_banner_and_controls() {
        let fr = std::sync::Arc::new(FakeRunner::lenient());
        let mut h = Hub::with(fr, std::env::temp_dir().join("ms-disp-t"), std::sync::Arc::new(|| {}));
        h.disp.data = Some(st());
        let ids = |it: &[Item]| -> Vec<String> { it.iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().flat_map(|r| r.ctls.iter().map(|c| c.id.clone())).collect() };
        let v = ids(&items(&h, None));
        for want in ["s:disp:mirror:", "s:disp:mode:DP-1", "s:disp:rate:DP-1", "s:disp:rot:HDMI-1", "s:disp:pos:HDMI-1", "s:disp:primary:HDMI-1", "s:disp:scale:", "s:disp:enable:HDMI-2"] {
            assert!(v.contains(&want.to_string()), "{want} in {v:?}");
        }
        assert!(!v.contains(&"s:disp:primary:DP-1".to_string()));
        assert!(!v.contains(&"s:disp:keep:".to_string()));
        h.sh.confirm.lock().unwrap().begin(Instant::now(), 15, vec![], vec![], None);
        let v = ids(&items(&h, None));
        assert!(v.contains(&"s:disp:keep:".to_string()) && v.contains(&"s:disp:revert:".to_string()));
        assert!(!ids(&items(&h, None)).contains(&"s:disp:night:".to_string()), "night shift hidden without a tool");
        h.disp.data.as_mut().unwrap().night_tool = Some("gammastep".into());
        assert!(ids(&items(&h, None)).contains(&"s:disp:night:".to_string()));
    }
}
