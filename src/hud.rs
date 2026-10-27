//! `machud`: the macOS-style on-screen display shown when volume, mute, brightness or the microphone change.
//!
//! A small rounded card (icon, label, 16-segment bar) in an override-redirect X11 popup that takes no input at
//! all (see [`WindowApp::popup_passive`]) and goes away after [`TIMEOUT`]. The binary reads the current value itself
//! (`wpctl` / `pactl`, `brightnessctl` / sysfs); a second launch while the card is up leaves its reading in a small
//! mailbox file in `$XDG_RUNTIME_DIR` and signals the running one, which redraws and extends its timer.

use crate::app::Button;
use crate::controlcenter::sys;
use crate::gfx::{blit_scaled, fill_rounded, stroke_rounded};
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use crate::window_app::{Action, KeyEvent, Response, Waker, WindowApp};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::{Color, Pixmap};

/// How long the card stays after the last invocation.
pub const TIMEOUT: Duration = Duration::from_millis(1400);
/// Signal a second launch sends to the running one (after writing the mailbox).
pub const SIG_UPDATE: i32 = libc::SIGUSR1;
/// Name of the single-instance lock (`$XDG_RUNTIME_DIR/machud.lock`).
pub const LOCK_NAME: &str = "machud";
/// Mailbox file name inside `$XDG_RUNTIME_DIR`.
pub const MAILBOX: &str = "machud.msg";
/// Number of bar segments.
pub const SEGMENTS: usize = 16;
/// Number of workspace dots (rdwm's tags).
pub const WORKSPACES: usize = 9;
/// The card's bottom edge sits this fraction of the monitor height above the monitor's bottom.
pub const BOTTOM_MARGIN: f32 = 0.10;

/// A probe must not hold the card back (a stuck audio daemon): give up after this long.
const PROBE_TIMEOUT: Duration = Duration::from_millis(800);

// ---------------------------------------------------------------- what is shown

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Volume,
    Brightness,
    Mic,
    /// The workspace rdwm switched to (`Reading::level` holds its number, 1-based).
    Workspace,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "volume" => Some(Kind::Volume),
            "brightness" => Some(Kind::Brightness),
            "mic" | "microphone" => Some(Kind::Mic),
            "workspace" => Some(Kind::Workspace),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Volume => "volume",
            Kind::Brightness => "brightness",
            Kind::Mic => "mic",
            Kind::Workspace => "workspace",
        }
    }
}

/// One value to show: `level` is a fraction (1.0 = 100 %; amplified volume may exceed it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub kind: Kind,
    pub level: f32,
    pub muted: bool,
}

impl Reading {
    pub fn percent(&self) -> u32 {
        (self.level.max(0.0) * 100.0).round() as u32
    }

    pub fn label(&self) -> String {
        match (self.kind, self.muted) {
            (Kind::Volume, true) => "Muted".into(),
            (Kind::Mic, true) => "Microphone muted".into(),
            (Kind::Volume, false) => format!("Volume {}%", self.percent()),
            (Kind::Mic, false) => format!("Microphone {}%", self.percent()),
            (Kind::Brightness, _) => format!("Brightness {}%", self.percent()),
            (Kind::Workspace, _) => format!("Workspace {}", self.workspace()),
        }
    }

    /// Workspace number of a `Kind::Workspace` reading, clamped to 1..=WORKSPACES.
    pub fn workspace(&self) -> usize {
        (self.level.round().max(1.0) as usize).min(WORKSPACES)
    }

