//! X11 top-level window backend (normal managed window, keyboard + wheel input).
//!
//! Worker threads wake the loop by sending a private client message to the window over a
//! second connection, so the main thread can simply block in `wait_for_event`.

use crate::app::Button;
use crate::keysym::{char_keysym, key_event, keysym_char};
use crate::window_app::{Action, Response, Waker, WindowApp};
use std::fmt;
use std::sync::Arc;
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::properties::WmSizeHints;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Pixels scrolled per wheel notch.
const WHEEL_STEP: f32 = 100.0;

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "x11: {}", self.0)
    }
}
impl std::error::Error for Error {}

fn err<E: fmt::Display>(e: E) -> Error {
    Error(e.to_string())
}

struct Atoms {
    wm_protocols: Atom,
    wm_delete: Atom,
    net_wm_name: Atom,
    utf8: Atom,
    net_wm_state: Atom,
    max_vert: Atom,
    max_horz: Atom,
    moveresize: Atom,
    wm_change_state: Atom,
    wake: Atom,
}

fn intern<C: Connection>(c: &C, name: &str) -> Result<Atom, Error> {
    Ok(c.intern_atom(false, name.as_bytes()).map_err(err)?.reply().map_err(err)?.atom)
}

fn atoms<C: Connection>(c: &C) -> Result<Atoms, Error> {
    Ok(Atoms {
        wm_protocols: intern(c, "WM_PROTOCOLS")?,
        wm_delete: intern(c, "WM_DELETE_WINDOW")?,
        net_wm_name: intern(c, "_NET_WM_NAME")?,
        utf8: intern(c, "UTF8_STRING")?,
        net_wm_state: intern(c, "_NET_WM_STATE")?,
        max_vert: intern(c, "_NET_WM_STATE_MAXIMIZED_VERT")?,
        max_horz: intern(c, "_NET_WM_STATE_MAXIMIZED_HORZ")?,
        moveresize: intern(c, "_NET_WM_MOVERESIZE")?,
        wm_change_state: intern(c, "WM_CHANGE_STATE")?,
        wake: intern(c, "_MACSHELL_WAKE")?,
    })
}

// ---------------------------------------------------------------- keyboard

/// Keysym table from the server's keyboard mapping.
struct Keymap {
    min_keycode: u8,
    per_keycode: usize,
    syms: Vec<u32>,
}

impl Keymap {
    fn load<C: Connection>(c: &C) -> Result<Keymap, Error> {
        let setup = c.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let reply = c.get_keyboard_mapping(min, max - min + 1).map_err(err)?.reply().map_err(err)?;
        Ok(Keymap { min_keycode: min, per_keycode: reply.keysyms_per_keycode as usize, syms: reply.keysyms })
    }

    fn column(&self, keycode: u8, col: usize) -> u32 {
        if keycode < self.min_keycode || col >= self.per_keycode {
            return 0;
        }
        let i = (keycode - self.min_keycode) as usize * self.per_keycode + col;
        self.syms.get(i).copied().unwrap_or(0)
    }

    /// Keysym for `keycode` given the modifier state (shift, caps lock, AltGr).
    fn lookup(&self, keycode: u8, state: u16) -> u32 {
        let shift = state & 1 != 0;
        let caps = state & 2 != 0;
        let altgr = state & (1 << 7) != 0; // Mod5 is ISO_Level3_Shift on most layouts
        let base = if altgr && self.column(keycode, 4) != 0 { 4 } else { 0 };
        let plain = self.column(keycode, base);
        let shifted = match self.column(keycode, base + 1) {
            0 => plain,
            s => s,
        };
        let sym = if shift { shifted } else { plain };
        // Caps lock only affects letters.
        if caps && !shift {
            if let Some(c) = keysym_char(plain).filter(|c| c.is_alphabetic()) {
                return char_keysym(c.to_uppercase().next().unwrap_or(c)).unwrap_or(sym);
            }
        }
        sym
    }
}

// ---------------------------------------------------------------- window

/// Top edge of a popup of height `h` on the monitor `mon`: above the bottom by the app's margin, centred, or a fifth down.
fn popup_top<A: WindowApp>(app: &A, mon: &super::x11::Rect, h: u32) -> i32 {
    if let Some(a) = app.popup_anchor() {
        mon.y + a.top
    } else if let Some(f) = app.popup_bottom_margin() {
        (mon.y + mon.h - h as i32 - (mon.h as f32 * f).round() as i32).max(mon.y)
    } else if app.popup_centered() {
        mon.y + (mon.h - h as i32) / 2
    } else {
        mon.y + mon.h / 5
    }
}

