//! The X11 side of the lock: one fullscreen override-redirect window over the whole root (every monitor), the
//! keyboard and pointer grabbed for as long as it lives, kept on top, and a small event loop that sleeps in `poll`
//! (a one-second tick for the clock and a ~60 Hz tick only while the wrong-password shake plays).

use super::auth::Authenticator;
use super::bg;
use super::state::{Input, LockState, Outcome};
use super::view::{panel_size, scale_for, Painter, View};
use super::{face_path, BgMode, Config};
use crate::keysym::keysym_char;
use crate::menubar::clock;
use std::os::fd::AsRawFd;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// How long the grabs are retried (another client may hold one for a moment, e.g. a closing menu).
pub const GRAB_TIMEOUT: Duration = Duration::from_secs(5);
const BAND_ROWS: u16 = 64;

/// Why the lock could not be established; the caller exits non-zero (nothing is left on screen).
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<E: std::fmt::Display>(e: E) -> Error {
    Error(e.to_string())
}

// ---------------------------------------------------------------- keyboard

const SHIFT: u16 = 1;
const LOCK: u16 = 1 << 1;
const CONTROL: u16 = 1 << 2;
const MOD1: u16 = 1 << 3;
const MOD2: u16 = 1 << 4; // Num Lock
const MOD4: u16 = 1 << 6;
const MOD5: u16 = 1 << 7; // ISO_Level3_Shift (AltGr)

pub const KEYSYM_CAPS_LOCK: u32 = 0xffe5;

struct Keymap {
    min_keycode: u8,
    per_keycode: usize,
    syms: Vec<u32>,
}

impl Keymap {
    fn load(c: &RustConnection) -> Result<Keymap, Error> {
        let (min, max) = (c.setup().min_keycode, c.setup().max_keycode);
        let r = c.get_keyboard_mapping(min, max - min + 1).map_err(err)?.reply().map_err(err)?;
        Ok(Keymap { min_keycode: min, per_keycode: r.keysyms_per_keycode as usize, syms: r.keysyms })
    }

    fn column(&self, keycode: u8, col: usize) -> u32 {
        if keycode < self.min_keycode || col >= self.per_keycode {
            return 0;
        }
        self.syms.get((keycode - self.min_keycode) as usize * self.per_keycode + col).copied().unwrap_or(0)
    }

    /// Keysym of `keycode` for the modifier `state` (Shift, Caps Lock on letters, Num Lock on the keypad, AltGr).
    fn lookup(&self, keycode: u8, state: u16) -> u32 {
        let base = if state & MOD5 != 0 && self.column(keycode, 4) != 0 { 4 } else { 0 };
        let plain = self.column(keycode, base);
        let shifted = match self.column(keycode, base + 1) {
            0 => plain,
            s => s,
        };
        let keypad = (0xff95..=0xff9f).contains(&plain);
        let shift = (state & SHIFT != 0) ^ (keypad && state & MOD2 != 0);
        let sym = if shift { shifted } else { plain };
        if state & LOCK != 0 && state & SHIFT == 0 {
            if let Some(c) = keysym_char(plain).filter(|c| c.is_alphabetic()) {
                return crate::keysym::char_keysym(c.to_uppercase().next().unwrap_or(c)).unwrap_or(sym);
            }
        }
        sym
    }
}

/// What a key press means for the lock. Everything with Ctrl/Alt/Super held is ignored except Ctrl+U / Ctrl+Backspace
/// (clear); function, media (XF86*) and modifier keys produce nothing.
pub fn decode_key(sym: u32, plain: u32, state: u16) -> Option<Input> {
    let ctrl = state & CONTROL != 0;
    if ctrl && (plain == 0xff08 || keysym_char(plain).is_some_and(|c| c.eq_ignore_ascii_case(&'u'))) {
        return Some(Input::Clear);
    }
    if state & (CONTROL | MOD1 | MOD4) != 0 {
        return None;
    }
    match sym {
        0xff0d | 0xff8d => Some(Input::Submit),
        0xff08 => Some(Input::Backspace),
        0xff1b => Some(Input::Clear),
        _ => keysym_char(sym).filter(|c| !c.is_control()).map(Input::Char),
    }
}

