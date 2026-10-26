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

/// Window geometry for a requested app size on a `sw` x `sh` screen.
pub(crate) fn place(spec: &SurfaceSpec, (w, h): (u32, u32), sw: i32, sh: i32) -> (i32, i32, u32, u32) {
    let width = if w == 0 { sw.max(1) as u32 } else { w };
    let x = ((sw - width as i32) / 2).max(0);
    let y = match spec.anchor {
        Anchor::Bottom => sh - h as i32 - spec.margin,
        Anchor::Top => spec.margin,
    };
    (x, y, width, h.max(1))
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
    let (sw, sh) = (screen.width_in_pixels as i32, screen.height_in_pixels as i32);

    let (wake, waker) = WakePipe::new().map_err(err)?;
    app.set_waker(waker);

    let (x, y, w, h) = place(&spec, app.size(), sw, sh);
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

    let mut rects = app.input_rects();
    set_input_shape(&conn, win, &rects, (w, h))?;

    let gc = conn.generate_id().map_err(err)?;
    conn.create_gc(gc, win, &CreateGCAux::new()).map_err(err)?;
    conn.map_window(win).map_err(err)?;
    conn.flush().map_err(err)?;

    let max_bytes = conn.maximum_request_bytes().saturating_sub(64);
    let mut size = (w, h);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error("pixmap".into()))?;
    app.resized(w, h);
    let mut dirty = true;
    let mut grabbed = false;
    let mut next_tick: Option<Instant> = Some(Instant::now());

    loop {
        // Follow app-driven changes: size, input region, keyboard grab.
        let wanted = place(&spec, app.size(), sw, sh);
        if (wanted.2, wanted.3) != size {
            size = (wanted.2, wanted.3);
            conn.configure_window(win, &ConfigureWindowAux::new().x(wanted.0).y(wanted.1).width(size.0).height(size.1)).map_err(err)?;
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

    #[test]
    fn dock_is_centred_at_bottom() {
        let (x, y, w, h) = place(&spec(Anchor::Bottom, 13, false), (400, 100), 1600, 900);
        assert_eq!((x, y, w, h), (600, 787, 400, 100));
    }

    #[test]
    fn bar_stretches_to_screen_width_at_top() {
        let (x, y, w, h) = place(&spec(Anchor::Top, 0, true), (0, 26), 1920, 1080);
        assert_eq!((x, y, w, h), (0, 0, 1920, 26));
    }

    #[test]
    fn growing_a_top_bar_keeps_its_origin() {
        let (_, y1, _, _) = place(&spec(Anchor::Top, 0, true), (0, 26), 1920, 1080);
        let (_, y2, _, h2) = place(&spec(Anchor::Top, 0, true), (0, 400), 1920, 1080);
        assert_eq!((y1, y2, h2), (0, 0, 400));
    }
}