/// Left edge of a popup `w` wide: centred on the monitor, or where the app anchors it.
fn popup_left<A: WindowApp>(app: &A, mon: &super::x11::Rect, w: u32) -> i32 {
    match app.popup_anchor() {
        Some(a) => mon.x + a.left(mon.w, w as i32),
        None => mon.x + (mon.w - w as i32) / 2,
    }
}

pub fn run<A: WindowApp + 'static>(mut app: A) -> Result<(), Error> {
    let (conn, screen_num) = x11rb::connect(None).map_err(err)?;
    let screen = conn.setup().roots[screen_num].clone();
    let at = atoms(&conn)?;
    let keymap = Keymap::load(&conn)?;

    let (sw, sh) = (u32::from(screen.width_in_pixels), u32::from(screen.height_in_pixels));
    let popup = app.overlay();
    let passive = popup && app.popup_passive();
    // The size a popup takes depends on the UI scale, so tell the app before asking for it.
    app.set_scale(xft_scale(&conn, screen.root));
    let mon = if popup { super::x11::panel_monitor(&conn, screen.root, (sw as i32, sh as i32)) } else { super::x11::Rect { x: 0, y: 0, w: sw as i32, h: sh as i32 } };
    if popup {
        app.set_popup(true);
        // Popups are placed on the primary monitor, so that is the "screen" they can use.
        app.set_screen(mon.w as u32, mon.h as u32);
    }
    let (iw, ih) = app.initial_size();
    let (w, h) = if popup {
        let (cw, ch) = app.content_size().unwrap_or((iw, ih));
        (cw.min((mon.w as u32).saturating_sub(40)).max(200), ch.max(1))
    } else {
        (iw.min(sw.saturating_sub(40)).max(320), ih.min(sh.saturating_sub(60)).max(240))
    };
    // Popup: centred horizontally on the primary monitor.
    let popup_y = popup_top(&app, &mon, h);
    let (px, py) = if popup { (popup_left(&app, &mon, w), popup_y) } else { (((sw.saturating_sub(w)) / 2) as i32, ((sh.saturating_sub(h)) / 2) as i32) };

    let win = conn.generate_id().map_err(err)?;
    let mut aux = CreateWindowAux::new().background_pixel(screen.black_pixel).event_mask(if passive {
        EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY
    } else {
        EventMask::FOCUS_CHANGE
            | EventMask::EXPOSURE
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::POINTER_MOTION
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::LEAVE_WINDOW
            | EventMask::ENTER_WINDOW
            | EventMask::KEY_PRESS
            | EventMask::KEY_RELEASE
    });
    if popup {
        // No window manager involvement at all: rdwm and other tiling WMs never see the window.
        aux = aux.override_redirect(1);
    }
    // A translucent popup needs a 32-bit visual and a compositor; otherwise it stays on the root visual and opaque.
    let argb = if popup && app.popup_translucent() && super::x11::has_compositor(&conn, screen_num) { super::x11::argb_visual(&screen) } else { None };
    if popup {
        app.set_popup_alpha(argb.is_some());
    }
    if let Some(visual) = argb {
        let colormap = conn.generate_id().map_err(err)?;
        conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual).map_err(err)?;
        aux = aux.background_pixel(0).border_pixel(0).colormap(colormap);
        conn.create_window(32, win, screen.root, px as i16, py as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, visual, &aux).map_err(err)?;
    } else {
        conn.create_window(COPY_DEPTH, win, screen.root, px as i16, py as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(err)?;
    }

    let title = app.title();
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes()).map_err(err)?;
    conn.change_property8(PropMode::REPLACE, win, at.net_wm_name, at.utf8, title.as_bytes()).map_err(err)?;
    let class = wm_class(app.app_id());
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, class.as_bytes()).map_err(err)?;
    conn.change_property32(PropMode::REPLACE, win, at.wm_protocols, AtomEnum::ATOM, &[at.wm_delete]).map_err(err)?;
    if !popup {
        let mut hints = WmSizeHints::new();
        hints.min_size = Some((480, 360));
        hints.set_normal_hints(&conn, win).map_err(err)?;
    }

    let gc = conn.generate_id().map_err(err)?;
    conn.create_gc(gc, win, &CreateGCAux::new()).map_err(err)?;

    // Cross-thread wake-up: a second connection that pokes our window with a client message.
    let wake_conn = Arc::new(x11rb::connect(None).map_err(err)?.0);
    let wake_atom = at.wake;
    let waker: Waker = {
        let wc = wake_conn.clone();
        Arc::new(move || {
            let ev = ClientMessageEvent::new(32, win, wake_atom, [0u32; 5]);
            let _ = wc.send_event(false, win, EventMask::NO_EVENT, ev);
            let _ = wc.flush();
        })
    };

    if !popup {
        app.set_screen(sw, sh);
    }
    app.set_rounded(popup);
    app.resize(w, h);
    app.set_waker(waker);

    let max_bytes = conn.maximum_request_bytes().saturating_sub(64);
    let mut size = (w, h);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error("pixmap".into()))?;
    let mut dirty = true;
    let depth = if argb.is_some() { 32 } else { screen.root_depth };
    let mut grabbed = !popup || passive;
    let mut bg = 0u32;

    if passive {
        // Click-through: an empty input region (nothing is ever hit, so no crossing, focus or button events either).
        conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[]).map_err(err)?;
        if argb.is_none() {
            // No compositor: shape the window and give it the card's colour before it is mapped, so it never flashes black.
            app.draw(&mut pm);
            if let Some(px) = backdrop(&pm) {
                bg = px;
                conn.change_window_attributes(win, &ChangeWindowAttributesAux::new().background_pixel(px)).map_err(err)?;
            }
            conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, win, 0, 0, &super::x11::visible_rects(&pm)).map_err(err)?;
        }
    }
    conn.map_window(win).map_err(err)?;
    conn.flush().map_err(err)?;

    loop {
        if dirty {
            dirty = false;
            if pm.width() != size.0 || pm.height() != size.1 {
                pm = Pixmap::new(size.0.max(1), size.1.max(1)).ok_or_else(|| Error("pixmap".into()))?;
            }
            app.draw(&mut pm);
            present(&conn, win, gc, &pm, depth, argb.is_some(), max_bytes)?;
            // Unpainted areas (a resize in flight, a window being dragged) show the window background:
            // follow the app's own backdrop instead of black.
            if let Some(px) = backdrop(&pm).filter(|_| argb.is_none()) {
                if px != bg {
                    bg = px;
                    conn.change_window_attributes(win, &ChangeWindowAttributesAux::new().background_pixel(px)).map_err(err)?;
                }
            }
            if popup && argb.is_none() {
                // Rounded corners without a compositor: clip the window to the opaque pixels.
                let rects = super::x11::visible_rects(&pm);
                conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, win, 0, 0, &rects).map_err(err)?;
                conn.flush().map_err(err)?;
            }
        }
        if popup && !grabbed {
            grabbed = true;
            let mods = grab_input(&conn, win, screen.root);
            let r = app.input_grabbed(mods);
            if r.action == Some(Action::Close) {
                return Ok(());
            }
            dirty |= r.redraw;
        }

        let mut next = Some(conn.wait_for_event().map_err(err)?);
        while let Some(ev) = next.take() {
            let mut resp = Response::NONE;
            match ev {
                Event::Expose(e) if e.count == 0 => dirty = true,
                Event::ConfigureNotify(c) => {
                    let ns = (u32::from(c.width), u32::from(c.height));
                    if ns != size && ns.0 > 0 && ns.1 > 0 {
                        size = ns;
                        app.resize(ns.0, ns.1);
                        dirty = true;
                    }
                }
                Event::MotionNotify(m) => resp = app.pointer_motion(f32::from(m.event_x), f32::from(m.event_y)),
                Event::EnterNotify(m) => resp = app.pointer_motion(f32::from(m.event_x), f32::from(m.event_y)),
                Event::LeaveNotify(_) => resp = app.pointer_leave(),
                Event::ButtonPress(b) if popup && (b.event_x < 0 || b.event_y < 0 || b.event_x as u32 >= size.0 || b.event_y as u32 >= size.1 || b.event != win) => return Ok(()),
                Event::ButtonPress(b) => {
                    let (x, y) = (f32::from(b.event_x), f32::from(b.event_y));
                    resp = match b.detail {
                        1 => app.pointer_button(x, y, Button::Left, true),
                        2 => app.pointer_button(x, y, Button::Middle, true),
                        3 => app.pointer_button(x, y, Button::Right, true),
                        4 => app.scroll(x, y, -WHEEL_STEP),
                        5 => app.scroll(x, y, WHEEL_STEP),
                        _ => Response::NONE,
                    };
                    if resp.action == Some(Action::Move) {
                        start_move(&conn, &at, screen.root, win, &b)?;
                        resp.action = None;
                    }
                }
                Event::ButtonRelease(b) => {
                    let (x, y) = (f32::from(b.event_x), f32::from(b.event_y));
                    resp = match b.detail {
                        1 => app.pointer_button(x, y, Button::Left, false),
                        2 => app.pointer_button(x, y, Button::Middle, false),
                        3 => app.pointer_button(x, y, Button::Right, false),
                        _ => Response::NONE,
                    };
                }
                Event::FocusOut(f) if popup && app.popup_closes_on_focus_loss() && f.mode == NotifyMode::NORMAL && f.detail != NotifyDetail::INFERIOR => return Ok(()),
                Event::KeyRelease(k) => resp = app.key_release(keymap.lookup(k.detail, 0)),
                Event::KeyPress(k) => {
                    let state: u16 = k.state.into();
                    // The hotkey that opened us (Super+Space) pressed again: toggle off. Our own grab swallows it
                    // before the WM's binding sees it. (A Super+letter typed while Super is still down is just text.)
                    if popup && state & 0x40 != 0 && keymap.lookup(k.detail, 0) == 0x20 {
                        return Ok(());
                    }
                    let sym = keymap.lookup(k.detail, state);
                    let plain = keymap.lookup(k.detail, 0);
                    let (shift, ctrl, alt) = (state & 1 != 0, state & 4 != 0, state & 8 != 0);
                    if let Some(ke) = key_event(sym, plain, None, shift, ctrl, alt) {
                        resp = app.key(&ke);
                    } else {
                        resp = app.unmapped_key(sym, shift, ctrl, alt);
                    }
                }
                Event::ClientMessage(m) => {
                    if m.type_ == at.wm_protocols && m.data.as_data32()[0] == at.wm_delete {
                        return Ok(());
                    }
                    if m.type_ == at.wake {
                        resp = app.tick();
                    }
                }
                Event::MapNotify(m) if m.window == win => app.set_visible(true),
                Event::UnmapNotify(m) if m.window == win => app.set_visible(false),
                Event::DestroyNotify(_) => return Ok(()),
                Event::Error(e) => return Err(Error(format!("server error: {e:?}"))),
                _ => {}
            }
            dirty |= resp.redraw;
            if popup {
                if let Some((cw, ch)) = app.content_size() {
                    let cw = cw.min((mon.w as u32).saturating_sub(40)).max(200);
                    if (cw, ch) != size {
                        size = (cw, ch);
                        let x = popup_left(&app, &mon, cw);
                        let mut cfg = ConfigureWindowAux::new().x(x).width(cw).height(ch);
                        if app.popup_centered() || app.popup_bottom_margin().is_some() || app.popup_anchor().is_some() {
                            cfg = cfg.y(popup_top(&app, &mon, ch));
                        }
                        conn.configure_window(win, &cfg).map_err(err)?;
                        app.resize(cw, ch);
                        dirty = true;
                    }
                }
            }
            match resp.action {
                Some(Action::Close) => return Ok(()),
                Some(Action::Minimize) => send_root(&conn, &at, screen.root, win, at.wm_change_state, [3, 0, 0, 0, 0])?,
                Some(Action::ToggleMaximize) => send_root(&conn, &at, screen.root, win, at.net_wm_state, [2, at.max_vert, at.max_horz, 1, 0])?,
                Some(Action::Move) | None => {}
            }
            next = conn.poll_for_event().map_err(err)?;
        }
    }
}

