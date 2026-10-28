//! decsession: the session menu, a full-screen dimmer with a centred row of big rounded buttons
//! (Lock, Log out, Suspend, Hibernate, Restart, Shut down), ported from caelestia-shell's session drawer.
//!
//! The UI is an X11 popup that covers the primary monitor (override-redirect, keyboard and pointer grabbed, see
//! [`crate::backend::x11_window`]). This module holds the logic (which buttons exist, the commands behind them,
//! keys, the confirm flow, layout, drawing); the `decsession` binary runs the chosen command once the overlay
//! (and with it the input grabs) is gone, so a screen locker can take the keyboard.
//!
//! Destructive choices (log out, restart, shut down) ask twice: the first key press or click arms the button
//! ("Press again"), the second one on the same button runs it. Anything else disarms.

use crate::app::Button;
use crate::gfx::{color, fill_rect, fill_rounded, stroke_rounded};
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use tiny_skia::{Color, Pixmap};

/// Side of a button in logical pixels.
const BUTTON: f32 = 112.0;
const GAP: f32 = 28.0;
const LABEL_H: f32 = 44.0;

// ---------------------------------------------------------------- the choices

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Lock,
    Logout,
    Suspend,
    Hibernate,
    Restart,
    Shutdown,
}

impl Choice {
    pub const ALL: [Choice; 6] = [Choice::Lock, Choice::Logout, Choice::Suspend, Choice::Hibernate, Choice::Restart, Choice::Shutdown];

    pub fn label(self) -> &'static str {
        match self {
            Choice::Lock => "Lock",
            Choice::Logout => "Log Out",
            Choice::Suspend => "Suspend",
            Choice::Hibernate => "Hibernate",
            Choice::Restart => "Restart",
            Choice::Shutdown => "Shut Down",
        }
    }

    /// The shortcut letter (lower case).
    pub fn key(self) -> char {
        match self {
            Choice::Lock => 'l',
            Choice::Logout => 'o',
            Choice::Suspend => 's',
            Choice::Hibernate => 'h',
            Choice::Restart => 'r',
            Choice::Shutdown => 'p',
        }
    }

    /// Symbol name (see `menubar::symbols`).
    pub fn icon(self) -> &'static str {
        match self {
            Choice::Lock => "lock",
            Choice::Logout => "logout",
            Choice::Suspend => "moon",
            Choice::Hibernate => "snowflake",
            Choice::Restart => "refresh",
            Choice::Shutdown => "power",
        }
    }

    /// Needs a second press (loses unsaved work).
    pub fn destructive(self) -> bool {
        matches!(self, Choice::Logout | Choice::Restart | Choice::Shutdown)
    }

    pub fn from_key(c: char) -> Option<Choice> {
        let c = c.to_ascii_lowercase();
        Choice::ALL.into_iter().find(|ch| ch.key() == c)
    }

    pub fn name(self) -> &'static str {
        match self {
            Choice::Lock => "lock",
            Choice::Logout => "logout",
            Choice::Suspend => "suspend",
            Choice::Hibernate => "hibernate",
            Choice::Restart => "restart",
            Choice::Shutdown => "shutdown",
        }
    }

    pub fn parse(s: &str) -> Option<Choice> {
        match s {
            "lock" => Some(Choice::Lock),
            "logout" | "log-out" => Some(Choice::Logout),
            "suspend" | "sleep" => Some(Choice::Suspend),
            "hibernate" => Some(Choice::Hibernate),
            "restart" | "reboot" => Some(Choice::Restart),
            "shutdown" | "poweroff" | "shut-down" => Some(Choice::Shutdown),
            _ => None,
        }
    }
}

/// Whether `/sys/power/state` (its text) offers hibernation (`disk`).
pub fn state_offers_disk(text: &str) -> bool {
    text.split_whitespace().any(|w| w == "disk")
}

/// Path of the kernel's list of sleep states (`DECSESSION_POWER_STATE` overrides it for tests).
pub fn power_state_path() -> PathBuf {
    std::env::var_os("DECSESSION_POWER_STATE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/sys/power/state"))
}

pub fn hibernate_available() -> bool {
    std::fs::read_to_string(power_state_path()).map(|t| state_offers_disk(&t)).unwrap_or(false)
}

