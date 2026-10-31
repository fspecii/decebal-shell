//! X11 front end of decnotify: banner windows stacked below the menu bar, and the history panel.
//!
//! Every banner is its own override-redirect window (no window manager involvement, nothing for a tiling
//! WM to tile), clipped with the Shape extension to its rounded outline so it never paints black corners
//! without a compositor. All state is event driven; with no banner on screen the process sleeps in poll(2).
//!
//! The queue, layout, deadlines, hover and press rules and the history panel's behaviour are shared with the
//! Wayland front end ([`super::view`], [`super::history`]); this file only owns windows, pixels and X events.

use super::history::{history_dimensions, HistOutcome, HistWin};
use super::model::Notification;
use super::view::{stack_origin, Banner, BannerLayout, Btn, Host, Metrics, Render, Stack, ThemeWake};
pub use super::view::{Palette, UiEvent};
use super::{Frontend, Shared, Wake};
use crate::backend::x11::{panel_monitor, visible_rects, Rect as MonRect};
use std::os::fd::{AsRawFd, RawFd};
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

struct Atoms {
    wtype: Atom,
    wtype_notification: Atom,
    wm_name: Atom,
    utf8: Atom,
}

/// The X server side: connection, monitor and drawing state. The banner stack drives it through [`Host`].
struct XHost {
    conn: RustConnection,
    root: Window,
    gc: Gcontext,
    depth: u8,
    mon: MonRect,
    r: Render,
    at: Atoms,
}

pub struct Ui {
    h: XHost,
    stack: Stack<Window>,
    hist: Option<HistWin<Window>>,
    theme: ThemeWake,
    theme_generation: u64,
}

fn cerr<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn xft_scale(conn: &RustConnection, root: Window) -> f32 {
    let Ok(Ok(r)) = conn.get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 65536).map(|c| c.reply()) else { return 1.0 };
    let text = String::from_utf8_lossy(&r.value);
    text.lines().find_map(|l| l.strip_prefix("Xft.dpi:")).and_then(|v| v.trim().parse::<f32>().ok()).map_or(1.0, |d| (d / 96.0).clamp(1.0, 4.0))
}

/// Uploads a premultiplied pixmap as BGRX, in bands small enough for the server's request limit.
fn present(conn: &RustConnection, win: Window, gc: Gcontext, pm: &Pixmap, depth: u8) {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let max = conn.maximum_request_bytes().saturating_sub(64);
    let rows_per = (max / (w * 4)).clamp(1, h.min(128));
    let mut buf = vec![0u8; w * rows_per * 4];
    let mut y = 0;
    while y < h {
        let rows = rows_per.min(h - y);
        for (d, s) in buf[..rows * w * 4].chunks_exact_mut(4).zip(pm.data()[y * w * 4..(y + rows) * w * 4].chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], 0xff]);
        }
        let _ = conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w as u16, rows as u16, 0, y as i16, 0, depth, &buf[..rows * w * 4]);
        y += rows;
    }
}

impl XHost {
    fn refresh_monitor(&mut self) {
        let fallback = self.conn.get_geometry(self.root).ok().and_then(|cookie| cookie.reply().ok()).map(|r| (i32::from(r.width), i32::from(r.height))).unwrap_or((self.mon.w, self.mon.h));
        self.mon = panel_monitor(&self.conn, self.root, fallback);
    }

    fn make_window(&self, x: i32, y: i32, w: u32, h: u32, keys: bool) -> Result<Window, String> {
        let win = self.conn.generate_id().map_err(cerr)?;
        let mut mask = EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::ENTER_WINDOW | EventMask::LEAVE_WINDOW | EventMask::POINTER_MOTION;
        if keys {
            mask |= EventMask::KEY_PRESS;
        }
        let aux = CreateWindowAux::new().override_redirect(1).background_pixel(self.r.pal.bg_pixel()).event_mask(mask);
        self.conn.create_window(0, win, self.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(cerr)?;
        let _ = self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"decnotify\0Decnotify\0");
        let _ = self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"Notification");
        let _ = self.conn.change_property8(PropMode::REPLACE, win, self.at.wm_name, self.at.utf8, b"Notification");
        let _ = self.conn.change_property32(PropMode::REPLACE, win, self.at.wtype, AtomEnum::ATOM, &[self.at.wtype_notification]);
        Ok(win)
    }

    fn set_shape(&self, win: Window, pm: &Pixmap) {
        let rects = visible_rects(pm);
        let _ = self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, win, 0, 0, &rects);
    }
}

