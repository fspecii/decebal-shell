//! X11 backend: a 32-bit ARGB override-redirect window (needs a compositor such as picom
//! for translucency). Override-redirect keeps tiling WMs like dwm from managing it.
//!
//! The loop polls the X socket plus a wake pipe, so the app can use timers (`App::tick`)
//! and worker threads (`Waker`).

use super::{poll_readable, Anchor, SurfaceSpec, WakePipe};
use crate::app::{App, Button, InputRect, Key};
use std::fmt;
use std::os::fd::AsRawFd;
use std::time::Instant;
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// evdev/xkb keycode for Escape on virtually every X11 setup.
const KEYCODE_ESCAPE: u8 = 9;

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

/// Finds a depth-32 TrueColor visual for real alpha.
fn argb_visual(screen: &Screen) -> Option<Visualid> {
    screen.allowed_depths.iter().find(|d| d.depth == 32).and_then(|d| d.visuals.iter().find(|v| v.class == VisualClass::TRUE_COLOR)).map(|v| v.visual_id)
}

/// A rectangle on the X screen (an output / monitor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Window geometry for a requested app size on the monitor `mon`.
pub(crate) fn place(spec: &SurfaceSpec, (w, h): (u32, u32), mon: Rect) -> (i32, i32, u32, u32) {
    let width = if w == 0 { mon.w.max(1) as u32 } else { w };
    let x = mon.x + ((mon.w - width as i32) / 2).max(0);
    let y = match spec.anchor {
        Anchor::Bottom => mon.y + mon.h - h as i32 - spec.margin,
        Anchor::Top => mon.y + spec.margin,
    };
    (x, y, width, h.max(1))
}

/// `_NET_WM_STRUT_PARTIAL` (left, right, top, bottom, then start/end pairs per side) for a panel
/// that keeps `spec.reserve` pixels (plus its margin) free along the edge of monitor `mon`.
/// Struts are measured from the edge of the whole X screen, `screen` = (width, height).
/// Returns `None` for surfaces that do not reserve space.
pub(crate) fn strut_partial(spec: &SurfaceSpec, win: (i32, i32, u32, u32), mon: Rect, screen: (i32, i32)) -> Option<[u32; 12]> {
    if spec.reserve <= 0 {
        return None;
    }
    let size = (spec.reserve + spec.margin).max(0);
    let (x0, x1) = (win.0.max(0), (win.0 + win.2 as i32 - 1).max(0));
    let mut s = [0u32; 12];
    match spec.anchor {
        Anchor::Top => {
            s[2] = (mon.y + size).max(0) as u32;
            s[8] = x0 as u32;
            s[9] = x1 as u32;
        }
        Anchor::Bottom => {
            s[3] = (screen.1 - (mon.y + mon.h) + size).max(0) as u32;
            s[10] = x0 as u32;
            s[11] = x1 as u32;
        }
    }
    Some(s)
}

/// Publishes the panel's EWMH hints: window type DOCK, the strut and its legacy 4-value form.
fn set_panel_hints(conn: &RustConnection, win: Window, atoms: &Atoms, strut: Option<[u32; 12]>) -> Result<(), Error> {
    conn.change_property32(PropMode::REPLACE, win, atoms.window_type, AtomEnum::ATOM, &[atoms.dock]).map_err(err)?;
    if let Some(s) = strut {
        conn.change_property32(PropMode::REPLACE, win, atoms.strut_partial, AtomEnum::CARDINAL, &s).map_err(err)?;
        conn.change_property32(PropMode::REPLACE, win, atoms.strut, AtomEnum::CARDINAL, &s[..4]).map_err(err)?;
    }
    Ok(())
}

struct Atoms {
    window_type: Atom,
    dock: Atom,
    strut: Atom,
    strut_partial: Atom,
}

fn intern_atoms(conn: &RustConnection) -> Result<Atoms, Error> {
    let get = |name: &str| -> Result<Atom, Error> { Ok(conn.intern_atom(false, name.as_bytes()).map_err(err)?.reply().map_err(err)?.atom) };
    Ok(Atoms { window_type: get("_NET_WM_WINDOW_TYPE")?, dock: get("_NET_WM_WINDOW_TYPE_DOCK")?, strut: get("_NET_WM_STRUT")?, strut_partial: get("_NET_WM_STRUT_PARTIAL")? })
}