/// The buttons shown, in order.
pub fn choices(hibernate: bool) -> Vec<Choice> {
    Choice::ALL.into_iter().filter(|c| *c != Choice::Hibernate || hibernate).collect()
}

/// What the commands depend on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    pub user: String,
    /// `rdwm-lock` is installed.
    pub rdwm_lock: bool,
    /// rdwm is running (its IPC socket exists) and `rdwmctl` is installed.
    pub rdwm: bool,
}

impl Env {
    pub fn detect() -> Env {
        let have = |t: &str| crate::controlcenter::sys::have(t);
        Env {
            user: std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default(),
            rdwm_lock: have("rdwm-lock"),
            rdwm: have("rdwmctl") && crate::menubar::rdwm::socket_path().exists(),
        }
    }
}

/// The command a choice runs.
pub fn command(choice: Choice, env: &Env) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match choice {
        Choice::Lock if env.rdwm_lock => v(&["rdwm-lock"]),
        Choice::Lock => v(&["loginctl", "lock-session"]),
        Choice::Logout if env.rdwm => v(&["rdwmctl", "action", "quit"]),
        Choice::Logout => v(&["loginctl", "terminate-user", &env.user]),
        Choice::Suspend => v(&["systemctl", "suspend"]),
        Choice::Hibernate => v(&["systemctl", "hibernate"]),
        Choice::Restart => v(&["systemctl", "reboot"]),
        Choice::Shutdown => v(&["systemctl", "poweroff"]),
    }
}

/// Starts `argv` detached in its own process group, no stdio. False if it could not be started.
pub fn spawn_detached(argv: &[String]) -> bool {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let Some((prog, args)) = argv.split_first() else { return false };
    Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().is_ok()
}

// ---------------------------------------------------------------- layout

/// A button rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub x: f32,
    pub y: f32,
    pub s: f32,
}

impl Cell {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.s && py >= self.y && py < self.y + self.s
    }
}

/// Button squares for `n` buttons, centred in a `w` x `h` window (slightly above the middle), shrunk to fit narrow screens.
pub fn layout(w: f32, h: f32, n: usize, scale: f32) -> Vec<Cell> {
    if n == 0 {
        return Vec::new();
    }
    let gap = GAP * scale;
    let avail = (w * 0.92 - gap * (n as f32 - 1.0)) / n as f32;
    let s = (BUTTON * scale).min(avail).max(24.0);
    let total = s * n as f32 + gap * (n as f32 - 1.0);
    let x0 = (w - total) / 2.0;
    let y = (h - (s + LABEL_H * scale)) / 2.0 - h * 0.04;
    (0..n).map(|i| Cell { x: x0 + i as f32 * (s + gap), y, s }).collect()
}

// ---------------------------------------------------------------- the app

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Run(Choice),
    Cancel,
}

pub struct Session {
    choices: Vec<Choice>,
    sel: usize,
    armed: Option<Choice>,
    outcome: Rc<RefCell<Outcome>>,
    size: (u32, u32),
    scale: f32,
    alpha: bool,
    /// Loaded after the first frame: finding and parsing a font costs more than the rest of the start-up together.
    fonts: Option<Fonts>,
    symbols: SymbolCache,
    /// When the process started; with `DECSESSION_TIMING` set, the time to the first painted frame is printed.
    started: Option<Instant>,
}

impl Session {
    pub fn new(choices: Vec<Choice>, outcome: Rc<RefCell<Outcome>>) -> Self {
        Self { choices, sel: 0, armed: None, outcome, size: (1920, 1080), scale: 1.0, alpha: false, fonts: None, symbols: SymbolCache::default(), started: None }
    }

    pub fn started_at(mut self, t: Instant) -> Self {
        self.started = Some(t);
        self
    }

    pub fn selected(&self) -> Option<Choice> {
        self.choices.get(self.sel).copied()
    }

    pub fn armed(&self) -> Option<Choice> {
        self.armed
    }

    fn cells(&self) -> Vec<Cell> {
        layout(self.size.0 as f32, self.size.1 as f32, self.choices.len(), self.scale)
    }

    fn cell_at(&self, x: f32, y: f32) -> Option<usize> {
        self.cells().iter().position(|c| c.contains(x, y))
    }

    fn finish(&mut self, o: Outcome) -> Response {
        *self.outcome.borrow_mut() = o;
        Response::act(Action::Close)
    }