/// Takes the keyboard and pointer so typing and clicking outside work whatever the WM thinks has focus.
/// The hotkey that launched us may still hold a passive keyboard grab, so retry for a moment.
/// Returns the modifier state once the keyboard is ours (`None` when it could not be grabbed).
fn grab_input(conn: &RustConnection, win: Window, _root: Window) -> Option<u16> {
    let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, win, x11rb::CURRENT_TIME);
    let mut got = false;
    for _ in 0..100 {
        got = conn.grab_keyboard(true, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.status == GrabStatus::SUCCESS);
        if got {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
    let _ = conn.grab_pointer(true, win, mask, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, x11rb::CURRENT_TIME);
    // The modifiers held right now (a quick tap may have let go of them already).
    let mods = got.then(|| conn.query_pointer(win).ok().and_then(|c| c.reply().ok()).map_or(0, |r| u16::from(r.mask)));
    let _ = conn.flush();
    mods
}

/// Politely closes another client's window (`WM_DELETE_WINDOW` when it supports it, else kills the client),
/// on its own short-lived connection.
pub fn close_window(id: u32) {
    let Ok((conn, _)) = x11rb::connect(None) else { return };
    let (Ok(protocols), Ok(delete)) = (intern(&conn, "WM_PROTOCOLS"), intern(&conn, "WM_DELETE_WINDOW")) else { return };
    let supports = conn.get_property(false, id, protocols, AtomEnum::ATOM, 0, 64).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|mut v| v.any(|a| a == delete))).unwrap_or(false);
    if supports {
        let ev = ClientMessageEvent::new(32, id, protocols, [delete, x11rb::CURRENT_TIME, 0, 0, 0]);
        let _ = conn.send_event(false, id, EventMask::NO_EVENT, ev);
    } else {
        let _ = conn.kill_client(id);
    }
    let _ = conn.flush();
    // Make sure the requests were processed before the connection (and so our pending close) goes away.
    let _ = conn.get_input_focus().map(|c| c.reply());
}