/// The monitor panels live on: RandR's primary, else the first active one, else the whole screen.
fn panel_monitor(conn: &RustConnection, root: Window, screen: (i32, i32)) -> Rect {
    let whole = Rect { x: 0, y: 0, w: screen.0, h: screen.1 };
    let Some(reply) = conn.randr_get_monitors(root, true).ok().and_then(|c| c.reply().ok()) else { return whole };
    let pick = reply.monitors.iter().find(|m| m.primary && m.width > 0).or_else(|| reply.monitors.iter().find(|m| m.width > 0));
    pick.map(|m| Rect { x: m.x as i32, y: m.y as i32, w: m.width as i32, h: m.height as i32 }).unwrap_or(whole)
}

fn set_input_shape(conn: &RustConnection, win: Window, rects: &[InputRect], size: (u32, u32)) -> Result<(), Error> {
    let list: Vec<Rectangle> = if rects.is_empty() {
        vec![Rectangle { x: 0, y: 0, width: size.0 as u16, height: size.1 as u16 }]
    } else {
        rects.iter().map(|r| Rectangle { x: r.x as i16, y: r.y as i16, width: r.w.max(1) as u16, height: r.h.max(1) as u16 }).collect()
    };
    conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &list).map_err(err)?;
    Ok(())
}