// ---------------------------------------------------------------- the screen

struct Monitor {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

fn primary_monitor(c: &RustConnection, root: Window, size: (u32, u32)) -> Monitor {
    let whole = Monitor { x: 0, y: 0, w: size.0, h: size.1 };
    let Ok(reply) = c.randr_get_monitors(root, true).map_err(err).and_then(|r| r.reply().map_err(err)) else { return whole };
    reply.monitors.iter().find(|m| m.primary).or(reply.monitors.first()).filter(|m| m.width > 0 && m.height > 0).map_or(whole, |m| Monitor {
        x: i32::from(m.x),
        y: i32::from(m.y),
        w: u32::from(m.width),
        h: u32::from(m.height),
    })
}

/// A pixel copy of the root, a band at a time, reduced to backdrop size. `None` when the format is not 32-bit BGRX.
fn capture(c: &RustConnection, root: Window, w: u16, h: u16, depth: u8) -> Option<Pixmap> {
    let setup = c.setup();
    let fmt = setup.pixmap_formats.iter().find(|f| f.depth == depth)?;
    if fmt.bits_per_pixel != 32 || setup.image_byte_order != ImageOrder::LSB_FIRST {
        return None;
    }
    let mut d = bg::Downscaler::new(u32::from(w), u32::from(h));
    let mut y = 0u16;
    while y < h {
        let rows = BAND_ROWS.min(h - y);
        let r = c.get_image(ImageFormat::Z_PIXMAP, root, 0, y as i16, w, rows, !0).ok()?.reply().ok()?;
        if r.data.len() < w as usize * rows as usize * 4 {
            return None;
        }
        d.add_rows(y as usize, &r.data);
        y += rows;
    }
    d.finish()
}

fn make_backdrop(c: &RustConnection, cfg: &Config, root: Window, w: u16, h: u16, depth: u8) -> Pixmap {
    let (w32, h32) = (u32::from(w), u32::from(h));
    let wallpaper = || {
        let paths = crate::settings::io::Paths::discover();
        crate::theme::dynamic::find_wallpaper(&paths).and_then(|p| bg::from_image(&p, w32, h32))
    };
    let raw = match cfg.bg {
        BgMode::Screenshot => capture(c, root, w, h, depth).or_else(wallpaper),
        BgMode::Wallpaper => wallpaper(),
        BgMode::Color => None,
    };
    let raw = raw.or_else(|| bg::solid(w32, h32, crate::theme::th().background.t())).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"));
    crate::mem::trim();
    bg::finish(raw, cfg.blur, cfg.dim)
}

fn blank_cursor(c: &RustConnection, root: Window) -> Option<Cursor> {
    let pm = c.generate_id().ok()?;
    c.create_pixmap(1, pm, root, 1, 1).ok()?;
    let cur = c.generate_id().ok()?;
    c.create_cursor(cur, pm, pm, 0, 0, 0, 0, 0, 0, 0, 0).ok()?;
    c.free_pixmap(pm).ok()?;
    Some(cur)
}

/// Separable fixed-point bilinear expansion of the small, already blurred backdrop.
/// Avoids a general affine rasterizer over millions of pixels at lock startup.
fn expand_backdrop(src: &Pixmap, w: u32, h: u32) -> Pixmap {
    let axis = |out: u32, input: u32| -> Vec<(usize, usize, u32)> {
        (0..out)
            .map(|i| {
                let pos = ((i as f64 + 0.5) * f64::from(input) / f64::from(out) - 0.5).clamp(0.0, f64::from(input - 1));
                let a = pos.floor() as usize;
                (a, (a + 1).min(input as usize - 1), ((pos - a as f64) * 256.0).round() as u32)
            })
            .collect()
    };
    let xs = axis(w, src.width());
    let ys = axis(h, src.height());
    let mut horizontal = vec![[0u32; 4]; w as usize * src.height() as usize];
    for (sy, row) in horizontal.chunks_exact_mut(w as usize).enumerate() {
        for (out, &(a, b, weight)) in row.iter_mut().zip(&xs) {
            let a = &src.data()[(sy * src.width() as usize + a) * 4..][..4];
            let b = &src.data()[(sy * src.width() as usize + b) * 4..][..4];
            for c in 0..4 {
                out[c] = u32::from(a[c]) * (256 - weight) + u32::from(b[c]) * weight;
            }
        }
    }
    let mut dst = Pixmap::new(w, h).expect("backdrop pixmap");
    for (row, &(a, b, weight)) in dst.data_mut().chunks_exact_mut(w as usize * 4).zip(&ys) {
        let ar = &horizontal[a * w as usize..][..w as usize];
        let br = &horizontal[b * w as usize..][..w as usize];
        for ((out, a), b) in row.chunks_exact_mut(4).zip(ar).zip(br) {
            for c in 0..4 {
                out[c] = ((a[c] * (256 - weight) + b[c] * weight + 32768) >> 16) as u8;
            }
        }
    }
    dst
}

struct Ui {
    w: u16,
    h: u16,
    depth: u8,
    small: Pixmap,
    panel: Pixmap,
    backdrop: Pixmap,
    panel_backdrop: Pixmap,
    static_panel: Pixmap,
    damage_y: u32,
    px: i32,
    py: i32,
    painter: Painter,
    last: Option<View>,
}

impl Ui {
    fn layout(c: &RustConnection, cfg: &Config, root: Window, (w, h): (u16, u16), small: Pixmap, depth: u8) -> Ui {
        let mon = primary_monitor(c, root, (u32::from(w), u32::from(h)));
        let scale = scale_for(mon.h);
        let (pw, ph) = panel_size(scale);
        let px = mon.x + (mon.w as i32 - pw as i32) / 2;
        let py = mon.y + (mon.h as i32 - ph as i32) / 2 - (mon.h as f32 * 0.03) as i32;
        let painter = Painter::new(scale, &cfg.user.display, face_path(&cfg.user.home).as_deref());
        let backdrop = expand_backdrop(&small, u32::from(w), u32::from(h));
        let mut panel_backdrop = Pixmap::new(pw, ph).expect("panel backdrop");
        panel_backdrop.draw_pixmap(-px, -py, backdrop.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        Ui { w, h, depth, small, backdrop, damage_y: 0, static_panel: panel_backdrop.clone(), panel_backdrop, panel: Pixmap::new(pw, ph).expect("panel pixmap"), px, py, painter, last: None }
    }

    /// Draws the backdrop and the panel for `view` into the panel pixmap.
    fn render(&mut self, view: &View) {
        let clock_lines = |v: &View| v.time.as_ref().map(|t| super::view::lines(t, v.hour12));
        let redraw_static = self.last.as_ref().is_none_or(|last| clock_lines(last) != clock_lines(view));
        self.damage_y = if redraw_static { 0 } else { (390.0 * self.painter.scale()).floor() as u32 };
        if redraw_static {
            self.static_panel.data_mut().copy_from_slice(self.panel_backdrop.data());
            self.painter.draw_static(&mut self.static_panel, view);
        }
        self.panel.data_mut().copy_from_slice(self.static_panel.data());
        self.painter.draw_input(&mut self.panel, view);
        self.last = Some(view.clone());
    }

    /// Sends the region (backdrop, panel on top) to the window.
    #[allow(clippy::too_many_arguments)]
    fn paint(&self, c: &RustConnection, win: Window, gc: Gcontext, x: i32, y: i32, w: u32, h: u32) -> Result<(), Error> {
        let (x0, y0) = (x.clamp(0, i32::from(self.w)), y.clamp(0, i32::from(self.h)));
        let (x1, y1) = ((x + w as i32).clamp(0, i32::from(self.w)), (y + h as i32).clamp(0, i32::from(self.h)));
        if x1 <= x0 || y1 <= y0 {
            return Ok(());
        }
        let max = c.maximum_request_bytes().saturating_sub(64);
        let rows_per = ((max / ((x1 - x0) as usize * 4)).max(1) as i32).min(256);
        let mut y = y0;
        while y < y1 {
            let rows = rows_per.min(y1 - y);
            let width = (x1 - x0) as usize;
            let mut bgrx = vec![0u8; width * rows as usize * 4];
            for row in 0..rows {
                let sy = y + row;
                let dst = &mut bgrx[row as usize * width * 4..(row as usize + 1) * width * 4];
                let offset = (sy as usize * usize::from(self.w) + x0 as usize) * 4;
                dst.copy_from_slice(&self.backdrop.data()[offset..offset + width * 4]);
                let py = sy - self.py;
                let left = x0.max(self.px);
                let right = x1.min(self.px + self.panel.width() as i32);
                if py >= 0 && py < self.panel.height() as i32 && right > left {
                    let from = (py as usize * self.panel.width() as usize + (left - self.px) as usize) * 4;
                    let to = (left - x0) as usize * 4;
                    let bytes = (right - left) as usize * 4;
                    dst[to..to + bytes].copy_from_slice(&self.panel.data()[from..from + bytes]);
                }
            }
            for p in bgrx.chunks_exact_mut(4) {
                p.swap(0, 2);
                p[3] = 0xff;
            }
            c.put_image(ImageFormat::Z_PIXMAP, win, gc, (x1 - x0) as u16, rows as u16, x0 as i16, y as i16, 0, self.depth, &bgrx).map_err(err)?;
            y += rows;
        }
        Ok(())
    }

    fn paint_all(&self, c: &RustConnection, win: Window, gc: Gcontext) -> Result<(), Error> {
        self.paint(c, win, gc, 0, 0, u32::from(self.w), u32::from(self.h))
    }

    fn paint_panel(&self, c: &RustConnection, win: Window, gc: Gcontext) -> Result<(), Error> {
        self.paint(c, win, gc, self.px, self.py + self.damage_y as i32, self.panel.width(), self.panel.height().saturating_sub(self.damage_y))
    }
}

// ---------------------------------------------------------------- the lock

type Verdict = Result<bool, String>;

/// Locks the screen and returns once the right password was entered (the grabs are released first).
/// `Err` means the lock could not be established (no grab); nothing is left on screen in that case.
pub fn run(cfg: &Config, auth: Arc<dyn Authenticator>) -> Result<(), Error> {
    let (conn, num) = crate::x11_connection::connect(None).map_err(|e| Error(format!("cannot open the X display: {e}")))?;
    let screen = conn.setup().roots[num].clone();
    let root = screen.root;
    let (mut w, mut h) = (screen.width_in_pixels, screen.height_in_pixels);

    // The backdrop is taken before our window exists (it would otherwise be in the picture).
    let small = make_backdrop(&conn, cfg, root, w, h, screen.root_depth);

    let cursor = blank_cursor(&conn, root);
    let win = conn.generate_id().map_err(err)?;
    let mut aux = CreateWindowAux::new()
        .background_pixel(screen.black_pixel)
        .override_redirect(1)
        .event_mask(EventMask::KEY_PRESS | EventMask::KEY_RELEASE | EventMask::EXPOSURE | EventMask::VISIBILITY_CHANGE | EventMask::STRUCTURE_NOTIFY | EventMask::FOCUS_CHANGE);
    if let Some(cur) = cursor {
        aux = aux.cursor(cur);
    }
    conn.create_window(screen.root_depth, win, root, 0, 0, w, h, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(err)?;
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"declock").map_err(err)?;
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"declock\0declock\0").map_err(err)?;
    // Other windows being mapped, moved or restacked, and monitor changes, are reported to us so we can stay on top.
    conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY)).map_err(err)?;
    conn.randr_select_input(root, randr::NotifyMask::SCREEN_CHANGE).map_err(err)?;
    let gc = conn.generate_id().map_err(err)?;
    conn.create_gc(gc, win, &CreateGCAux::new()).map_err(err)?;
    conn.map_window(win).map_err(err)?;
    raise(&conn, win);
    let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, win, x11rb::CURRENT_TIME);
    conn.get_input_focus().map_err(err)?.reply().map_err(err)?; // the window is viewable from here on

    grab_all(&conn, win, cursor)?;
    // Both grabs are ours. xss-lock's --transfer-sleep-lock waits for us to release this descriptor.
    if let Some(fd) = std::env::var("XSS_SLEEP_LOCK_FD").ok().and_then(|v| v.parse::<i32>().ok()).filter(|fd| *fd > 2) {
        // SAFETY: closing a descriptor number the environment handed us for exactly this purpose.
        unsafe { libc::close(fd) };
    }

    let mut keymap = Keymap::load(&conn)?;
    let mut ui = Ui::layout(&conn, cfg, root, (w, h), small, screen.root_depth);
    let mut state = LockState::new();
    state.caps = conn.query_pointer(root).ok().and_then(|c| c.reply().ok()).is_some_and(|p| u16::from(p.mask) & LOCK != 0);
    let (tx, rx) = mpsc::channel::<Verdict>();
    let (pipe_r, pipe_w) = wake_pipe().map_err(err)?;
    let xfd = conn.stream().as_raw_fd();

    let mut timed_out = true;
    loop {
        // 1. X events
        while let Some(ev) = conn.poll_for_event().map_err(err)? {
            match ev {
                Event::KeyPress(k) => {
                    let sym = keymap.lookup(k.detail, k.state.into());
                    if sym == KEYSYM_CAPS_LOCK {
                        state.caps = u16::from(k.state) & LOCK == 0;
                    } else {
                        state.caps = u16::from(k.state) & LOCK != 0;
                        if let Some(input) = decode_key(sym, keymap.column(k.detail, 0), k.state.into()) {
                            if let Outcome::Check(secret) = state.input(input, Instant::now()) {
                                spawn_check(auth.clone(), secret, tx.clone(), pipe_w);
                            }
                        }
                    }
                }
                Event::KeyRelease(k) => state.caps = u16::from(k.state) & LOCK != 0,
                Event::Expose(e) => ui.paint(&conn, win, gc, i32::from(e.x), i32::from(e.y), u32::from(e.width), u32::from(e.height))?,
                Event::VisibilityNotify(v) if v.window == win && v.state != Visibility::UNOBSCURED => raise(&conn, win),
                Event::ConfigureNotify(c) if c.window != win => raise(&conn, win),
                Event::MapNotify(m) if m.window != win => raise(&conn, win),
                Event::CirculateNotify(c) if c.window != win => raise(&conn, win),
                Event::FocusOut(f) if f.event == win => {
                    let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, win, x11rb::CURRENT_TIME);
                }
                Event::MappingNotify(_) => {
                    if let Ok(k) = Keymap::load(&conn) {
                        keymap = k;
                    }
                }
                Event::RandrScreenChangeNotify(_) => {
                    if let Some(g) = conn.get_geometry(root).ok().and_then(|c| c.reply().ok()) {
                        (w, h) = (g.width, g.height);
                        let _ = conn.configure_window(win, &ConfigureWindowAux::new().x(0).y(0).width(u32::from(w)).height(u32::from(h)));
                        let small = std::mem::replace(&mut ui.small, Pixmap::new(1, 1).expect("1x1 pixmap"));
                        ui = Ui::layout(&conn, cfg, root, (w, h), small, screen.root_depth);
                        raise(&conn, win);
                        ui.render(&View::from_state(&state, clock::now(), cfg.hour12, Instant::now()));
                        ui.paint_all(&conn, win, gc)?;
                    }
                }
                _ => {}
            }
        }
        // 2. the worker's verdict
        while let Ok(v) = rx.try_recv() {
            if let Err(msg) = &v {
                eprintln!("declock: {msg}");
            }
            state.verdict(v.map_err(|_| ()), Instant::now());
        }
        if state.is_unlocked() {
            break;
        }
        // 3. time
        let now = Instant::now();
        state.tick(now);
        if timed_out {
            raise(&conn, win); // belt and braces: a no-op when we already are on top
        }
        let view = View::from_state(&state, clock::now(), cfg.hour12, now);
        if ui.last.as_ref() != Some(&view) {
            ui.render(&view);
            ui.paint_panel(&conn, win, gc)?;
        }
        conn.flush().map_err(err)?;
        // 4. sleep until an event, the worker, or the next tick
        let timeout = state.next_change(now).map_or(1000, |d| d.as_millis().clamp(1, 1000) as i32);
        let mut fds = [libc::pollfd { fd: xfd, events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: pipe_r, events: libc::POLLIN, revents: 0 }];
        // SAFETY: `fds` is a valid array of two pollfd structs for the duration of the call.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        timed_out = n == 0;
        if fds[1].revents != 0 {
            let mut sink = [0u8; 16];
            // SAFETY: reads into a local buffer from our own non-blocking pipe.
            unsafe { libc::read(pipe_r, sink.as_mut_ptr().cast(), sink.len()) };
        }
    }

    // Unlocked: let go of everything and make sure the server has processed it before we exit.
    let _ = conn.ungrab_keyboard(x11rb::CURRENT_TIME);
    let _ = conn.ungrab_pointer(x11rb::CURRENT_TIME);
    let _ = conn.destroy_window(win);
    let _ = conn.get_input_focus().map(|c| c.reply());
    Ok(())
}