/// `WM_CLASS` value: instance (lower case) and class (first letter upper case) of an application id.
fn wm_class(app_id: &str) -> String {
    let inst = app_id.to_lowercase();
    let mut c = app_id.chars();
    let class = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
    format!("{inst}\0{class}\0")
}

/// `Xft.dpi / 96` from the root window's resource database (1.0 when unset).
pub(crate) fn xft_scale(conn: &RustConnection, root: Window) -> f32 {
    let Ok(Ok(r)) = conn.get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 65536).map(|c| c.reply()) else { return 1.0 };
    let text = String::from_utf8_lossy(&r.value);
    text.lines().find_map(|l| l.strip_prefix("Xft.dpi:")).and_then(|v| v.trim().parse::<f32>().ok()).map_or(1.0, |d| (d / 96.0).clamp(1.0, 4.0))
}

/// `CopyFromParent` depth for `create_window`.
const COPY_DEPTH: u8 = 0;

fn send_root(conn: &RustConnection, _at: &Atoms, root: Window, win: Window, ty: Atom, data: [u32; 5]) -> Result<(), Error> {
    let ev = ClientMessageEvent::new(32, win, ty, data);
    conn.send_event(false, root, EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT, ev).map_err(err)?;
    conn.flush().map_err(err)
}

/// Asks the window manager to start an interactive move (title-bar drag).
fn start_move(conn: &RustConnection, at: &Atoms, root: Window, win: Window, b: &ButtonPressEvent) -> Result<(), Error> {
    conn.ungrab_pointer(x11rb::CURRENT_TIME).map_err(err)?;
    // 8 = _NET_WM_MOVERESIZE_MOVE, button 1, source indication 1 (normal application).
    send_root(conn, at, root, win, at.moveresize, [b.root_x as u32, b.root_y as u32, 8, 1, 1])
}