impl Host for XHost {
    type Win = Window;

    fn metrics(&self) -> Metrics {
        // below the classic bar; with the Caelestia pill bar below it when it runs along the top (`top`, `split`), else near the edge
        let th = crate::theme::th();
        let (top, right_gap) = stack_origin(self.r.s, self.mon.y, self.mon.h, crate::theme::bar_bottom(), th.pill_bar().is_some(), th.bar_insets().right);
        Metrics { s: self.r.s, x: self.mon.x, y: self.mon.y, w: self.mon.w, h: self.mon.h, top, right_gap }
    }

    fn layout(&mut self, n: &Notification) -> BannerLayout {
        let m = self.metrics();
        self.r.layout(&m, n)
    }

    fn icon(&mut self, n: &Notification) -> Option<Pixmap> {
        self.r.resolve_icon(n)
    }

    fn create(&mut self, layout: &BannerLayout, x: i32, y: i32) -> Option<Window> {
        self.make_window(x, y, layout.w, layout.h, false).ok()
    }

    fn map(&mut self, win: Window) {
        let _ = self.conn.map_window(win);
        self.r.fonts.trim();
    }

    fn destroy(&mut self, win: Window) {
        let _ = self.conn.destroy_window(win);
    }

    fn resize(&mut self, win: Window, w: u32, h: u32) {
        let _ = self.conn.configure_window(win, &ConfigureWindowAux::new().width(w).height(h));
    }

    fn place(&mut self, win: Window, x: i32, y: i32) {
        let _ = self.conn.configure_window(win, &ConfigureWindowAux::new().x(x).y(y));
    }

    fn paint(&mut self, b: &Banner<Window>) {
        let pm = self.r.paint_banner(b);
        self.set_shape(b.win, &pm);
        present(&self.conn, b.win, self.gc, &pm, self.depth);
        let _ = self.conn.flush();
        self.r.fonts.trim();
    }

    fn flush(&mut self) {
        let _ = self.conn.flush();
    }
}

