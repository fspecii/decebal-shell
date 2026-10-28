//! The X11 side of the lock: one fullscreen override-redirect window over the whole root (every monitor), the
//! keyboard and pointer grabbed for as long as it lives, kept on top, and a small event loop that sleeps in `poll`
//! (a one-second tick for the clock and a ~60 Hz tick only while the wrong-password shake plays).

use super::auth::Authenticator;
use super::bg;
use super::state::{Input, LockState, Outcome};
use super::view::{panel_size, scale_for, Painter, View};
use super::{face_path, BgMode, Config};
use crate::gfx::blit_scaled;
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

struct Ui {
    w: u16,
    h: u16,
    depth: u8,
    small: Pixmap,
    panel: Pixmap,
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
        Ui { w, h, depth, small, panel: Pixmap::new(pw, ph).expect("panel pixmap"), px, py, painter, last: None }
    }

    /// Draws the backdrop and the panel for `view` into the panel pixmap.
    fn render(&mut self, view: &View) {
        self.panel.fill(tiny_skia::Color::TRANSPARENT);
        blit_scaled(&mut self.panel, &self.small, -(self.px as f32), -(self.py as f32), f32::from(self.w), f32::from(self.h), 1.0);
        self.painter.draw(&mut self.panel, view);
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
            let mut pm = Pixmap::new((x1 - x0) as u32, rows as u32).ok_or_else(|| Error("pixmap".into()))?;
            blit_scaled(&mut pm, &self.small, -(x0 as f32), -(y as f32), f32::from(self.w), f32::from(self.h), 1.0);
            pm.draw_pixmap(self.px - x0, self.py - y, self.panel.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            let mut bgrx = pm.take();
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
        self.paint(c, win, gc, self.px, self.py, self.panel.width(), self.panel.height())
    }
}

// ---------------------------------------------------------------- the lock

type Verdict = Result<bool, String>;

/// Locks the screen and returns once the right password was entered (the grabs are released first).
/// `Err` means the lock could not be established (no grab); nothing is left on screen in that case.
pub fn run(cfg: &Config, auth: Arc<dyn Authenticator>) -> Result<(), Error> {
    let (conn, num) = x11rb::connect(None).map_err(|e| Error(format!("cannot open the X display: {e}")))?;
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