/// The backdrop colour of a frame: the pixel just inside the bottom-right corner, where windows have
/// plain content background (rounded corners and the sidebar are elsewhere).
fn backdrop(pm: &Pixmap) -> Option<u32> {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    if w < 16 || h < 16 {
        return None;
    }
    let i = ((h - 8) * w + (w - 8)) * 4;
    let d = pm.data();
    (d[i + 3] == 0xff).then(|| u32::from(d[i]) << 16 | u32::from(d[i + 1]) << 8 | u32::from(d[i + 2]))
}

/// Uploads the pixmap as premultiplied BGRX, splitting into row bands when a request would be too large.
fn present(conn: &RustConnection, win: Window, gc: Gcontext, pm: &Pixmap, depth: u8, alpha: bool, max_bytes: usize) -> Result<(), Error> {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let rows_per = (max_bytes / (w * 4)).clamp(1, h.min(128));
    let mut buf = vec![0u8; w * rows_per * 4];
    let mut y = 0;
    while y < h {
        let rows = rows_per.min(h - y);
        let src = &pm.data()[y * w * 4..(y + rows) * w * 4];
        for (d, s) in buf[..rows * w * 4].chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = if alpha { s[3] } else { 0xff };
        }
        conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w as u16, rows as u16, 0, y as i16, 0, depth, &buf[..rows * w * 4]).map_err(err)?;
        y += rows;
    }
    conn.flush().map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_from_app_id() {
        assert_eq!(wm_class("MacSettings"), "macsettings\0MacSettings\0");
        assert_eq!(wm_class("machaven"), "machaven\0Machaven\0");
    }

    #[test]
    fn keymap_shift_and_caps() {
        // keycode 9 -> 'a' / 'A'
        let km = Keymap { min_keycode: 8, per_keycode: 2, syms: vec![0, 0, 0x61, 0x41] };
        assert_eq!(km.lookup(9, 0), 0x61);
        assert_eq!(km.lookup(9, 1), 0x41);
        assert_eq!(km.lookup(9, 2), 0x41, "caps lock uppercases letters");
        assert_eq!(km.lookup(9, 3), 0x41, "shift wins over caps lock for the shifted column");
        assert_eq!(km.lookup(3, 0), 0, "keycode below the minimum");
        assert_eq!(km.lookup(200, 0), 0, "keycode beyond the table");
    }
}
