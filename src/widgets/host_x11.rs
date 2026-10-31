//! The X11 backend of the `decwidgets` daemon: one small X11 window per desktop widget, kept at the bottom of the stack.
//!
//! The windows are override-redirect (rdwm never sees or tiles them) and sit below every application window. With a compositor
//! they get a 32-bit visual and real alpha; without one the visible shape is cut out of the window with the SHAPE extension, so rounded
//! cards look right either way. The loop sleeps in `poll(2)` until an X event, a data source or the refresh timer wakes it.

use super::config::{self, Instance, Place, Resolved, WidgetsConfig};
use super::data::{Data, Live};
use super::host::{data_with_options, dots_under, lay, theme_stamp, Paths};
use super::layout::{self, Ctx, Item};
use super::paint::{self, FontMetrics, ImageCache, Painter};
use crate::backend::x11::{self as bx, Rect as MonRect};
use crate::backend::{poll_readable, WakePipe};
use crate::menubar::symbols::SymbolCache;
use crate::text::Fonts;
use std::collections::BTreeSet;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

struct Win {
    id: Window,
    gc: Gcontext,
    inst: Instance,
    res: Resolved,
    w: u32,
    h: u32,
    item: Option<Item>,
    images: ImageCache,
    argb: bool,
    /// Without a compositor: what the root window showed under the card when it was created, so translucent parts blend with it.
    backdrop: Option<Pixmap>,
    depth: u8,
    colormap: Option<Colormap>,
    glass: bool,
    native: bool,
    position: (i32, i32),
    renderer: crate::backend::glass::Renderer,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// True only once the host on this X display has completed its first render.
pub fn ready() -> bool {
    let Ok((conn, screen)) = crate::x11_connection::connect(None) else { return false };
    let name = format!("_DECEBAL_WIDGET_HOST_S{screen}");
    let Some(atom) = conn.intern_atom(true, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).filter(|a| *a != 0) else { return false };
    conn.get_selection_owner(atom).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.owner != 0)
}