fn raise(c: &RustConnection, win: Window) {
    let _ = c.configure_window(win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
}

/// Grabs keyboard and pointer, retrying for [`GRAB_TIMEOUT`]; on failure everything taken is released again.
fn grab_all(c: &RustConnection, win: Window, cursor: Option<Cursor>) -> Result<(), Error> {
    let deadline = Instant::now() + GRAB_TIMEOUT;
    loop {
        let kb = c.grab_keyboard(false, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC).map_err(err)?.reply().map_err(err)?.status;
        let ptr = c
            .grab_pointer(
                false,
                win,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                win,
                cursor.unwrap_or(x11rb::NONE),
                x11rb::CURRENT_TIME,
            )
            .map_err(err)?
            .reply()
            .map_err(err)?
            .status;
        if kb == GrabStatus::SUCCESS && ptr == GrabStatus::SUCCESS {
            return Ok(());
        }
        let _ = c.ungrab_keyboard(x11rb::CURRENT_TIME);
        let _ = c.ungrab_pointer(x11rb::CURRENT_TIME);
        let _ = c.flush();
        if Instant::now() >= deadline {
            let _ = c.destroy_window(win);
            let _ = c.get_input_focus().map(|r| r.reply());
            return Err(Error(format!("could not grab the keyboard ({kb:?}) and pointer ({ptr:?}): another program holds them")));
        }
        std::thread::sleep(Duration::from_millis(40));
        raise(c, win);
    }
}

fn wake_pipe() -> std::io::Result<(i32, i32)> {
    let mut fds = [0i32; 2];
    // SAFETY: `fds` has room for the two descriptors pipe2 writes.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((fds[0], fds[1]))
}

/// Checks `secret` on a worker thread (PAM may block for seconds); the verdict comes back over `tx` and the pipe wakes the loop.
fn spawn_check(auth: Arc<dyn Authenticator>, secret: super::secret::Secret, tx: mpsc::Sender<Verdict>, wake_fd: i32) {
    std::thread::spawn(move || {
        let verdict = auth.authenticate(secret.as_bytes()).map_err(|e| e.0);
        drop(secret); // wipes the buffer
        let _ = tx.send(verdict);
        // SAFETY: a one-byte write to the loop's own pipe, which stays open for the life of the process.
        unsafe { libc::write(wake_fd, [1u8].as_ptr().cast(), 1) };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_backdrop_is_smooth_opaque_and_clamps_edges() {
        let mut small = Pixmap::new(2, 1).unwrap();
        small.data_mut().copy_from_slice(&[0, 0, 0, 255, 200, 100, 50, 255]);
        let full = expand_backdrop(&small, 8, 4);
        assert_eq!(&full.data()[..4], &[0, 0, 0, 255]);
        assert_eq!(&full.data()[28..32], &[200, 100, 50, 255]);
        assert!(full.pixel(3, 2).unwrap().red() > 0 && full.pixel(3, 2).unwrap().red() < 200);
        assert!(full.pixels().iter().all(|p| p.alpha() == 255));
        assert_eq!(&full.data()[..32], &full.data()[32..64]);
    }

    /// Rendering/upload only: no locker, grabs, password, or authenticator.
    #[test]
    #[ignore = "run through tests/e2e/lock-perf.sh on private Xvfb"]
    fn private_x11_render_performance() {
        assert_eq!(std::env::var("DECLOCK_PERF_PRIVATE").as_deref(), Ok("1"));
        let display = std::env::var("DISPLAY").unwrap();
        assert!(!matches!(display.as_str(), ":0" | ":1"));
        let (c, n) = crate::x11_connection::connect(None).unwrap();
        let screen = &c.setup().roots[n];
        let (w, h) = (screen.width_in_pixels, screen.height_in_pixels);
        assert_eq!((w, h), (3440, 1440));
        let cfg = Config {
            bg: BgMode::Screenshot,
            blur: 3,
            dim: 0.45,
            hour12: false,
            user: super::super::UserInfo { login: "test".into(), display: "Performance Test".into(), home: "/nonexistent".into() },
        };
        let start = Instant::now();
        let small = make_backdrop(&c, &cfg, screen.root, w, h, screen.root_depth);
        let captured = start.elapsed();
        let mut ui = Ui::layout(&c, &cfg, screen.root, (w, h), small, screen.root_depth);
        let laid_out = start.elapsed();
        let win = c.generate_id().unwrap();
        let gc = c.generate_id().unwrap();
        c.create_window(screen.root_depth, win, screen.root, 0, 0, w, h, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new().background_pixel(0)).unwrap();
        c.create_gc(gc, win, &CreateGCAux::new()).unwrap();
        c.map_window(win).unwrap();
        let state = LockState::new();
        let mut view = View::from_state(&state, clock::now(), false, Instant::now());
        ui.render(&view);
        let rendered = start.elapsed();
        ui.paint_all(&c, win, gc).unwrap();
        c.get_input_focus().unwrap().reply().unwrap();
        let startup = start.elapsed();
        println!(
            "LOCK_STAGES capture_ms={:.2} layout_ms={:.2} render_ms={:.2} upload_ms={:.2}",
            captured.as_secs_f64() * 1000.0,
            (laid_out - captured).as_secs_f64() * 1000.0,
            (rendered - laid_out).as_secs_f64() * 1000.0,
            (startup - rendered).as_secs_f64() * 1000.0
        );
        let mut timings = Vec::new();
        for dots in 1..=20 {
            let start = Instant::now();
            view.dots = dots;
            ui.render(&view);
            ui.paint_panel(&c, win, gc).unwrap();
            c.get_input_focus().unwrap().reply().unwrap();
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        timings.sort_by(f64::total_cmp);
        println!("LOCK_PERF startup_ms={:.2} key_median_ms={:.2} key_p95_ms={:.2}", startup.as_secs_f64() * 1000.0, timings[10], timings[19]);
        // Cached/static split must produce the same complete image as a full redraw.
        let mut reference = ui.panel_backdrop.clone();
        let mut painter = Painter::new(ui.painter.scale(), "Performance Test", None);
        painter.draw(&mut reference, &view);
        assert_eq!(reference.data(), ui.panel.data());
        let image = c.get_image(ImageFormat::Z_PIXMAP, win, ui.px as i16, ui.py as i16, ui.panel.width() as u16, ui.panel.height() as u16, !0).unwrap().reply().unwrap();
        assert_eq!(image.data.len(), ui.panel.data().len());
        for (actual, expected) in image.data.chunks_exact(4).zip(ui.panel.data().chunks_exact(4)) {
            assert_eq!(&actual[..3], &[expected[2], expected[1], expected[0]], "partial upload left stale pixels");
        }
        let max_ms = std::env::var("DECLOCK_PERF_MAX_KEY_MS").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(250.0);
        assert!(timings[10] < max_ms, "typing render/upload median exceeds {max_ms}ms");
        c.destroy_window(win).unwrap();
        c.flush().unwrap();
    }

    #[test]
    fn keys_decode() {
        assert_eq!(decode_key(0xff0d, 0xff0d, 0), Some(Input::Submit));
        assert_eq!(decode_key(0xff8d, 0xff8d, 0), Some(Input::Submit));
        assert_eq!(decode_key(0xff08, 0xff08, 0), Some(Input::Backspace));
        assert_eq!(decode_key(0xff1b, 0xff1b, 0), Some(Input::Clear));
        assert_eq!(decode_key('A' as u32, 'a' as u32, SHIFT), Some(Input::Char('A')));
        assert_eq!(decode_key(0xe9, 0xe9, MOD5), Some(Input::Char('\u{e9}')));
        assert_eq!(decode_key('u' as u32, 'u' as u32, CONTROL), Some(Input::Clear));
        assert_eq!(decode_key(0xff08, 0xff08, CONTROL), Some(Input::Clear));
    }

    #[test]
    fn shortcuts_function_media_and_modifier_keys_do_nothing() {
        assert_eq!(decode_key('c' as u32, 'c' as u32, CONTROL), None);
        assert_eq!(decode_key('x' as u32, 'x' as u32, MOD1), None);
        assert_eq!(decode_key('x' as u32, 'x' as u32, MOD4), None);
        assert_eq!(decode_key(0x1008ff13, 0x1008ff13, 0), None, "XF86AudioRaiseVolume");
        assert_eq!(decode_key(0xffbe, 0xffbe, 0), None, "F1");
        assert_eq!(decode_key(0xffe1, 0xffe1, 0), None, "Shift_L");
        assert_eq!(decode_key(0xff09, 0xff09, 0), None, "Tab");
        assert_eq!(decode_key(0xff51, 0xff51, 0), None, "Left");
    }

    #[test]
    fn keymap_lookup_handles_shift_caps_and_numlock() {
        let mut syms = vec![0u32; 5 * 4];
        // keycode 8: a / A ; keycode 9: KP_End / KP_1 ; keycode 10: 1 / ! ; keycode 11: e / E / eacute / Eacute (AltGr columns 4,5 are separate below)
        syms[0..2].copy_from_slice(&['a' as u32, 'A' as u32]);
        syms[5..7].copy_from_slice(&[0xff9c, 0xffb1]);
        syms[10..12].copy_from_slice(&['1' as u32, '!' as u32]);
        syms[15..17].copy_from_slice(&['e' as u32, 'E' as u32]);
        syms[15 + 4] = 0xe9;
        let km = Keymap { min_keycode: 8, per_keycode: 5, syms };
        assert_eq!(km.lookup(8, 0), 'a' as u32);
        assert_eq!(km.lookup(8, SHIFT), 'A' as u32);
        assert_eq!(km.lookup(8, LOCK), 'A' as u32);
        assert_eq!(km.lookup(8, LOCK | SHIFT), 'A' as u32);
        assert_eq!(km.lookup(10, LOCK), '1' as u32, "caps lock leaves digits alone");
        assert_eq!(km.lookup(9, 0), 0xff9c, "keypad without Num Lock is End");
        assert_eq!(km.lookup(9, MOD2), 0xffb1, "keypad with Num Lock is 1");
        assert_eq!(km.lookup(11, MOD5), 0xe9, "AltGr column");
        assert_eq!(km.lookup(200, 0), 0);
    }
}