    /// Symbol name (see `menubar::symbols`).
    pub fn icon(&self) -> &'static str {
        match self.kind {
            Kind::Volume if self.muted => "speaker-muted",
            Kind::Volume if self.level <= 0.0 => "speaker-0",
            Kind::Volume if self.level < 0.34 => "speaker-1",
            Kind::Volume if self.level < 0.67 => "speaker-2",
            Kind::Volume => "speaker-3",
            Kind::Mic if self.muted => "mic-muted",
            Kind::Mic => "mic",
            Kind::Brightness => "sun",
            Kind::Workspace => "grid",
        }
    }

    /// Segments drawn lit: none when muted, otherwise the level rounded to the nearest segment.
    pub fn lit(&self) -> usize {
        if self.muted {
            return 0;
        }
        ((self.level.clamp(0.0, 1.0) * SEGMENTS as f32).round() as usize).min(SEGMENTS)
    }

    /// One line for the mailbox: `kind level muted`.
    pub fn encode(&self) -> String {
        format!("{} {:.4} {}", self.kind.name(), self.level, u8::from(self.muted))
    }

    pub fn decode(line: &str) -> Option<Reading> {
        let mut it = line.split_whitespace();
        let kind = Kind::parse(it.next()?)?;
        let level: f32 = it.next()?.parse().ok().filter(|l: &f32| l.is_finite())?;
        let muted = it.next()? == "1";
        Some(Reading { kind, level, muted })
    }
}

// ---------------------------------------------------------------- reading the system

/// Default output (or input) volume and mute state: `wpctl`, else `pactl`.
pub fn probe_audio(kind: Kind) -> Option<Reading> {
    let source = kind == Kind::Mic;
    let target = if source { "@DEFAULT_AUDIO_SOURCE@" } else { "@DEFAULT_AUDIO_SINK@" };
    if let Some(v) = sys::run(&["wpctl", "get-volume", target], PROBE_TIMEOUT).and_then(|o| sys::parse_wpctl_volume(&o)) {
        return Some(Reading { kind, level: v.level, muted: v.muted });
    }
    let (get, mute, dev) = if source { ("get-source-volume", "get-source-mute", "@DEFAULT_SOURCE@") } else { ("get-sink-volume", "get-sink-mute", "@DEFAULT_SINK@") };
    let vol = sys::run(&["pactl", get, dev], PROBE_TIMEOUT)?;
    let muted = sys::run(&["pactl", mute, dev], PROBE_TIMEOUT).unwrap_or_default();
    let v = sys::parse_pactl_volume(&vol, &muted)?;
    Some(Reading { kind, level: v.level, muted: v.muted })
}

/// Brightness of the first backlight under `root` (`/sys/class/backlight`) in percent.
pub fn brightness_from_sysfs(root: &Path) -> Option<u32> {
    let mut dirs: Vec<_> = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    dirs.iter().find_map(|d| {
        let read = |f: &str| std::fs::read_to_string(d.join(f)).ok()?.trim().parse::<u64>().ok();
        let (cur, max) = (read("brightness")?, read("max_brightness")?);
        (max > 0).then(|| ((cur * 100 + max / 2) / max).min(100) as u32)
    })
}

/// Screen brightness: `brightnessctl -m`, else sysfs (`MACHUD_BACKLIGHT_DIR` replaces `/sys/class/backlight`, for tests).
pub fn probe_brightness() -> Option<Reading> {
    let root = std::env::var_os("MACHUD_BACKLIGHT_DIR").map_or_else(|| "/sys/class/backlight".into(), std::path::PathBuf::from);
    let pct = sys::run(&["brightnessctl", "-m"], PROBE_TIMEOUT).and_then(|o| sys::parse_brightnessctl(&o)).or_else(|| brightness_from_sysfs(&root))?;
    Some(Reading { kind: Kind::Brightness, level: pct as f32 / 100.0, muted: false })
}

pub fn probe(kind: Kind) -> Option<Reading> {
    match kind {
        Kind::Brightness => probe_brightness(),
        Kind::Volume | Kind::Mic => probe_audio(kind),
        Kind::Workspace => probe_workspace(),
    }
}