impl Ui {
    pub fn connect() -> Result<Ui, String> {
        let (conn, num) = crate::x11_connection::connect(None).map_err(cerr)?;
        let screen = conn.setup().roots[num].clone();
        let root = screen.root;
        let gc = conn.generate_id().map_err(cerr)?;
        conn.create_gc(gc, root, &CreateGCAux::new()).map_err(cerr)?;
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE)).map_err(cerr)?;
        let atom = |name: &str| -> Result<Atom, String> { Ok(conn.intern_atom(false, name.as_bytes()).map_err(cerr)?.reply().map_err(cerr)?.atom) };
        let at = Atoms { wtype: atom("_NET_WM_WINDOW_TYPE")?, wtype_notification: atom("_NET_WM_WINDOW_TYPE_NOTIFICATION")?, wm_name: atom("_NET_WM_NAME")?, utf8: atom("UTF8_STRING")? };
        let mon = panel_monitor(&conn, root, (i32::from(screen.width_in_pixels), i32::from(screen.height_in_pixels)));
        let s = xft_scale(&conn, root);
        let r = Render::new(s)?;
        let theme = ThemeWake::start()?;
        let theme_generation = crate::theme::live::generation();
        let mut stack = Stack::new();
        stack.generation = theme_generation;
        Ok(Ui { h: XHost { conn, root, gc, depth: screen.root_depth, mon, r, at }, stack, hist: None, theme, theme_generation })
    }

    fn fd(&self) -> RawFd {
        self.h.conn.stream().as_raw_fd()
    }

    /// Lays everything out again (theme, font scale or monitor change); ids, timers and hover are kept.
    fn reflow(&mut self, sh: &Shared) {
        self.stack.reflow(&mut self.h);
        self.refresh_history(sh);
        let _ = self.h.conn.flush();
    }

    fn monitor_reflow(&mut self, sh: &Shared) {
        self.h.refresh_monitor();
        self.reflow(sh);
    }

    // ---- history panel

    fn close_history(&mut self) {
        if let Some(h) = self.hist.take() {
            let _ = self.h.conn.ungrab_pointer(x11rb::CURRENT_TIME);
            let _ = self.h.conn.ungrab_keyboard(x11rb::CURRENT_TIME);
            let _ = self.h.conn.destroy_window(h.win);
            let _ = self.h.conn.flush();
        }
    }

    fn redraw_history(&mut self, sh: &Shared) {
        let m = self.h.metrics();
        let dims = history_dimensions(&self.h.r.fonts, &m, sh, &|w, h| (w, h));
        let (x, y) = (m.x_for(dims.w), m.top);
        let s = self.h.r.s;
        let Some(hw) = &mut self.hist else { return };
        let (w, h) = (dims.w, dims.h);
        if hw.relayout(dims, x, y, sh, s) {
            let _ = self.h.conn.configure_window(hw.win, &ConfigureWindowAux::new().x(x).y(y).width(w).height(h));
        }
        let (scroll, rows) = (hw.scroll, hw.rows);
        self.h.r.prepare_history(sh, scroll, rows);
        let Some(hw) = &self.hist else { return };
        let pm = self.h.r.paint_history(sh, hw);
        self.h.set_shape(hw.win, &pm);
        present(&self.h.conn, hw.win, self.h.gc, &pm, self.h.depth);
        let _ = self.h.conn.flush();
        self.h.r.fonts.trim();
        hw.debug(sh, u64::from(hw.win));
    }

    fn apply_history(&mut self, outcome: HistOutcome, sh: &Shared) {
        match outcome {
            HistOutcome::Idle => {}
            HistOutcome::Redraw => self.redraw_history(sh),
            HistOutcome::Close => self.close_history(),
        }
    }

    fn history_key(&mut self, e: KeyPressEvent, sh: &mut Shared) {
        let sym = self.h.conn.get_keyboard_mapping(e.detail, 1).ok().and_then(|cookie| cookie.reply().ok()).and_then(|reply| reply.keysyms.first().copied()).unwrap_or(if e.detail == 9 {
            0xff1b
        } else {
            0
        });
        let shift = e.state.contains(KeyButMask::SHIFT);
        let s = self.h.r.s;
        let Some(hw) = &mut self.hist else { return };
        let outcome = hw.key(sym, shift, sh, s);
        self.apply_history(outcome, sh);
    }

    // ---- events

    fn handle(&mut self, ev: Event, sh: &mut Shared, out: &mut Vec<UiEvent>) {
        match ev {
            Event::Expose(e) if e.count == 0 => {
                if let Some(i) = self.stack.index_of(e.window) {
                    self.stack.redraw(&mut self.h, i);
                } else if self.hist.as_ref().is_some_and(|h| h.win == e.window) {
                    self.redraw_history(sh);
                }
            }
            Event::EnterNotify(e) if e.mode == NotifyMode::NORMAL => {
                if let Some(i) = self.stack.index_of(e.event) {
                    self.stack.enter(&mut self.h, i);
                }
            }
            Event::LeaveNotify(e) if e.mode == NotifyMode::NORMAL => {
                if let Some(i) = self.stack.index_of(e.event) {
                    self.stack.leave(&mut self.h, i, sh.cfg.banner_seconds);
                }
            }
            Event::MotionNotify(e) => self.motion(e.event, f32::from(e.event_x), f32::from(e.event_y), sh),
            Event::ButtonPress(e) => self.press(e, sh, out),
            Event::KeyPress(e) if self.hist.is_some() => self.history_key(e, sh),
            Event::ConfigureNotify(e) if e.window == self.h.root => self.monitor_reflow(sh),
            Event::PropertyNotify(e) if e.window == self.h.root && e.atom == u32::from(AtomEnum::RESOURCE_MANAGER) => {
                self.h.r.s = xft_scale(&self.h.conn, self.h.root);
                self.monitor_reflow(sh);
            }
            _ => {}
        }
    }

    fn motion(&mut self, win: Window, x: f32, y: f32, sh: &Shared) {
        if let Some(i) = self.stack.index_of(win) {
            self.stack.motion(&mut self.h, i, x, y);
        } else if let Some(hw) = self.hist.as_mut().filter(|h| h.win == win) {
            if hw.hover_at(x, y, sh) {
                self.redraw_history(sh);
            }
        }
    }

    fn press(&mut self, e: ButtonPressEvent, sh: &mut Shared, out: &mut Vec<UiEvent>) {
        let (x, y) = (f32::from(e.event_x), f32::from(e.event_y));
        if let Some(i) = self.stack.index_of(e.event) {
            let button = match e.detail {
                1 => Btn::Left,
                2 => Btn::Middle,
                3 => Btn::Right,
                _ => return,
            };
            out.extend(self.stack.press(&mut self.h, i, button, x, y));
            return;
        }
        let s = self.h.r.s;
        let Some(hw) = self.hist.as_mut() else { return };
        if e.event == hw.win && (e.detail == 4 || e.detail == 5) {
            let outcome = hw.scroll_step(e.detail == 4, sh, s);
            self.apply_history(outcome, sh);
            return;
        }
        if e.event != hw.win || !hw.contains(x, y) {
            self.close_history();
            return;
        }
        let outcome = hw.click(x, y, sh);
        self.apply_history(outcome, sh);
    }
}