    /// Chooses `choice`: runs it, or arms it when it is destructive and not armed yet.
    pub fn press(&mut self, choice: Choice) -> Response {
        if let Some(i) = self.choices.iter().position(|c| *c == choice) {
            self.sel = i;
        } else {
            return Response::NONE;
        }
        if choice.destructive() && self.armed != Some(choice) {
            self.armed = Some(choice);
            return Response::REDRAW;
        }
        self.finish(Outcome::Run(choice))
    }

    fn step(&mut self, delta: isize) -> Response {
        let n = self.choices.len() as isize;
        if n == 0 {
            return Response::NONE;
        }
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        self.armed = None;
        Response::REDRAW
    }

    #[allow(clippy::too_many_arguments)]
    fn text_centered(&mut self, pm: &mut Pixmap, text: &str, cx: f32, top: f32, px: f32, weight: Weight, c: Color) {
        if let Some(f) = &mut self.fonts {
            let w = f.measure(text, px, weight);
            f.draw(pm, text, cx - w / 2.0, top, px, weight, c);
        }
    }

    fn draw_all(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        // The dimmer: translucent black over the desktop, or (no compositor) an opaque darkened theme background.
        let scrim = if self.alpha { color(0, 0, 0, 0.62) } else { th.background.over(crate::theme::Rgb(0, 0, 0), 0.7).color(1.0) };
        pm.fill(Color::TRANSPARENT);
        fill_rect(pm, 0.0, 0.0, w, h, scrim);

        let cells = self.cells();
        let sc = self.scale;
        let white = |a: f32| color(255, 255, 255, a);
        for (i, ch) in self.choices.clone().into_iter().enumerate() {
            let c = cells[i];
            let selected = i == self.sel;
            let armed = self.armed == Some(ch);
            let radius = c.s * 0.26;
            let fill = if armed {
                th.danger.color(0.92)
            } else if selected {
                white(0.30)
            } else {
                white(0.16)
            };
            fill_rounded(pm, c.x, c.y, c.s, c.s, radius, fill);
            if selected && !armed {
                stroke_rounded(pm, c.x + 1.5, c.y + 1.5, c.s - 3.0, c.s - 3.0, radius - 1.5, th.accent_c(), 3.0);
            } else if !armed {
                stroke_rounded(pm, c.x + 0.5, c.y + 0.5, c.s - 1.0, c.s - 1.0, radius - 0.5, white(0.10), 1.0);
            }
            let icon = (c.s * 0.42).round();
            let px = (icon * 2.0).ceil() as u32;
            if let Some(sym) = self.symbols.get(ch.icon(), (255, 255, 255), px) {
                crate::gfx::blit_scaled(pm, sym, c.x + (c.s - icon) / 2.0, c.y + (c.s - icon) / 2.0, icon, icon, 1.0);
            }
            let cx = c.x + c.s / 2.0;
            let label = ch.label();
            self.text_centered(pm, label, cx, c.y + c.s + 12.0 * sc, 15.0 * sc, Weight::SemiBold, white(0.95));
            if armed {
                self.text_centered(pm, "Press again", cx, c.y + c.s + 31.0 * sc, 12.0 * sc, Weight::SemiBold, th.danger.over(crate::theme::Rgb(255, 255, 255), 0.45).color(1.0));
            } else {
                let hint = ch.key().to_ascii_uppercase().to_string();
                self.text_centered(pm, &hint, cx, c.y + c.s + 31.0 * sc, 12.0 * sc, Weight::SemiBold, white(0.55));
            }
        }
        if let (Some(first), Some(last)) = (cells.first(), cells.last()) {
            let mid = (first.x + last.x + last.s) / 2.0;
            let hint = "Esc to cancel";
            self.text_centered(pm, hint, mid, first.y + first.s + (LABEL_H + 28.0) * sc, 12.0 * sc, Weight::SemiBold, white(0.4));
        }
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }
}

impl WindowApp for Session {
    fn title(&self) -> String {
        "decsession".into()
    }