/// Lowest tag of the selected monitor's view in a `get_state` reply (1-based).
pub fn workspace_from_state(line: &str) -> Option<usize> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let st = v.get("state")?;
    let mons = st.get("monitors")?.as_array()?;
    let sel = st.get("selected_mon").and_then(serde_json::Value::as_i64).unwrap_or(0);
    let m = mons.iter().find(|m| m.get("id").and_then(serde_json::Value::as_i64) == Some(sel)).or_else(|| mons.first())?;
    let view = m.get("view")?.as_u64()?;
    (view != 0).then(|| view.trailing_zeros() as usize + 1)
}

/// The workspace rdwm is showing now.
pub fn probe_workspace() -> Option<Reading> {
    let n = crate::menubar::rdwm::request(r#"{"cmd":"get_state"}"#).and_then(|l| workspace_from_state(&l))?;
    Some(Reading { kind: Kind::Workspace, level: n as f32, muted: false })
}

/// Long-running: shows the card whenever rdwm changes the workspace. Blocks; reconnects when rdwm restarts.
pub fn watch_workspaces() {
    use std::io::{BufRead, BufReader, Write};
    let mut last: Option<usize> = None;
    loop {
        let conn = std::os::unix::net::UnixStream::connect(crate::menubar::rdwm::socket_path());
        if let Ok(mut s) = conn {
            last = crate::menubar::rdwm::request(r#"{"cmd":"get_state"}"#).and_then(|l| workspace_from_state(&l));
            if s.write_all(b"{\"cmd\":\"subscribe\",\"events\":[\"tag_view\"]}\n").is_ok() {
                for line in BufReader::new(&mut s).lines() {
                    let Ok(line) = line else { break };
                    if !line.contains("\"event\"") {
                        continue;
                    }
                    let now = crate::menubar::rdwm::request(r#"{"cmd":"get_state"}"#).and_then(|l| workspace_from_state(&l));
                    if now.is_some() && now != last {
                        last = now;
                        if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(exe).arg("workspace").stdin(std::process::Stdio::null()).spawn().map(|mut c| std::thread::spawn(move || c.wait()));
                        }
                    }
                }
            }
        }
        let _ = last;
        std::thread::sleep(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------- mailbox (a second launch updates the running card)

/// Leaves `r` for the running instance (written to a temp file and renamed, so a reader never sees half of it).
pub fn post(r: &Reading) {
    let path = crate::single::runtime_file(MAILBOX);
    let tmp = path.with_extension(format!("msg.{}", std::process::id()));
    if std::fs::write(&tmp, r.encode()).is_ok() && std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Takes the posted reading, if any.
pub fn take_post() -> Option<Reading> {
    let path = crate::single::runtime_file(MAILBOX);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Reading::decode(&text)
}

/// Drops a stale post (this instance was started fresh and shows its own reading).
pub fn clear_post() {
    let _ = std::fs::remove_file(crate::single::runtime_file(MAILBOX));
}

// ---------------------------------------------------------------- layout

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub w: u32,
    pub h: u32,
    pub icon_px: u32,
    pub icon_x: f32,
    pub icon_y: f32,
    /// Vertical middle of the label.
    pub label_cy: f32,
    pub bar_x: f32,
    pub bar_y: f32,
    pub seg_w: f32,
    pub seg_h: f32,
    pub gap: f32,
}

impl Layout {
    /// Left edge of segment `i`.
    pub fn seg_x(&self, i: usize) -> f32 {
        self.bar_x + i as f32 * (self.seg_w + self.gap)
    }

    /// Vertical middle of the bar.
    pub fn bar_cy(&self) -> f32 {
        self.bar_y + self.seg_h / 2.0
    }
}

/// Card geometry at UI scale `scale` (1.0 = 96 dpi): 240 x 148, the bar 190 wide and 28 above the bottom edge.
pub fn layout(scale: f32) -> Layout {
    let s = scale.clamp(1.0, 4.0);
    let px = |v: f32| (v * s).round();
    let (w, h) = (px(240.0), px(148.0));
    let (seg_w, gap) = (px(10.0), px(2.0));
    let bar_w = SEGMENTS as f32 * seg_w + (SEGMENTS as f32 - 1.0) * gap;
    let icon = px(60.0);
    Layout {
        w: w as u32,
        h: h as u32,
        icon_px: icon as u32,
        icon_x: ((w - icon) / 2.0).round(),
        icon_y: px(20.0),
        label_cy: px(100.0),
        bar_x: ((w - bar_w) / 2.0).round(),
        bar_y: px(116.0),
        seg_w,
        seg_h: px(8.0),
        gap,
    }
}

// ---------------------------------------------------------------- the app

pub struct Hud {
    reading: Reading,
    timeout: Duration,
    deadline: Arc<Mutex<Instant>>,
    posted: Arc<AtomicBool>,
    scale: f32,
    alpha: bool,
    fonts: Option<Fonts>,
    symbols: SymbolCache,
}

impl Hud {
    pub fn new(reading: Reading) -> Self {
        Self::with_timeout(reading, TIMEOUT)
    }

    pub fn with_timeout(reading: Reading, timeout: Duration) -> Self {
        Self {
            reading,
            timeout,
            deadline: Arc::new(Mutex::new(Instant::now() + timeout)),
            posted: Arc::new(AtomicBool::new(false)),
            scale: 1.0,
            alpha: false,
            fonts: Fonts::load(),
            symbols: SymbolCache::default(),
        }
    }

    pub fn reading(&self) -> Reading {
        self.reading
    }

    /// Shows `r` and restarts the countdown.
    pub fn update(&mut self, r: Reading) -> Response {
        self.reading = r;
        if let Ok(mut d) = self.deadline.lock() {
            *d = Instant::now() + self.timeout;
        }
        Response::REDRAW
    }

    fn expired(&self) -> bool {
        self.deadline.lock().map_or(true, |d| Instant::now() >= *d)
    }

    fn measure(&self, s: &str, px: f32) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, Weight::Medium))
    }

    fn draw_card(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let l = layout(self.scale);
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        pm.fill(Color::TRANSPARENT);
        let radius = th.r(1.8) * self.scale;
        let base = if self.alpha { th.overlay_c() } else { th.overlay.color(1.0) };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, base);

        let r = self.reading;
        let px = l.icon_px;
        if r.kind == Kind::Workspace {
            // The number itself, big, where the other cards have their icon.
            let digit = r.workspace().to_string();
            let size = 54.0 * self.scale;
            let tw = self.measure(&digit, size);
            if let Some(f) = &mut self.fonts {
                f.draw(pm, &digit, (w / 2.0 - tw / 2.0).round(), l.icon_y - size * 0.05, size, Weight::Medium, th.text_c());
            }
        } else if let Some(icon) = self.symbols.get(r.icon(), th.text.t(), px) {
            blit_scaled(pm, icon, l.icon_x, l.icon_y, px as f32, px as f32, 1.0);
        }

        let label = r.label();
        let size = 14.0 * self.scale;
        let tw = self.measure(&label, size);
        if let Some(f) = &mut self.fonts {
            f.draw(pm, &label, (w / 2.0 - tw / 2.0).round(), l.label_cy - size * 0.62, size, Weight::Medium, th.text_c());
        }

        if r.kind == Kind::Workspace {
            let (dot, gap) = (10.0 * self.scale, 8.0 * self.scale);
            let x0 = (w - (WORKSPACES as f32 * dot + (WORKSPACES as f32 - 1.0) * gap)) / 2.0;
            for i in 0..WORKSPACES {
                let c = if i + 1 == r.workspace() { th.text.color(0.95) } else { th.ink(0.24) };
                fill_rounded(pm, (x0 + i as f32 * (dot + gap)).round(), l.bar_y, dot, l.seg_h.max(dot * 0.8), dot / 2.0, c);
            }
            stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), th.outline_c(), 1.0);
            if let Some(f) = &self.fonts {
                f.trim();
            }
            return;
        }
        let lit = r.lit();
        let unlit = th.ink(if r.muted { 0.12 } else { 0.24 });
        for i in 0..SEGMENTS {
            let c = if i < lit { th.text.color(0.95) } else { unlit };
            fill_rounded(pm, l.seg_x(i), l.bar_y, l.seg_w, l.seg_h, 1.5 * self.scale, c);
        }
        stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), th.outline_c(), 1.0);
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }
}

