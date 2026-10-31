//! The dechot daemon: tiny InputOnly windows in the corners of every monitor and a dwell timer.
//!
//! No polling: the process sleeps in `poll(2)` on the X socket and an inotify descriptor (the config file). The
//! only timeouts are a running dwell (150 ms by default), the debounce of a restack and, when inotify is not
//! available, a slow check of the config file's modification time.
//!
//! The windows are override-redirect and select only Enter/LeaveNotify: no grabs, no focus, no input handling. They are
//! kept above every other window (a cheap re-check, debounced, after windows map or restack).

use super::{corner_spots, in_path, Action, Check, Command, Corner, HotConfig, Machine, Rect, Spot, COOLDOWN_MS, CORNER_PX, LOCK_NAME};
use crate::config::Stamp;
use std::collections::HashSet;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// How soon after a map/restack of some other window the corner windows are checked to still be on top.
const RAISE_DEBOUNCE_MS: u64 = 100;
/// Config re-check interval when inotify cannot watch the directory.
const STAMP_POLL_MS: u64 = 3000;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn buttons_down(mask: KeyButMask) -> bool {
    let any = u16::from(KeyButMask::BUTTON1) | u16::from(KeyButMask::BUTTON2) | u16::from(KeyButMask::BUTTON3) | u16::from(KeyButMask::BUTTON4) | u16::from(KeyButMask::BUTTON5);
    u16::from(mask) & any != 0
}

/// Starts `cmd` through `sh -c`, detached (own process group, no stdio); never blocks, never waits.
pub fn spawn(cmd: &Command) -> bool {
    if let Some(prog) = cmd.needs {
        if !in_path(prog) {
            eprintln!("dechot: {prog} is not in PATH, nothing to run");
            return false;
        }
    }
    let child =
        std::process::Command::new("sh").arg("-c").arg(&cmd.line).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).process_group(0).spawn();
    match child {
        Ok(mut c) => {
            // Reaped on its own thread, so no zombie stays behind and the event loop never waits.
            std::thread::spawn(move || {
                let _ = c.wait();
            });
            true
        }
        Err(e) => {
            eprintln!("dechot: cannot run {:?}: {e}", cmd.line);
            false
        }
    }
}

// ---------------------------------------------------------------- inotify on the config directory

struct Watch {
    fd: RawFd,
    name: String,
}

impl Watch {
    fn new(dir: &std::path::Path, name: &str) -> Option<Watch> {
        let c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).ok()?;
        // SAFETY: plain inotify syscalls; the descriptor is owned by the returned value for the life of the process.
        unsafe {
            let fd = libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK);
            if fd < 0 {
                return None;
            }
            let mask = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM;
            if libc::inotify_add_watch(fd, c.as_ptr(), mask) < 0 {
                libc::close(fd);
                return None;
            }
            Some(Watch { fd, name: name.to_string() })
        }
    }

    /// Reads everything pending; true when an event named after the config file was among it.
    fn drain(&self) -> bool {
        let mut hit = false;
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: read(2) into a stack buffer of the length passed.
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                return hit;
            }
            let mut off = 0usize;
            let n = n as usize;
            while off + 16 <= n {
                let len = u32::from_ne_bytes(buf[off + 12..off + 16].try_into().unwrap_or_default()) as usize;
                let end = (off + 16 + len).min(n);
                let name = &buf[off + 16..end];
                let name = name.split(|b| *b == 0).next().unwrap_or_default();
                hit |= name == self.name.as_bytes();
                off = off + 16 + len;
            }
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: closing the descriptor this value owns.
        unsafe { libc::close(self.fd) };
    }
}

fn wait_readable(fds: &[RawFd], timeout: Option<Duration>) {
    let mut p: Vec<libc::pollfd> = fds.iter().map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    let ms = timeout.map_or(-1, |d| i32::try_from(d.as_millis() + 1).unwrap_or(i32::MAX));
    // SAFETY: poll(2) on a slice of valid pollfds that lives on the stack; it only reads `fd` and writes `revents`.
    unsafe { libc::poll(p.as_mut_ptr(), p.len() as libc::nfds_t, ms) };
}

// ---------------------------------------------------------------- the daemon

struct Corners {
    spot: Spot,
    win: Window,
}

