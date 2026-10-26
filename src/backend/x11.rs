//! X11 backend: a 32-bit ARGB override-redirect window (needs a compositor such as picom
//! for translucency). Override-redirect keeps tiling WMs like dwm from managing it.

use super::{Anchor, SurfaceSpec};
use crate::app::{App, Button};
use std::fmt;
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;

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
    screen
        .allowed_depths
        .iter()
        .find(|d| d.depth == 32)
        .and_then(|d| d.visuals.iter().find(|v| v.class == VisualClass::TRUE_COLOR))
        .map(|v| v.visual_id)
}

pub fn run<A: App>(mut app: A, spec: SurfaceSpec) -> Result<(), Error> {
    let (conn, screen_num) = x11rb::connect(None).map_err(err)?;
    let screen = conn.setup().roots[screen_num].clone();
    let visual = argb_visual(&screen).ok_or_else(|| Error("no 32-bit ARGB visual".into()))?;

    let (w, h) = app.size();
    let (sw, sh) = (screen.width_in_pixels as i32, screen.height_in_pixels as i32);
    let x = ((sw - w as i32) / 2).max(0);
    let y = match spec.anchor {
        Anchor::Bottom => sh - h as i32 - spec.margin,
        Anchor::Top => spec.margin,
    };

    let colormap = conn.generate_id().map_err(err)?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual).map_err(err)?;
    let win = conn.generate_id().map_err(err)?;
    let aux = CreateWindowAux::new()
        .background_pixel(0)
        .border_pixel(0)
        .override_redirect(1)
        .colormap(colormap)
        .event_mask(
            EventMask::EXPOSURE
                | EventMask::POINTER_MOTION
                | EventMask::LEAVE_WINDOW
                | EventMask::ENTER_WINDOW
                | EventMask::BUTTON_PRESS
                | EventMask::STRUCTURE_NOTIFY,
        );
    conn.create_window(32, win, screen.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, visual, &aux)
        .map_err(err)?;
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, spec.namespace.as_bytes()).map_err(err)?;

    // Only the panel strip takes input; the transparent headroom lets clicks fall through.
    if let Some(r) = app.input_rect() {
        let rect = Rectangle { x: r.x as i16, y: r.y as i16, width: r.w as u16, height: r.h as u16 };
        conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[rect]).map_err(err)?;
    }

    let gc = conn.generate_id().map_err(err)?;
    conn.create_gc(gc, win, &CreateGCAux::new()).map_err(err)?;
    conn.map_window(win).map_err(err)?;
    conn.flush().map_err(err)?;

    let max_bytes = conn.maximum_request_bytes().saturating_sub(64);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error("pixmap".into()))?;
    let mut dirty = true;

    loop {
        if dirty {
            dirty = false;
            app.draw(&mut pm);
            present(&conn, win, gc, &pm, max_bytes)?;
        }
        let event = conn.wait_for_event().map_err(err)?;
        let mut next = Some(event);
        // Drain queued events so a burst of motion triggers one redraw.
        while let Some(ev) = next.take() {
            match ev {
                Event::Expose(_) => dirty = true,
                Event::MotionNotify(m) => dirty |= app.pointer_motion(m.event_x as f32, m.event_y as f32),
                Event::EnterNotify(m) => dirty |= app.pointer_motion(m.event_x as f32, m.event_y as f32),
                Event::LeaveNotify(_) => dirty |= app.pointer_leave(),
                Event::ButtonPress(b) => {
                    let btn = match b.detail {
                        1 => Button::Left,
                        2 => Button::Middle,
                        3 => Button::Right,
                        _ => continue,
                    };
                    dirty |= app.pointer_button(b.event_x as f32, b.event_y as f32, btn);
                }
                Event::DestroyNotify(_) => return Ok(()),
                Event::Error(e) => return Err(Error(format!("server error: {e:?}"))),
                _ => {}
            }
            next = conn.poll_for_event().map_err(err)?;
        }
    }
}

/// Uploads the pixmap as premultiplied BGRA, splitting into row bands when a request would be too large.
fn present<C: Connection>(conn: &C, win: Window, gc: Gcontext, pm: &Pixmap, max_bytes: usize) -> Result<(), Error> {
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