    fn app_id(&self) -> &'static str {
        "decsession"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn popup_fullscreen(&self) -> bool {
        true
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

    fn input_grabbed(&mut self, mods: Option<u16>) -> Response {
        if let (Some(t), Some(_)) = (self.started, std::env::var_os("DECSESSION_TIMING")) {
            eprintln!("decsession: first frame painted {} ms after start", t.elapsed().as_millis());
        }
        // Without the keyboard the menu could only be left with the mouse: do not hang around.
        match mods {
            None => self.finish(Outcome::Cancel),
            // The dimmer and the buttons are up; now add the labels.
            Some(_) => {
                self.fonts = Fonts::load();
                Response::REDRAW
            }
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        self.size
    }

    fn set_waker(&mut self, _waker: Waker) {}

    fn set_screen(&mut self, w: u32, h: u32) {
        if w > 0 && h > 0 {
            self.size = (w, h);
        }
    }

    fn set_scale(&mut self, scale: f32) {
        self.scale = scale.clamp(1.0, 4.0);
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w, h);
    }

    fn set_rounded(&mut self, _rounded: bool) {}

    fn draw(&mut self, pm: &mut Pixmap) {
        self.draw_all(pm);
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        match self.cell_at(x, y) {
            Some(i) if i != self.sel => {
                self.sel = i;
                self.armed = None;
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed || button != Button::Left {
            return Response::NONE;
        }
        match self.cell_at(x, y) {
            Some(i) => {
                let choice = self.choices[i];
                self.press(choice)
            }
            // A click on the dimmed background dismisses the menu.
            None => self.finish(Outcome::Cancel),
        }
    }

    fn scroll(&mut self, _x: f32, _y: f32, _dy: f32) -> Response {
        Response::NONE
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Escape => self.finish(Outcome::Cancel),
            Key::Enter => match self.selected() {
                Some(c) => self.press(c),
                None => Response::NONE,
            },
            Key::Right | Key::Down => self.step(1),
            Key::Left | Key::Up => self.step(-1),
            Key::Tab => self.step(if ev.shift { -1 } else { 1 }),
            Key::Home => {
                let d = -(self.sel as isize);
                self.step(d)
            }
            Key::End => {
                let d = self.choices.len() as isize - 1 - self.sel as isize;
                self.step(d)
            }
            Key::Char if !ev.ctrl && !ev.alt => match ev.ch.and_then(Choice::from_key) {
                Some(c) => self.press(c),
                None => Response::NONE,
            },
            _ => Response::NONE,
        }
    }

    fn tick(&mut self) -> Response {
        Response::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(hib: bool) -> (Session, Rc<RefCell<Outcome>>) {
        let o = Rc::new(RefCell::new(Outcome::Pending));
        let mut s = Session::new(choices(hib), o.clone());
        s.resize(1920, 1080);
        (s, o)
    }

    fn k(key: Key) -> KeyEvent {
        KeyEvent { key, text: String::new(), ch: None, ctrl: false, shift: false, alt: false }
    }

    fn ch(c: char) -> KeyEvent {
        KeyEvent { key: Key::Char, text: c.to_string(), ch: Some(c.to_ascii_lowercase()), ctrl: false, shift: c.is_uppercase(), alt: false }
    }

    fn env() -> Env {
        Env { user: "ann".into(), rdwm_lock: true, rdwm: false }
    }

    #[test]
    fn hibernate_only_when_the_kernel_offers_disk() {
        assert!(state_offers_disk("freeze mem disk\n"));
        assert!(!state_offers_disk("freeze mem\n"));
        assert!(!state_offers_disk("diskette"));
        assert_eq!(choices(true).len(), 6);
        assert!(!choices(false).contains(&Choice::Hibernate));
        assert_eq!(choices(false).len(), 5);
    }

    #[test]
    fn commands() {
        let e = env();
        assert_eq!(command(Choice::Lock, &e), ["rdwm-lock"]);
        assert_eq!(command(Choice::Lock, &Env { rdwm_lock: false, ..e.clone() }), ["loginctl", "lock-session"]);
        assert_eq!(command(Choice::Logout, &e), ["loginctl", "terminate-user", "ann"]);
        assert_eq!(command(Choice::Logout, &Env { rdwm: true, ..e.clone() }), ["rdwmctl", "action", "quit"]);
        assert_eq!(command(Choice::Suspend, &e), ["systemctl", "suspend"]);
        assert_eq!(command(Choice::Hibernate, &e), ["systemctl", "hibernate"]);
        assert_eq!(command(Choice::Restart, &e), ["systemctl", "reboot"]);
        assert_eq!(command(Choice::Shutdown, &e), ["systemctl", "poweroff"]);
    }

    #[test]
    fn shortcut_letters_are_unique_and_parse_back() {
        let mut keys: Vec<char> = Choice::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(keys, ['l', 'o', 's', 'h', 'r', 'p']);
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 6);
        assert_eq!(Choice::from_key('P'), Some(Choice::Shutdown));
        assert_eq!(Choice::from_key('x'), None);
        for c in Choice::ALL {
            assert_eq!(Choice::parse(c.name()), Some(c));
        }
    }

    #[test]
    fn lock_and_suspend_run_at_once() {
        for (key, want) in [('l', Choice::Lock), ('s', Choice::Suspend), ('h', Choice::Hibernate)] {
            let (mut s, o) = session(true);
            let r = s.key(&ch(key));
            assert_eq!(r.action, Some(Action::Close));
            assert_eq!(*o.borrow(), Outcome::Run(want));
        }
    }

    #[test]
    fn destructive_choices_need_a_second_press() {
        for (key, want) in [('o', Choice::Logout), ('r', Choice::Restart), ('p', Choice::Shutdown)] {
            let (mut s, o) = session(true);
            let r = s.key(&ch(key));
            assert_eq!(r, Response::REDRAW, "{key}: first press only arms");
            assert_eq!(s.armed(), Some(want));
            assert_eq!(*o.borrow(), Outcome::Pending);
            let r = s.key(&ch(key.to_ascii_uppercase()));
            assert_eq!(r.action, Some(Action::Close), "{key}: second press runs");
            assert_eq!(*o.borrow(), Outcome::Run(want));
        }
    }

    #[test]
    fn arming_then_choosing_something_else_disarms() {
        let (mut s, o) = session(true);
        s.key(&ch('p'));
        assert_eq!(s.armed(), Some(Choice::Shutdown));
        s.key(&ch('r'));
        assert_eq!(s.armed(), Some(Choice::Restart), "re-armed on the new button, shutdown did not run");
        assert_eq!(*o.borrow(), Outcome::Pending);
        s.key(&k(Key::Left));
        assert_eq!(s.armed(), None, "moving the selection disarms");
        assert_eq!(*o.borrow(), Outcome::Pending);
    }

    #[test]
    fn arrows_tab_home_end_and_return() {
        let (mut s, o) = session(false);
        assert_eq!(s.selected(), Some(Choice::Lock));
        s.key(&k(Key::Right));
        assert_eq!(s.selected(), Some(Choice::Logout));
        s.key(&k(Key::Left));
        s.key(&k(Key::Left));
        assert_eq!(s.selected(), Some(Choice::Shutdown), "wraps around");
        s.key(&k(Key::Home));
        assert_eq!(s.selected(), Some(Choice::Lock));
        s.key(&k(Key::End));
        assert_eq!(s.selected(), Some(Choice::Shutdown));
        s.key(&KeyEvent { shift: true, ..k(Key::Tab) });
        assert_eq!(s.selected(), Some(Choice::Restart));
        s.key(&k(Key::Tab));
        assert_eq!(s.selected(), Some(Choice::Shutdown));
        // Return on a destructive choice arms first.
        assert_eq!(s.key(&k(Key::Enter)), Response::REDRAW);
        assert_eq!(*o.borrow(), Outcome::Pending);
        assert_eq!(s.key(&k(Key::Enter)).action, Some(Action::Close));
        assert_eq!(*o.borrow(), Outcome::Run(Choice::Shutdown));
    }

    #[test]
    fn escape_cancels_even_when_armed() {
        let (mut s, o) = session(true);
        s.key(&ch('p'));
        let r = s.key(&k(Key::Escape));
        assert_eq!(r.action, Some(Action::Close));
        assert_eq!(*o.borrow(), Outcome::Cancel);
    }

    #[test]
    fn a_missing_hibernate_button_ignores_its_key() {
        let (mut s, o) = session(false);
        assert_eq!(s.key(&ch('h')), Response::NONE);
        assert_eq!(*o.borrow(), Outcome::Pending);
    }

    #[test]
    fn modified_and_unknown_keys_do_nothing() {
        let (mut s, o) = session(true);
        assert_eq!(s.key(&KeyEvent { ctrl: true, ..ch('l') }), Response::NONE);
        assert_eq!(s.key(&ch('x')), Response::NONE);
        assert_eq!(*o.borrow(), Outcome::Pending);
    }

    #[test]
    fn clicks_follow_the_same_confirm_flow() {
        let (mut s, o) = session(true);
        let cells = s.cells();
        let at = |i: usize| (cells[i].x + cells[i].s / 2.0, cells[i].y + cells[i].s / 2.0);
        let (x, y) = at(5); // Shut Down
        assert_eq!(s.pointer_button(x, y, Button::Left, true), Response::REDRAW);
        assert_eq!(s.armed(), Some(Choice::Shutdown));
        assert_eq!(s.pointer_button(x, y, Button::Left, false), Response::NONE, "release does nothing");
        // Hovering another button disarms.
        let (x2, y2) = at(2);
        s.pointer_motion(x2, y2);
        assert_eq!(s.armed(), None);
        s.pointer_button(x, y, Button::Left, true);
        assert_eq!(*o.borrow(), Outcome::Pending, "armed again, not run");
        assert_eq!(s.pointer_button(x, y, Button::Left, true).action, Some(Action::Close));
        assert_eq!(*o.borrow(), Outcome::Run(Choice::Shutdown));
    }

    #[test]
    fn click_on_lock_runs_and_background_click_cancels() {
        let (mut s, o) = session(true);
        let c = s.cells()[0];
        assert_eq!(s.pointer_button(c.x + 5.0, c.y + 5.0, Button::Left, true).action, Some(Action::Close));
        assert_eq!(*o.borrow(), Outcome::Run(Choice::Lock));
        let (mut s, o) = session(true);
        assert_eq!(s.pointer_button(3.0, 3.0, Button::Left, true).action, Some(Action::Close));
        assert_eq!(*o.borrow(), Outcome::Cancel);
        let (mut s, o) = session(true);
        assert_eq!(s.pointer_button(3.0, 3.0, Button::Right, true), Response::NONE);
        assert_eq!(*o.borrow(), Outcome::Pending);
    }

    #[test]
    fn grab_failure_cancels() {
        let (mut s, o) = session(true);
        assert_eq!(s.input_grabbed(None).action, Some(Action::Close));
        assert_eq!(*o.borrow(), Outcome::Cancel);
        let (mut s, _) = session(true);
        assert_eq!(s.input_grabbed(Some(0)), Response::REDRAW, "labels are added once the grab worked");
    }

    #[test]
    fn layout_is_centred_in_order_and_inside_the_screen() {
        for (w, n) in [(1920.0, 6), (1920.0, 5), (800.0, 6), (320.0, 6), (1280.0, 1)] {
            let cells = layout(w, 600.0, n, 1.0);
            assert_eq!(cells.len(), n);
            let first = cells[0];
            let last = cells[n - 1];
            assert!(first.x >= 0.0 && last.x + last.s <= w, "{w}/{n}: inside");
            assert!((first.x + (last.x + last.s - first.x) / 2.0 - w / 2.0).abs() < 0.5, "{w}/{n}: centred");
            for p in cells.windows(2) {
                assert!(p[1].x >= p[0].x + p[0].s, "{w}/{n}: no overlap");
                assert_eq!(p[0].y, p[1].y);
            }
            assert!(first.y > 0.0 && first.y + first.s < 600.0);
        }
        assert_eq!(layout(1920.0, 1080.0, 6, 1.0)[0].s, BUTTON);
        assert!(layout(1920.0, 1080.0, 6, 2.0)[0].s > BUTTON);
        assert!(layout(100.0, 100.0, 0, 1.0).is_empty());
    }

    #[test]
    fn draws_something_dim_and_opaque_without_a_compositor() {
        let (mut s, _) = session(true);
        s.resize(640, 360);
        let mut pm = Pixmap::new(640, 360).unwrap();
        s.draw(&mut pm);
        assert!(pm.data().chunks_exact(4).all(|p| p[3] == 255), "opaque scrim");
        let corner = &pm.data()[0..4];
        assert!(corner[0] < 40 && corner[1] < 40 && corner[2] < 40, "dark corner {corner:?}");
        let c = s.cells()[0];
        let i = (((c.y + c.s / 2.0) as usize) * 640 + (c.x + 4.0) as usize) * 4;
        assert!(pm.data()[i] > corner[0] + 30, "button is lighter than the scrim");
    }
}