pub fn run<A: App>(mut app: A, spec: SurfaceSpec) -> Result<(), Error> {
    let (conn, screen_num) = x11rb::connect(None).map_err(err)?;
    let screen = conn.setup().roots[screen_num].clone();
    let visual = argb_visual(&screen).ok_or_else(|| Error("no 32-bit ARGB visual".into()))?;
    let mut screen_size = (screen.width_in_pixels as i32, screen.height_in_pixels as i32);
    let mut mon = panel_monitor(&conn, screen.root, screen_size);
    let atoms = intern_atoms(&conn)?;
    // Follow monitor hot-plug / resolution changes.
    conn.randr_select_input(screen.root, randr::NotifyMask::SCREEN_CHANGE).map_err(err)?;

    let (wake, waker) = WakePipe::new().map_err(err)?;
    app.set_waker(waker);

    let (x, y, w, h) = place(&spec, app.size(), mon);
    let colormap = conn.generate_id().map_err(err)?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual).map_err(err)?;
    let win = conn.generate_id().map_err(err)?;
    let aux = CreateWindowAux::new().background_pixel(0).border_pixel(0).override_redirect(1).colormap(colormap).event_mask(
        EventMask::EXPOSURE
            | EventMask::POINTER_MOTION
            | EventMask::LEAVE_WINDOW
            | EventMask::ENTER_WINDOW
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::KEY_PRESS
            | EventMask::STRUCTURE_NOTIFY,
    );
    conn.create_window(32, win, screen.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, visual, &aux).map_err(err)?;
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, spec.namespace.as_bytes()).map_err(err)?;

    set_panel_hints(&conn, win, &atoms, strut_partial(&spec, (x, y, w, h), mon, screen_size))?;
    let mut rects = app.input_rects();
    set_input_shape(&conn, win, &rects, (w, h))?;

    let gc = conn.generate_id().map_err(err)?;
    conn.create_gc(gc, win, &CreateGCAux::new()).map_err(err)?;
    conn.map_window(win).map_err(err)?;
    conn.flush().map_err(err)?;

    let max_bytes = conn.maximum_request_bytes().saturating_sub(64);
    let mut size = (w, h);
    let mut pos = (x, y);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error("pixmap".into()))?;
    app.resized(w, h);
    let mut dirty = true;
    let mut grabbed = false;
    let mut next_tick: Option<Instant> = Some(Instant::now());

    loop {
        // Follow app-driven changes: size, input region, keyboard grab.
        let wanted = place(&spec, app.size(), mon);
        if (wanted.2, wanted.3) != size || (wanted.0, wanted.1) != pos {
            size = (wanted.2, wanted.3);
            pos = (wanted.0, wanted.1);
            conn.configure_window(win, &ConfigureWindowAux::new().x(wanted.0).y(wanted.1).width(size.0).height(size.1)).map_err(err)?;
            set_panel_hints(&conn, win, &atoms, strut_partial(&spec, wanted, mon, screen_size))?;
            if let Some(p) = Pixmap::new(size.0.max(1), size.1.max(1)) {
                pm = p;
            }
            app.resized(size.0, size.1);
            dirty = true;
        }
        let new_rects = app.input_rects();
        if new_rects != rects {
            rects = new_rects;
            set_input_shape(&conn, win, &rects, size)?;
            dirty = true;
        }
        let want_kb = app.wants_keyboard();
        if want_kb && !grabbed {
            let reply = conn.grab_keyboard(false, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC).map_err(err)?.reply().map_err(err)?;
            grabbed = reply.status == GrabStatus::SUCCESS;
        } else if !want_kb && grabbed {
            conn.ungrab_keyboard(x11rb::CURRENT_TIME).map_err(err)?;
            grabbed = false;
        }

        if dirty {
            dirty = false;
            app.draw(&mut pm);
            present(&conn, win, gc, &pm, max_bytes)?;
        } else {
            conn.flush().map_err(err)?;
        }

        // Drain everything already queued before sleeping.
        let mut handled = false;
        while let Some(ev) = conn.poll_for_event().map_err(err)? {
            handled = true;
            match ev {
                Event::Expose(_) => dirty = true,
                Event::MotionNotify(m) => dirty |= app.pointer_motion(m.event_x as f32, m.event_y as f32),
                Event::EnterNotify(m) => dirty |= app.pointer_motion(m.event_x as f32, m.event_y as f32),
                Event::LeaveNotify(_) => dirty |= app.pointer_leave(),
                Event::ButtonPress(b) => {
                    let (ex, ey) = (b.event_x as f32, b.event_y as f32);
                    dirty |= match b.detail {
                        1 => app.pointer_button(ex, ey, Button::Left),
                        2 => app.pointer_button(ex, ey, Button::Middle),
                        3 => app.pointer_button(ex, ey, Button::Right),
                        4 => app.scroll(ex, ey, -1.0),
                        5 => app.scroll(ex, ey, 1.0),
                        _ => false,
                    };
                }
                Event::ButtonRelease(b) => {
                    let (ex, ey) = (b.event_x as f32, b.event_y as f32);
                    dirty |= match b.detail {
                        1 => app.pointer_release(ex, ey, Button::Left),
                        2 => app.pointer_release(ex, ey, Button::Middle),
                        3 => app.pointer_release(ex, ey, Button::Right),
                        _ => false,
                    };
                }
                Event::KeyPress(k) if k.detail == KEYCODE_ESCAPE => dirty |= app.key(Key::Escape),
                Event::RandrScreenChangeNotify(_) => {
                    // Re-read the screen and monitor; the placement check at the top of the loop moves
                    // the window and republishes the strut.
                    if let Some(g) = conn.get_geometry(screen.root).ok().and_then(|c| c.reply().ok()) {
                        screen_size = (g.width as i32, g.height as i32);
                    }
                    mon = panel_monitor(&conn, screen.root, screen_size);
                }
                Event::DestroyNotify(_) => return Ok(()),
                Event::Error(e) => return Err(Error(format!("server error: {e:?}"))),
                _ => {}
            }
        }
        if handled || dirty {
            continue;
        }

        let timeout = next_tick.map(|t| t.saturating_duration_since(Instant::now()));
        let ready = poll_readable(&[conn.stream().as_raw_fd(), wake.fd()], timeout).map_err(err)?;
        if ready[1] {
            wake.drain();
        }
        let due = next_tick.map(|t| Instant::now() >= t).unwrap_or(false);
        if ready[1] || due {
            let tick = app.tick();
            dirty |= tick.redraw;
            next_tick = tick.next.map(|d| Instant::now() + d);
        }
    }
}