impl WindowApp for Hud {
    fn title(&self) -> String {
        "machud".into()
    }

    fn app_id(&self) -> &'static str {
        "machud"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn popup_passive(&self) -> bool {
        true
    }

    fn popup_bottom_margin(&self) -> Option<f32> {
        Some(BOTTOM_MARGIN)
    }

    fn popup_translucent(&self) -> bool {
        true
    }

    fn popup_closes_on_focus_loss(&self) -> bool {
        false
    }

    fn set_popup_alpha(&mut self, alpha: bool) {
        self.alpha = alpha;
    }

    fn initial_size(&self) -> (u32, u32) {
        let l = layout(self.scale);
        (l.w, l.h)
    }

    fn content_size(&self) -> Option<(u32, u32)> {
        Some(self.initial_size())
    }

    fn set_waker(&mut self, waker: Waker) {
        // A second launch signals us: a thread turns that into a wake-up for the loop ...
        let posted = self.posted.clone();
        let w = waker.clone();
        std::thread::spawn(move || {
            crate::single::block_signals(&[SIG_UPDATE]);
            while crate::single::wait_signal(&[SIG_UPDATE]).is_some() {
                posted.store(true, Ordering::SeqCst);
                w();
            }
        });
        // ... and another one wakes it when the countdown ends (zero cost in between: it sleeps until then).
        let deadline = self.deadline.clone();
        std::thread::spawn(move || loop {
            let left = deadline.lock().map(|d| d.saturating_duration_since(Instant::now())).unwrap_or_default();
            if left.is_zero() {
                waker();
                // `tick` closes the window; if a late update extended the deadline instead, look again shortly.
                std::thread::sleep(Duration::from_millis(25));
            } else {
                std::thread::sleep(left);
            }
        });
    }