pub fn run(paths: Paths) -> Result<(), String> {
    let (conn, screen_num) = crate::x11_connection::connect(None).map_err(err)?;
    let screen = conn.setup().roots[screen_num].clone();
    let root = screen.root;
    conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE)).map_err(err)?;
    let (pipe, waker) = WakePipe::new().map_err(err)?;
    {
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w.wake());
    }
    let ready_atom = conn.intern_atom(false, format!("_DECEBAL_WIDGET_HOST_S{screen_num}").as_bytes()).map_err(err)?.reply().map_err(err)?.atom;
    let ready_window = conn.generate_id().map_err(err)?;
    conn.create_window(0, ready_window, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).map_err(err)?;
    let mut announced = false;
    let mut fonts = Fonts::load().ok_or("no usable font found")?;
    let mut symbols = SymbolCache::default();
    let max_bytes = conn.maximum_request_bytes().saturating_sub(64);
    let mut compositor = bx::has_compositor(&conn, screen_num);
    let mut argb_visual = if compositor { bx::argb_visual(&screen) } else { None };

    let mut wins: Vec<Win> = Vec::new();
    let mut live: Option<Live> = None;
    let mut data = Data::empty();
    let mut stamp = (crate::config::Stamp::of(&paths.shell_json), 0u64);
    let mut next_refresh = Instant::now();
    let mut rebuild = true;

    loop {
        if rebuild {
            rebuild = false;
            for w in wins.drain(..) {
                let _ = conn.free_gc(w.gc);
                let _ = conn.destroy_window(w.id);
                if let Some(c) = w.colormap {
                    let _ = conn.free_colormap(c);
                }
            }
            let cfg = WidgetsConfig::load(&paths.shell_json);
            let mon = bx::panel_monitor(&conn, root, (i32::from(screen.width_in_pixels), i32::from(screen.height_in_pixels)));
            let mut used: BTreeSet<String> = BTreeSet::new();
            let mut commands = Vec::new();
            for inst in cfg.items.iter().filter(|i| cfg.enabled && i.enabled && i.place == Place::Desktop) {
                match config::resolve(inst, &paths.themes_dir, cfg.allow_exec) {
                    Ok(res) => {
                        used.extend(res.def.paths.iter().cloned());
                        commands.extend(res.def.commands.iter().cloned());
                        wins.push(new_window(&conn, &screen, argb_visual, inst, res, mon, &data, &fonts)?);
                    }
                    Err(e) => eprintln!("decwidgets: {}: {e}", inst.use_),
                }
            }
            // A rebuild (new theme, wallpaper or colours) usually leaves the data the cards need unchanged: keep the running
            // collectors and their history. Only a different set of data groups or commands replaces (and so stops) them.
            live = match live.take() {
                _ if wins.is_empty() => None,
                Some(l) if l.matches(&used, &commands) => Some(l),
                _ => Some(Live::new(&used, &commands, waker.clone())),
            };
            if wins.is_empty() {
                fonts.trim();
                crate::mem::trim();
            }
            stamp.1 = theme_stamp(&paths);
            stack_widgets(&conn, root, &wins)?;
            conn.flush().map_err(err)?;
            next_refresh = Instant::now();
        }

        // redraw
        if Instant::now() >= next_refresh {
            if let Some(l) = live.as_mut() {
                l.update(&mut data);
            }
            let th = crate::theme::th();
            for w in wins.iter_mut() {
                draw(&conn, &screen, w, &data, &th, &mut fonts, &mut symbols, max_bytes, compositor)?;
            }
            if !announced {
                conn.set_selection_owner(ready_window, ready_atom, x11rb::CURRENT_TIME).map_err(err)?;
                announced = true;
            }
            let ms = wins.iter().map(|w| w.res.def.refresh_ms).min().unwrap_or(1000);
            let ms = if crate::glass::Material::from_theme(&th).enabled() { ms.min(250) } else { ms };
            next_refresh = Instant::now() + Duration::from_millis(u64::from(ms));
        }

        // Small cards may fit entirely in x11rb's output buffer. Always submit
        // drawing, shape and readiness requests before sleeping for input.
        conn.flush().map_err(err)?;
        let timeout = next_refresh.saturating_duration_since(Instant::now()).min(Duration::from_millis(500));
        let ready = poll_readable(&[conn.stream().as_raw_fd(), pipe.fd()], Some(timeout)).map_err(err)?;
        if ready[1] {
            pipe.drain();
        }
        let mut redraw = ready[1] && crate::theme::take_changed();
        while let Some(ev) = conn.poll_for_event().map_err(err)? {
            match ev {
                Event::Expose(e) if e.count == 0 => redraw = true,
                Event::ButtonPress(b) => {
                    let Some(w) = wins.iter().find(|w| w.id == b.event) else { continue };
                    let Some(item) = &w.item else { continue };
                    let (x, y) = (f32::from(b.event_x), f32::from(b.event_y));
                    match b.detail {
                        1 => {
                            if let Some(hit) = layout::hit(item, x, y) {
                                if let Some(a) = &hit.on_click {
                                    super::actions::run(a);
                                }
                            } else if let Some(n) = dots_under(item, x, y) {
                                super::actions::run(&super::actions::Action::Workspace(super::actions::Ws::N(n)));
                            }
                        }
                        4 | 5 => {
                            if let Some(a) = layout::hit_scroll(item, x, y, b.detail == 4).and_then(|h| if b.detail == 4 { h.on_scroll_up.as_ref() } else { h.on_scroll_down.as_ref() }) {
                                super::actions::run(a);
                            }
                        }
                        _ => {}
                    }
                }
                Event::PropertyNotify(p) if p.window == root => {
                    let wallpaper = conn.intern_atom(true, b"_XROOTPMAP_ID").map_err(err)?.reply().map_err(err)?.atom;
                    if p.atom == wallpaper {
                        rebuild = true;
                    }
                    stack_widgets(&conn, root, &wins)?;
                }
                Event::ConfigureNotify(c) => {
                    if let Some(w) = wins.iter_mut().find(|w| w.id == c.window) {
                        w.position = (i32::from(c.x), i32::from(c.y));
                    }
                    stack_widgets(&conn, root, &wins)?;
                }
                Event::MapNotify(_) | Event::DestroyNotify(_) => stack_widgets(&conn, root, &wins)?,
                _ => {}
            }
        }
        // Login services may start the compositor after this watcher, and a
        // compositor can restart. Recreate surfaces with the appropriate visual.
        let active_compositor = bx::has_compositor(&conn, screen_num);
        if active_compositor != compositor {
            compositor = active_compositor;
            argb_visual = if compositor { bx::argb_visual(&screen) } else { None };
            rebuild = true;
        }
        // the config or the selected theme changed on disk
        let now = (crate::config::Stamp::of(&paths.shell_json), theme_stamp(&paths));
        if now != stamp {
            stamp = now;
            rebuild = true;
        }
        if redraw {
            next_refresh = Instant::now();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn new_window(conn: &RustConnection, screen: &Screen, argb_visual: Option<u32>, inst: &Instance, res: Resolved, mon: MonRect, data: &Data, fonts: &Fonts) -> Result<Win, String> {
    let d = data_with_options(data, inst);
    let item = lay(&res.def, &d, fonts);
    let (w, h) = (item.rect.w.ceil().max(8.0) as u32, item.rect.h.ceil().max(8.0) as u32);
    let (x, y) = config::place_clear(&inst.anchor, inst.x, inst.y, (mon.x, mon.y, mon.w, mon.h), w as i32, h as i32, crate::theme::th().bar_insets());
    // taken before the window exists, so the snapshot is the desktop and not our own black rectangle
    let backdrop = if argb_visual.is_some() { None } else { snapshot(conn, screen.root, x, y, w, h) };
    let id = conn.generate_id().map_err(err)?;
    let mask = EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::STRUCTURE_NOTIFY;
    let mut aux = CreateWindowAux::new().override_redirect(1).event_mask(mask).background_pixel(screen.black_pixel);
    let mut colormap = None;
    match argb_visual {
        Some(visual) => {
            let cmap = conn.generate_id().map_err(err)?;
            colormap = Some(cmap);
            conn.create_colormap(ColormapAlloc::NONE, cmap, screen.root, visual).map_err(err)?;
            aux = aux.background_pixel(0).border_pixel(0).colormap(cmap);
            conn.create_window(32, id, screen.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, visual, &aux).map_err(err)?;
        }
        None => {
            conn.create_window(0, id, screen.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(err)?;
        }
    }
    let name = format!("decwidgets: {}", res.def.name);
    conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, name.as_bytes()).map_err(err)?;
    conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_CLASS, AtomEnum::STRING, b"decwidgets\0decwidgets\0").map_err(err)?;
    // These override-redirect cards are desktop content, not popup overlays.
    // Publish their standard stacking role before mapping so a shell frame can
    // stay above them without ever covering unknown popups or screen lockers.
    let state = conn.intern_atom(false, b"_NET_WM_STATE").map_err(err)?.reply().map_err(err)?.atom;
    let below = conn.intern_atom(false, b"_NET_WM_STATE_BELOW").map_err(err)?.reply().map_err(err)?.atom;
    conn.change_property32(PropMode::REPLACE, id, state, AtomEnum::ATOM, &[below]).map_err(err)?;
    let gc = conn.generate_id().map_err(err)?;
    conn.create_gc(gc, id, &CreateGCAux::new()).map_err(err)?;
    let glass = crate::theme::th().blur > 0.0;
    bx::set_glass_hint(conn, id, glass).map_err(err)?;
    bx::set_native_glass_hint(conn, id, false).map_err(err)?;
    conn.map_window(id).map_err(err)?;
    let base = res.base.clone();
    Ok(Win {
        id,
        gc,
        inst: inst.clone(),
        res,
        w,
        h,
        item: Some(item),
        images: ImageCache::new(&base),
        argb: argb_visual.is_some(),
        backdrop,
        depth: screen.root_depth,
        colormap,
        glass,
        native: false,
        position: (x, y),
        renderer: Default::default(),
    })
}

/// Desktop cards belong above wallpaper windows, below managed applications. Absolute
/// lowering can hide them behind an external DESKTOP wallpaper window.
fn stack_widgets(conn: &RustConnection, root: Window, wins: &[Win]) -> Result<(), String> {
    if wins.is_empty() {
        return Ok(());
    }
    let kind = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE").map_err(err)?.reply().map_err(err)?.atom;
    let desktop = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DESKTOP").map_err(err)?.reply().map_err(err)?.atom;
    let clients = conn.intern_atom(false, b"_NET_CLIENT_LIST").map_err(err)?.reply().map_err(err)?.atom;
    let managed: Vec<u32> = conn.get_property(false, root, clients, AtomEnum::WINDOW, 0, 65536).map_err(err)?.reply().map_err(err)?.value32().map(|v| v.collect()).unwrap_or_default();
    let tree = conn.query_tree(root).map_err(err)?.reply().map_err(err)?.children;
    let desktops: Vec<_> = tree
        .iter()
        .copied()
        .filter(|id| !wins.iter().any(|w| w.id == *id))
        .filter(|id| {
            if !conn.get_window_attributes(*id).ok().and_then(|c| c.reply().ok()).is_some_and(|a| a.map_state == MapState::VIEWABLE) {
                return false;
            }
            conn.get_property(false, *id, kind, AtomEnum::ATOM, 0, 16).ok().and_then(|r| r.reply().ok()).and_then(|p| p.value32().map(|mut v| v.any(|a| a == desktop))).unwrap_or(false)
        })
        .collect();
    let wallpaper = desktops.last().copied();
    let first_app = tree
        .iter()
        .copied()
        .find(|id| managed.contains(id) && !desktops.contains(id) && conn.get_window_attributes(*id).ok().and_then(|c| c.reply().ok()).is_some_and(|a| a.map_state == MapState::VIEWABLE));
    let rank = |id| tree.iter().position(|w| *w == id);
    if let Some(bg) = wallpaper.filter(|bg| first_app.is_none_or(|app| rank(*bg) < rank(app))) {
        let base = rank(bg).unwrap();
        let ordered = wins.iter().enumerate().all(|(i, w)| rank(w.id) == Some(base + i + 1));
        if !ordered {
            let mut sibling = bg;
            for w in wins {
                conn.configure_window(w.id, &ConfigureWindowAux::new().sibling(sibling).stack_mode(StackMode::ABOVE)).map_err(err)?;
                sibling = w.id;
            }
        }
    } else if let Some(app) = first_app {
        for w in wins {
            if rank(w.id) > rank(app) {
                conn.configure_window(w.id, &ConfigureWindowAux::new().sibling(app).stack_mode(StackMode::BELOW)).map_err(err)?;
            }
        }
    } else if !wins.iter().enumerate().all(|(i, w)| rank(w.id) == Some(i)) {
        // A bare root pixmap is not a window. Keep cards below shell panels and
        // application popups even before the first managed app is launched.
        for w in wins.iter().rev() {
            conn.configure_window(w.id, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW)).map_err(err)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw(
    conn: &RustConnection,
    screen: &Screen,
    w: &mut Win,
    base: &Data,
    th: &crate::theme::Theme,
    fonts: &mut Fonts,
    symbols: &mut SymbolCache,
    max_bytes: usize,
    compositor: bool,
) -> Result<(), String> {
    let data = data_with_options(base, &w.inst);
    let item = layout::layout(&w.res.def, &Ctx { data: &data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, Some((w.w as f32, w.h as f32)));
    let mut pm = Pixmap::new(w.w, w.h).ok_or("pixmap")?;
    paint::paint(&mut pm, &item, &mut Painter { fonts, symbols, images: &mut w.images, theme: th, background_opacity: paint::background_opacity(&data) }, 1.0);
    // Keep parsed metrics while cards are active; trim when the collection is empty.
    let depth = if w.argb { 32 } else { w.depth };
    let mut material = crate::glass::Material::from_theme(th);
    let background_visible = paint::background_opacity(&data) > 0.0;
    if !background_visible {
        material.refraction = 0.0;
    }
    // Preserve foreground coverage before native glass makes its interior opaque.
    let shape = if material.enabled() || (th.blur > 0.0 && w.argb) { Some(bx::glass_rects(&pm)) } else { None };
    let r = item.rect;
    let native = w.renderer.paint(conn, screen, w.id, w.position, &mut pm, &[crate::glass::Region { x: r.x, y: r.y, w: r.w, h: r.h, radius: item.radius }], material);
    let glass = th.blur > 0.0 && !native && background_visible;
    if glass != w.glass {
        bx::set_glass_hint(conn, w.id, glass).map_err(err)?;
        w.glass = glass;
    }
    if native != w.native {
        bx::set_native_glass_hint(conn, w.id, native).map_err(err)?;
        w.native = native;
    }
    if let Some(rects) = &shape {
        conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, w.id, 0, 0, rects).map_err(err)?;
    }
    if !w.argb {
        // no alpha channel: blend the card over what the desktop showed there, and cut the window to the card's shape
        let rects = shape.clone().unwrap_or_else(|| bx::visible_rects(&pm));
        conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, w.id, 0, 0, &rects).map_err(err)?;
        let mut out = w.backdrop.clone().unwrap_or_else(|| {
            let mut p = Pixmap::new(w.w, w.h).expect("size checked above");
            p.fill(th.background.color(1.0));
            p
        });
        out.draw_pixmap(0, 0, pm.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        pm = out;
    }
    crate::backend::x11_window::present(conn, w.id, w.gc, &pm, depth, w.argb, max_bytes).map_err(err)?;
    let _ = compositor;
    w.item = Some(item);
    Ok(())
}

/// The pixels of the root window under (x, y, w, h), or `None` when the area is off screen or the format is unexpected.
fn snapshot(conn: &RustConnection, root: Window, x: i32, y: i32, w: u32, h: u32) -> Option<Pixmap> {
    let (x, y) = (i16::try_from(x).ok()?, i16::try_from(y).ok()?);
    // Read the wallpaper pixmap rather than root's composited children (which
    // could include an application or another widget at this location).
    let atom = conn.intern_atom(true, b"_XROOTPMAP_ID").ok()?.reply().ok()?.atom;
    let wallpaper = if atom == 0 { None } else { conn.get_property(false, root, atom, AtomEnum::PIXMAP, 0, 1).ok().and_then(|c| c.reply().ok()).and_then(|p| p.value32().and_then(|mut v| v.next())) };
    let img = conn.get_image(ImageFormat::Z_PIXMAP, wallpaper.unwrap_or(root), x, y, w as u16, h as u16, !0).ok()?.reply().ok()?;
    if img.data.len() != (w * h * 4) as usize {
        return None;
    }
    let mut pm = Pixmap::new(w, h)?;
    for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(img.data.chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], 255]);
    }
    Some(pm)
}