/// Uploads the pixmap as premultiplied BGRA, splitting into row bands when a request would be too large.
fn present(conn: &RustConnection, win: Window, gc: Gcontext, pm: &Pixmap, max_bytes: usize) -> Result<(), Error> {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let rows_per = (max_bytes / (w * 4)).clamp(1, h);
    let mut buf = vec![0u8; w * rows_per * 4];
    let mut y = 0;
    while y < h {
        let rows = rows_per.min(h - y);
        let src = &pm.data()[y * w * 4..(y + rows) * w * 4];
        for (d, s) in buf[..rows * w * 4].chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w as u16, rows as u16, 0, y as i16, 0, 32, &buf[..rows * w * 4]).map_err(err)?;
        y += rows;
    }
    conn.flush().map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(anchor: Anchor, margin: i32, stretch: bool) -> SurfaceSpec {
        SurfaceSpec { anchor, margin, reserve: 0, stretch, namespace: "t" }
    }

    fn mon(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn dock_is_centred_at_bottom() {
        let (x, y, w, h) = place(&spec(Anchor::Bottom, 13, false), (400, 100), mon(0, 0, 1600, 900));
        assert_eq!((x, y, w, h), (600, 787, 400, 100));
    }

    #[test]
    fn bar_stretches_to_screen_width_at_top() {
        let (x, y, w, h) = place(&spec(Anchor::Top, 0, true), (0, 26), mon(0, 0, 1920, 1080));
        assert_eq!((x, y, w, h), (0, 0, 1920, 26));
    }

    #[test]
    fn growing_a_top_bar_keeps_its_origin() {
        let (_, y1, _, _) = place(&spec(Anchor::Top, 0, true), (0, 26), mon(0, 0, 1920, 1080));
        let (_, y2, _, h2) = place(&spec(Anchor::Top, 0, true), (0, 400), mon(0, 0, 1920, 1080));
        assert_eq!((y1, y2, h2), (0, 0, 400));
    }

    #[test]
    fn panels_follow_their_monitor() {
        // second monitor to the right of a 1920 wide one
        let m = mon(1920, 100, 1280, 720);
        assert_eq!(place(&spec(Anchor::Top, 0, true), (0, 26), m), (1920, 100, 1280, 26));
        assert_eq!(place(&spec(Anchor::Bottom, 13, false), (400, 100), m), (2360, 707, 400, 100));
    }

    fn reserving(anchor: Anchor, margin: i32, reserve: i32) -> SurfaceSpec {
        SurfaceSpec { anchor, margin, reserve, stretch: false, namespace: "t" }
    }

    #[test]
    fn bar_strut_is_its_height_over_the_bar_width() {
        let s = reserving(Anchor::Top, 0, 26);
        let m = mon(0, 0, 1920, 1080);
        let win = place(&s, (0, 420), m);
        assert_eq!(strut_partial(&s, win, m, (1920, 1080)), Some([0, 0, 26, 0, 0, 0, 0, 0, 0, 1919, 0, 0]));
    }

    #[test]
    fn dock_strut_includes_margin_and_covers_only_the_dock() {
        let s = reserving(Anchor::Bottom, 13, 71);
        let m = mon(0, 0, 1600, 900);
        let win = place(&s, (400, 100), m);
        assert_eq!(strut_partial(&s, win, m, (1600, 900)), Some([0, 0, 0, 84, 0, 0, 0, 0, 0, 0, 600, 999]));
    }

    #[test]
    fn struts_are_measured_from_the_screen_edge_not_the_monitor() {
        // 1920x1080 left monitor, 1280x720 right monitor lower down: screen is 3200x1080.
        let m = mon(1920, 360, 1280, 720);
        let bar = reserving(Anchor::Top, 0, 26);
        let bw = place(&bar, (0, 420), m);
        // the top edge of the monitor is 360 below the screen top, so the strut spans that gap too
        assert_eq!(strut_partial(&bar, bw, m, (3200, 1080)).unwrap()[2], 386);
        assert_eq!(&strut_partial(&bar, bw, m, (3200, 1080)).unwrap()[8..10], &[1920, 3199]);
        let dock = reserving(Anchor::Bottom, 13, 71);
        let dw = place(&dock, (400, 100), m);
        // monitor bottom is flush with the screen bottom
        assert_eq!(strut_partial(&dock, dw, m, (3200, 1080)).unwrap()[3], 84);
    }

    #[test]
    fn floating_surfaces_reserve_nothing() {
        let s = spec(Anchor::Top, 0, true);
        let m = mon(0, 0, 100, 100);
        assert_eq!(strut_partial(&s, place(&s, (0, 10), m), m, (100, 100)), None);
    }
}