    fn set_screen(&mut self, _w: u32, _h: u32) {}

    fn set_scale(&mut self, scale: f32) {
        self.scale = scale.clamp(1.0, 4.0);
    }

    fn resize(&mut self, _w: u32, _h: u32) {}

    fn set_rounded(&mut self, _rounded: bool) {}

    fn draw(&mut self, pm: &mut Pixmap) {
        self.draw_card(pm);
    }

    fn pointer_motion(&mut self, _x: f32, _y: f32) -> Response {
        Response::NONE
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, _x: f32, _y: f32, _button: Button, _pressed: bool) -> Response {
        Response::NONE
    }

    fn scroll(&mut self, _x: f32, _y: f32, _dy: f32) -> Response {
        Response::NONE
    }

    fn key(&mut self, _ev: &KeyEvent) -> Response {
        Response::NONE
    }

    fn tick(&mut self) -> Response {
        let mut resp = Response::NONE;
        if self.posted.swap(false, Ordering::SeqCst) {
            if let Some(r) = take_post() {
                resp = self.update(r);
            }
        }
        if self.expired() {
            return Response::act(Action::Close);
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(kind: Kind, level: f32, muted: bool) -> Reading {
        Reading { kind, level, muted }
    }

    #[test]
    fn labels() {
        assert_eq!(r(Kind::Volume, 0.45, false).label(), "Volume 45%");
        assert_eq!(r(Kind::Volume, 0.45, true).label(), "Muted");
        assert_eq!(r(Kind::Brightness, 0.7, false).label(), "Brightness 70%");
        assert_eq!(r(Kind::Mic, 0.3, false).label(), "Microphone 30%");
        assert_eq!(r(Kind::Mic, 0.3, true).label(), "Microphone muted");
        assert_eq!(r(Kind::Volume, 1.5, false).label(), "Volume 150%", "amplified volume is shown as it is");
        assert_eq!(r(Kind::Volume, 0.0, false).label(), "Volume 0%");
    }

    #[test]
    fn icons_follow_level_and_mute() {
        assert_eq!(r(Kind::Volume, 0.9, true).icon(), "speaker-muted");
        assert_eq!(r(Kind::Volume, 0.0, false).icon(), "speaker-0");
        assert_eq!(r(Kind::Volume, 0.2, false).icon(), "speaker-1");
        assert_eq!(r(Kind::Volume, 0.5, false).icon(), "speaker-2");
        assert_eq!(r(Kind::Volume, 0.8, false).icon(), "speaker-3");
        assert_eq!(r(Kind::Mic, 0.8, true).icon(), "mic-muted");
        assert_eq!(r(Kind::Mic, 0.8, false).icon(), "mic");
        assert_eq!(r(Kind::Brightness, 0.8, false).icon(), "sun");
        for k in [Kind::Volume, Kind::Mic, Kind::Brightness] {
            for m in [false, true] {
                for l in [0.0, 0.2, 0.5, 0.9] {
                    assert!(crate::menubar::symbols::shapes(r(k, l, m).icon()).is_some(), "{k:?} {l} {m}");
                }
            }
        }
    }

    #[test]
    fn lit_segments_round_to_the_nearest_and_clamp() {
        assert_eq!(r(Kind::Volume, 0.0, false).lit(), 0);
        assert_eq!(r(Kind::Volume, 0.5, false).lit(), 8);
        assert_eq!(r(Kind::Volume, 0.45, false).lit(), 7);
        assert_eq!(r(Kind::Volume, 1.0, false).lit(), 16);
        assert_eq!(r(Kind::Volume, 1.5, false).lit(), 16);
        assert_eq!(r(Kind::Volume, -0.2, false).lit(), 0);
        assert_eq!(r(Kind::Volume, 0.9, true).lit(), 0, "muted: an empty bar");
        assert_eq!(r(Kind::Brightness, 0.7, false).lit(), 11);
    }

    #[test]
    fn mailbox_lines_round_trip() {
        for x in [r(Kind::Volume, 0.45, false), r(Kind::Mic, 1.25, true), r(Kind::Brightness, 0.0, false)] {
            let back = Reading::decode(&x.encode()).unwrap();
            assert_eq!(back.kind, x.kind);
            assert_eq!(back.muted, x.muted);
            assert!((back.level - x.level).abs() < 1e-4);
        }
        assert_eq!(Reading::decode(""), None);
        assert_eq!(Reading::decode("volume x 0"), None);
        assert_eq!(Reading::decode("volume NaN 0"), None);
        assert_eq!(Reading::decode("nonsense 0.5 0"), None);
        assert_eq!(Reading::decode("volume 0.5"), None);
    }

    #[test]
    fn workspace_card_and_state() {
        let w = r(Kind::Workspace, 3.0, false);
        assert_eq!((w.label().as_str(), w.workspace()), ("Workspace 3", 3));
        assert_eq!(r(Kind::Workspace, 40.0, false).workspace(), WORKSPACES, "clamped");
        assert_eq!(Reading::decode(&w.encode()).unwrap().workspace(), 3);
        assert_eq!(Kind::parse("workspace"), Some(Kind::Workspace));
        assert!(crate::menubar::symbols::shapes(w.icon()).is_some());
        let st = |view: u64| format!(r#"{{"ok":true,"state":{{"selected_mon":1,"monitors":[{{"id":0,"view":1}},{{"id":1,"view":{view}}}]}}}}"#);
        assert_eq!(workspace_from_state(&st(1)), Some(1));
        assert_eq!(workspace_from_state(&st(8)), Some(4));
        assert_eq!(workspace_from_state(&st(12)), Some(3), "several tags: the lowest");
        assert_eq!(workspace_from_state(&st(0)), None);
        assert_eq!(workspace_from_state("nonsense"), None);
    }

    #[test]
    fn kinds_parse() {
        assert_eq!(Kind::parse("volume"), Some(Kind::Volume));
        assert_eq!(Kind::parse("brightness"), Some(Kind::Brightness));
        assert_eq!(Kind::parse("mic"), Some(Kind::Mic));
        assert_eq!(Kind::parse("vol"), None);
    }

    #[test]
    fn tool_outputs_parse() {
        let v = sys::parse_wpctl_volume("Volume: 0.45\n").unwrap();
        assert_eq!((v.level, v.muted), (0.45, false));
        let v = sys::parse_wpctl_volume("Volume: 0.45 [MUTED]\n").unwrap();
        assert_eq!((v.level, v.muted), (0.45, true));
        assert_eq!(sys::parse_wpctl_volume("garbage"), None);
        assert_eq!(sys::parse_brightnessctl("intel_backlight,backlight,19200,50%,38400\n"), Some(50));
        assert_eq!(sys::parse_brightnessctl("kbd_backlight,leds,1,33%,3\nintel_backlight,backlight,26880,70%,38400\n"), Some(70));
        assert_eq!(sys::parse_brightnessctl("kbd_backlight,leds,1,33%,3\n"), None);
        let p = sys::parse_pactl_volume("Volume: front-left: 29491 /  45% / -20.00 dB,   front-right: 29491 /  45% / -20.00 dB", "Mute: yes").unwrap();
        assert_eq!((p.level, p.muted), (0.45, true));
    }

    #[test]
    fn brightness_from_a_sysfs_tree() {
        let dir = std::env::temp_dir().join(format!("machud-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(brightness_from_sysfs(&dir), None, "no backlight at all");
        let bl = dir.join("intel_backlight");
        std::fs::create_dir_all(&bl).unwrap();
        std::fs::write(bl.join("brightness"), "26880\n").unwrap();
        std::fs::write(bl.join("max_brightness"), "38400\n").unwrap();
        assert_eq!(brightness_from_sysfs(&dir), Some(70));
        std::fs::write(bl.join("max_brightness"), "0\n").unwrap();
        assert_eq!(brightness_from_sysfs(&dir), None, "a zero maximum is not a backlight");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_is_centred_and_inside_the_card() {
        for s in [1.0, 1.25, 1.5, 2.0] {
            let l = layout(s);
            let bar_w = l.seg_x(SEGMENTS - 1) + l.seg_w - l.bar_x;
            assert!((l.bar_x * 2.0 + bar_w - l.w as f32).abs() <= 1.0, "bar centred at {s}");
            assert!((l.icon_x * 2.0 + l.icon_px as f32 - l.w as f32).abs() <= 1.0, "icon centred at {s}");
            assert!(l.icon_y + l.icon_px as f32 <= l.label_cy, "icon above the label at {s}");
            assert!(l.label_cy < l.bar_y, "label above the bar at {s}");
            assert!(l.bar_y + l.seg_h < l.h as f32, "bar inside the card at {s}");
            assert!(l.w >= 200, "wide enough for the window backend's minimum");
        }
        let l = layout(1.0);
        assert_eq!((l.w, l.h), (240, 148));
        assert_eq!(l.h as f32 - l.bar_cy(), 28.0, "tests/e2e/hud.sh relies on this");
        assert_eq!(l.seg_x(0) + l.seg_w / 2.0, 30.0, "tests/e2e/hud.sh relies on this");
        assert_eq!(l.seg_w + l.gap, 12.0, "tests/e2e/hud.sh relies on this");
    }

    fn drawn(reading: Reading, alpha: bool) -> Pixmap {
        let mut hud = Hud::new(reading);
        hud.set_popup_alpha(alpha);
        let l = layout(1.0);
        let mut pm = Pixmap::new(l.w, l.h).unwrap();
        hud.draw(&mut pm);
        pm
    }

    fn rgb(pm: &Pixmap, x: f32, y: f32) -> (u8, u8, u8, u8) {
        let p = pm.pixel(x as u32, y as u32).unwrap();
        (p.red(), p.green(), p.blue(), p.alpha())
    }

    /// Segments whose middle is much further from the card colour than an unlit one.
    fn lit_in(pm: &Pixmap) -> usize {
        let l = layout(1.0);
        let bg = rgb(pm, l.w as f32 / 2.0, 4.0);
        let dist = |p: (u8, u8, u8, u8)| (i32::from(p.0) - i32::from(bg.0)).abs() + (i32::from(p.1) - i32::from(bg.1)).abs() + (i32::from(p.2) - i32::from(bg.2)).abs();
        (0..SEGMENTS).filter(|&i| dist(rgb(pm, l.seg_x(i) + l.seg_w / 2.0, l.bar_cy())) > 350).count()
    }

    #[test]
    fn bar_fill_matches_the_level() {
        for (level, want) in [(0.0, 0), (0.25, 4), (0.5, 8), (0.75, 12), (1.0, 16)] {
            assert_eq!(lit_in(&drawn(r(Kind::Volume, level, false), false)), want, "level {level}");
        }
        assert_eq!(lit_in(&drawn(r(Kind::Volume, 0.8, true), false)), 0, "muted draws an empty bar");
        assert_eq!(lit_in(&drawn(r(Kind::Brightness, 0.5, false), true)), 8, "also with an alpha channel");
    }

    #[test]
    fn card_is_opaque_without_a_compositor_and_translucent_with_one() {
        let l = layout(1.0);
        let opaque = drawn(r(Kind::Volume, 0.5, false), false);
        assert_eq!(rgb(&opaque, l.w as f32 / 2.0, 4.0).3, 255);
        assert_eq!(rgb(&opaque, 0.0, 0.0).3, 0, "rounded corner");
        let glass = drawn(r(Kind::Volume, 0.5, false), true);
        let a = rgb(&glass, l.w as f32 / 2.0, 4.0).3;
        assert!(a > 0 && a < 255, "translucent card, alpha {a}");
    }

    #[test]
    fn icon_and_label_are_drawn() {
        let l = layout(1.0);
        let pm = drawn(r(Kind::Volume, 0.5, false), false);
        let bg = rgb(&pm, l.w as f32 / 2.0, 4.0);
        let differs = |x0: f32, y0: f32, w: f32, h: f32| (0..w as u32).any(|dx| (0..h as u32).any(|dy| rgb(&pm, x0 + dx as f32, y0 + dy as f32) != bg));
        assert!(differs(l.icon_x, l.icon_y, l.icon_px as f32, l.icon_px as f32), "icon");
        assert!(differs(40.0, l.label_cy - 8.0, 160.0, 16.0), "label");
        // Muted and unmuted icons differ.
        let muted = drawn(r(Kind::Volume, 0.5, true), false);
        assert_ne!(pm.data(), muted.data());
    }

    #[test]
    fn countdown_closes_unless_extended() {
        let mut hud = Hud::with_timeout(r(Kind::Volume, 0.5, false), Duration::from_millis(0));
        assert_eq!(hud.tick().action, Some(Action::Close));
        let mut hud = Hud::with_timeout(r(Kind::Volume, 0.5, false), Duration::from_secs(60));
        assert_eq!(hud.tick(), Response::NONE);
        let before = *hud.deadline.lock().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let resp = hud.update(r(Kind::Volume, 0.6, false));
        assert!(resp.redraw);
        assert!(*hud.deadline.lock().unwrap() > before, "an update restarts the countdown");
        assert_eq!(hud.reading().level, 0.6);
    }

    #[test]
    fn passive_popup_wiring() {
        let hud = Hud::new(r(Kind::Volume, 0.5, false));
        assert!(hud.overlay() && hud.popup_passive() && hud.popup_translucent());
        assert!(!hud.popup_closes_on_focus_loss());
        assert_eq!(hud.popup_bottom_margin(), Some(0.10));
        assert_eq!(hud.content_size(), Some((240, 148)));
    }
}