impl Frontend for Ui {
    fn show(&mut self, n: Notification, secs: u32) {
        self.stack.show(&mut self.h, n, secs);
    }

    fn close(&mut self, id: u32, reason: u32) -> Vec<UiEvent> {
        self.stack.close(&mut self.h, id, reason)
    }

    fn history_open(&self) -> bool {
        self.hist.is_some()
    }

    fn toggle_history(&mut self, sh: &Shared) -> Vec<UiEvent> {
        if self.hist.is_some() {
            self.close_history();
            return Vec::new();
        }
        let events = self.stack.close_all(&mut self.h);
        let m = self.h.metrics();
        let dims = history_dimensions(&self.h.r.fonts, &m, sh, &|w, h| (w, h));
        let x = m.x_for(dims.w);
        let Ok(win) = self.h.make_window(x, m.top, dims.w, dims.h, true) else { return events };
        self.hist = Some(HistWin::new(win, dims, x, m.top));
        self.redraw_history(sh);
        let _ = self.h.conn.map_window(win);
        let _ = self.h.conn.set_input_focus(InputFocus::POINTER_ROOT, win, x11rb::CURRENT_TIME);
        let _ = self.h.conn.grab_keyboard(true, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC);
        let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        let _ = self.h.conn.grab_pointer(true, win, mask, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, x11rb::CURRENT_TIME);
        let _ = self.h.conn.flush();
        self.h.r.fonts.trim();
        if let Some(hw) = &self.hist {
            hw.debug(sh, u64::from(win));
        }
        events
    }

    fn refresh_history(&mut self, sh: &Shared) {
        if self.hist.is_some() {
            self.redraw_history(sh);
        }
    }

    fn refresh_theme(&mut self, sh: &Shared) {
        self.theme.drain();
        let Some(generation) = self.h.r.retheme(self.theme_generation) else { return };
        self.theme_generation = generation;
        self.stack.generation = generation;
        // The shared watcher also wakes us for icon-pack changes. The cache dropped both successful and missing
        // lookups; refresh retained banner artwork without replacing notifications or resetting expiry/hover state.
        self.stack.reicon(&mut self.h);
        self.reflow(sh);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.stack.next_deadline()
    }

    fn expire(&mut self, now: Instant) -> Vec<UiEvent> {
        self.stack.expire(&mut self.h, now)
    }

    fn wait(&mut self, bus: RawFd, timeout: Option<Duration>) -> Result<Wake, String> {
        let ready = crate::backend::poll_readable(&[bus, self.fd(), self.theme.fd()], timeout).map_err(|e| e.to_string())?;
        Ok(Wake { bus: ready[0], theme: ready[2] })
    }

    /// Drains pending X events; returns what to tell the bus.
    fn pump(&mut self, sh: &mut Shared) -> Result<Vec<UiEvent>, String> {
        let mut out = Vec::new();
        while let Some(ev) = self.h.conn.poll_for_event().map_err(cerr)? {
            self.handle(ev, sh, &mut out);
        }
        let _ = self.h.conn.flush();
        Ok(out)
    }
}