struct Daemon {
    conn: RustConnection,
    root: Window,
    cfg: HotConfig,
    path: PathBuf,
    stamp: Stamp,
    watch: Option<Watch>,
    start: Instant,
    /// What the current windows were built for: monitors and corners.
    built_for: (Vec<Rect>, Vec<Corner>),
    wins: Vec<Corners>,
    machine: Machine,
    raise_at: Option<u64>,
    stamp_at: u64,
}

impl Daemon {
    fn new() -> Result<Daemon, String> {
        let (conn, n) = crate::x11_connection::connect(None).map_err(err)?;
        let root = conn.setup().roots[n].root;
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY)).map_err(err)?.check().map_err(err)?;
        // RandR 1.5 monitors; without the extension the screen is the one monitor.
        let _ = conn.randr_select_input(root, randr::NotifyMask::SCREEN_CHANGE | randr::NotifyMask::CRTC_CHANGE | randr::NotifyMask::OUTPUT_CHANGE);
        let path = super::config_path();
        let watch = path.parent().and_then(|d| Watch::new(d, super::CONFIG_FILE));
        let cfg = HotConfig::load_from(path.parent().unwrap_or(std::path::Path::new(".")));
        let machine = Machine::new(0, cfg.dwell_ms, COOLDOWN_MS);
        let mut d =
            Daemon { conn, root, stamp: Stamp::of(&path), cfg, path, watch, start: Instant::now(), built_for: (Vec::new(), Vec::new()), wins: Vec::new(), machine, raise_at: None, stamp_at: 0 };
        d.relayout(true)?;
        Ok(d)
    }

    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn monitors(&self) -> Vec<Rect> {
        let mons: Vec<Rect> = self
            .conn
            .randr_get_monitors(self.root, true)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.monitors.iter().filter(|m| m.width > 0 && m.height > 0).map(|m| Rect::new(i32::from(m.x), i32::from(m.y), i32::from(m.width), i32::from(m.height))).collect())
            .unwrap_or_default();
        if !mons.is_empty() {
            return mons;
        }
        self.conn.get_geometry(self.root).ok().and_then(|c| c.reply().ok()).map(|g| vec![Rect::new(0, 0, i32::from(g.width), i32::from(g.height))]).unwrap_or_default()
    }

    fn wanted(&self) -> Vec<Corner> {
        Corner::ALL.into_iter().filter(|c| *self.cfg.action(*c) != Action::None).collect()
    }

    fn destroy_windows(&mut self) {
        for w in self.wins.drain(..) {
            let _ = self.conn.destroy_window(w.win);
        }
    }

    /// Builds the corner windows for the current monitors and configuration (only when something changed, or `force`).
    fn relayout(&mut self, force: bool) -> Result<(), String> {
        let key = (self.monitors(), self.wanted());
        if !force && key == self.built_for {
            return Ok(());
        }
        self.destroy_windows();
        let spots = corner_spots(&key.0, CORNER_PX, &key.1);
        for spot in &spots {
            let win = self.conn.generate_id().map_err(err)?;
            let r = spot.rect;
            let aux = CreateWindowAux::new().override_redirect(1).event_mask(EventMask::ENTER_WINDOW | EventMask::LEAVE_WINDOW);
            self.conn.create_window(0, win, self.root, r.x as i16, r.y as i16, r.w as u16, r.h as u16, 0, WindowClass::INPUT_ONLY, 0, &aux).map_err(err)?;
            let _ = self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"dechot corner");
            let _ = self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"dechot\0Dechot\0");
            self.conn.map_window(win).map_err(err)?;
            self.wins.push(Corners { spot: *spot, win });
        }
        self.machine = Machine::new(self.wins.len(), self.cfg.dwell_ms, COOLDOWN_MS);
        // A pointer that is already in a corner (the corner was just switched on, or the monitors changed) does not count as entering.
        if let Some(p) = self.conn.query_pointer(self.root).ok().and_then(|c| c.reply().ok()) {
            for (i, w) in self.wins.iter().enumerate() {
                if w.spot.rect.contains(i32::from(p.root_x), i32::from(p.root_y)) {
                    self.machine.suppress(i);
                }
            }
        }
        self.built_for = key;
        self.conn.flush().map_err(err)?;
        Ok(())
    }

    /// Re-reads the config file; rebuilds the windows only when the set of active corners changed.
    fn reload(&mut self) {
        let dir = self.path.parent().map(PathBuf::from).unwrap_or_default();
        let cfg = HotConfig::load_from(&dir);
        self.stamp = Stamp::of(&self.path);
        if cfg == self.cfg {
            return;
        }
        self.cfg = cfg;
        self.machine.set_dwell(self.cfg.dwell_ms);
        if let Err(e) = self.relayout(false) {
            eprintln!("dechot: {e}");
        }
    }

    fn slot_of(&self, win: Window) -> Option<usize> {
        self.wins.iter().position(|w| w.win == win)
    }

    /// Makes sure our windows are the topmost children of the root.
    fn raise(&self) {
        let Some(tree) = self.conn.query_tree(self.root).ok().and_then(|c| c.reply().ok()) else { return };
        let ours: HashSet<Window> = self.wins.iter().map(|w| w.win).collect();
        let n = ours.len();
        if n == 0 || tree.children.iter().rev().take(n).all(|c| ours.contains(c)) {
            return;
        }
        for w in &self.wins {
            let _ = self.conn.configure_window(w.win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        }
        let _ = self.conn.flush();
    }

    fn handle(&mut self, ev: Event, now: u64) -> Result<(), String> {
        match ev {
            Event::EnterNotify(e) if e.mode == NotifyMode::NORMAL => {
                if let Some(slot) = self.slot_of(e.event) {
                    self.machine.enter(slot, now, buttons_down(e.state));
                }
            }
            Event::LeaveNotify(e) => {
                if let Some(slot) = self.slot_of(e.event) {
                    self.machine.leave(slot, now);
                }
            }
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => self.relayout(false)?,
            Event::MapNotify(e) if self.slot_of(e.window).is_none() => self.want_raise(now),
            Event::ConfigureNotify(e) if self.slot_of(e.window).is_none() => self.want_raise(now),
            Event::CirculateNotify(e) if self.slot_of(e.window).is_none() => self.want_raise(now),
            _ => {}
        }
        Ok(())
    }

    fn want_raise(&mut self, now: u64) {
        if !self.wins.is_empty() && self.raise_at.is_none() {
            self.raise_at = Some(now + RAISE_DEBOUNCE_MS);
        }
    }

    /// Fires every corner whose dwell ran out.
    fn fire_due(&mut self, now: u64) {
        let (conn, root) = (&self.conn, self.root);
        let wins = &self.wins;
        let due = self.machine.poll(now, |slot| {
            let Some(p) = conn.query_pointer(root).ok().and_then(|c| c.reply().ok()) else { return Check::Gone };
            if !wins[slot].spot.rect.contains(i32::from(p.root_x), i32::from(p.root_y)) {
                Check::Gone
            } else if buttons_down(p.mask) {
                Check::Held
            } else {
                Check::Fire
            }
        });
        for slot in due {
            let corner = self.wins[slot].spot.corner;
            if let Some(cmd) = self.cfg.action(corner).command() {
                spawn(&cmd);
            }
        }
    }

    fn run(&mut self) -> Result<(), String> {
        let xfd = self.conn.stream().as_raw_fd();
        loop {
            let now = self.now();
            while let Some(ev) = self.conn.poll_for_event().map_err(err)? {
                self.handle(ev, now)?;
            }
            let now = self.now();
            if self.raise_at.is_some_and(|t| now >= t) {
                self.raise_at = None;
                self.raise();
            }
            self.fire_due(now);
            // Config: inotify when we have it, else a slow look at the modification time.
            let mut deadline = [self.machine.deadline(), self.raise_at].into_iter().flatten().min();
            if self.watch.is_none() {
                if now >= self.stamp_at {
                    self.stamp_at = now + STAMP_POLL_MS;
                    if Stamp::of(&self.path) != self.stamp {
                        self.reload();
                    }
                }
                deadline = Some(deadline.map_or(self.stamp_at, |d| d.min(self.stamp_at)));
            }
            self.conn.flush().map_err(err)?;
            let timeout = deadline.map(|t| Duration::from_millis(t.saturating_sub(self.now())));
            let mut fds = vec![xfd];
            fds.extend(self.watch.as_ref().map(|w| w.fd));
            wait_readable(&fds, timeout);
            if self.watch.as_ref().is_some_and(Watch::drain) {
                self.reload();
            }
        }
    }
}

/// Runs the daemon until the X connection ends. Only one runs per session: a second one returns at once.
pub fn run() -> Result<(), String> {
    let Some(_lock) = crate::single::acquire_signalling(LOCK_NAME, 0) else {
        eprintln!("dechot: the daemon is already running");
        return Ok(());
    };
    Daemon::new()?.run()
}
