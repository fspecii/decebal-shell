//! The `macwidgets` daemon: one small X11 window per desktop widget, kept at the bottom of the stack.
//!
//! The windows are override-redirect (rdwm never sees or tiles them) and sit below every application window. With a compositor
//! they get a 32-bit visual and real alpha; without one the visible shape is cut out of the window with the SHAPE extension, so rounded
//! cards look right either way. The loop sleeps in `poll(2)` until an X event, a data source or the refresh timer wakes it.

use super::config::{self, Instance, Place, Resolved, WidgetsConfig};
use super::data::{Data, Live};
use super::dsl::WidgetDef;
use super::layout::{self, Ctx, Item};
use super::paint::{self, FontMetrics, ImageCache, Painter};
use crate::backend::x11::{self as bx, Rect as MonRect};
use crate::backend::{poll_readable, WakePipe};
use crate::menubar::symbols::SymbolCache;
use crate::text::Fonts;
use std::collections::BTreeSet;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

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
}

pub struct Paths {
    pub shell_json: PathBuf,
    pub themes_dir: PathBuf,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Size of the card for the current data and fonts.
fn lay(def: &WidgetDef, data: &Data, fonts: &Fonts) -> Item {
    let th = crate::theme::th();
    layout::layout(def, &Ctx { data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, None)
}

fn data_with_options(base: &Data, inst: &Instance) -> Data {
    let mut d = base.clone();
    for (k, v) in &inst.options {
        let val = match v {
            serde_json::Value::String(s) => super::data::Val::Text(s.clone()),
            serde_json::Value::Number(n) => super::data::Val::Num(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::Bool(b) => super::data::Val::Bool(*b),
            _ => continue,
        };
        d.set(&format!("opt.{k}"), val);
    }
    d
}

pub fn run(paths: Paths) -> Result<(), String> {
    let (conn, screen_num) = x11rb::connect(None).map_err(err)?;
    let screen = conn.setup().roots[screen_num].clone();
    let root = screen.root;
    let (pipe, waker) = WakePipe::new().map_err(err)?;
    {
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w.wake());
    }
    let mut fonts = Fonts::load().ok_or("no usable font found")?;
    let mut symbols = SymbolCache::default();
    let max_bytes = conn.maximum_request_bytes().saturating_sub(64);
    let compositor = bx::has_compositor(&conn, screen_num);
    let argb_visual = if compositor { bx::argb_visual(&screen) } else { None };

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
                let _ = conn.destroy_window(w.id);
            }
            let cfg = WidgetsConfig::load(&paths.shell_json);
            let mon = bx::panel_monitor(&conn, root, (i32::from(screen.width_in_pixels), i32::from(screen.height_in_pixels)));
            let mut used: BTreeSet<String> = BTreeSet::new();
            let mut commands = Vec::new();
            for inst in cfg.items.iter().filter(|i| i.enabled && i.place == Place::Desktop) {
                match config::resolve(inst, &paths.themes_dir, cfg.allow_exec) {
                    Ok(res) => {
                        used.extend(res.def.paths.iter().cloned());
                        commands.extend(res.def.commands.iter().cloned());
                        wins.push(new_window(&conn, &screen, argb_visual, inst, res, mon, &data, &fonts)?);
                    }
                    Err(e) => eprintln!("macwidgets: {}: {e}", inst.use_),
                }
            }
            live = Some(Live::new(&used, &commands, waker.clone()));
            stamp.1 = theme_stamp(&paths);
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
                draw(&conn, w, &data, &th, &mut fonts, &mut symbols, max_bytes, compositor)?;
            }
            let ms = wins.iter().map(|w| w.res.def.refresh_ms).min().unwrap_or(1000);
            next_refresh = Instant::now() + Duration::from_millis(u64::from(ms));
        }

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
                _ => {}
            }
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

/// Mtimes of the widget definition files in use, so editing a widget redraws it without a restart.
fn theme_stamp(paths: &Paths) -> u64 {
    let cfg = WidgetsConfig::load(&paths.shell_json);
    let mut sum = 0u64;
    for i in &cfg.items {
        let file = match &i.source {
            Some(s) => config::package_dir(&paths.themes_dir, s).map(|d| d.join("widgets").join(format!("{}.toml", i.use_))),
            None => Some(config::user_dir().join(format!("{}.toml", i.use_))),
        };
        if let Some(m) = file.and_then(|f| std::fs::metadata(f).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
            sum = sum.wrapping_add(m.as_nanos() as u64);
        }
    }
    sum
}

fn dots_under(item: &Item, x: f32, y: f32) -> Option<u32> {
    if !item.rect.contains(x, y) {
        return None;
    }
    item.children.iter().find_map(|c| dots_under(c, x, y)).or_else(|| layout::dot_at(item, x))
}

#[allow(clippy::too_many_arguments)]
fn new_window(conn: &RustConnection, screen: &Screen, argb_visual: Option<u32>, inst: &Instance, res: Resolved, mon: MonRect, data: &Data, fonts: &Fonts) -> Result<Win, String> {
    let d = data_with_options(data, inst);
    let item = lay(&res.def, &d, fonts);
    let (w, h) = (item.rect.w.ceil().max(8.0) as u32, item.rect.h.ceil().max(8.0) as u32);
    let (x, y) = config::place_at(&inst.anchor, inst.x, inst.y, (mon.x, mon.y, mon.w, mon.h), w as i32, h as i32);
    let id = conn.generate_id().map_err(err)?;
    let mask = EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::STRUCTURE_NOTIFY;
    let mut aux = CreateWindowAux::new().override_redirect(1).event_mask(mask).background_pixel(screen.black_pixel);
    match argb_visual {
        Some(visual) => {
            let colormap = conn.generate_id().map_err(err)?;
            conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual).map_err(err)?;
            aux = aux.background_pixel(0).border_pixel(0).colormap(colormap);
            conn.create_window(32, id, screen.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, visual, &aux).map_err(err)?;
        }
        None => {
            conn.create_window(0, id, screen.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(err)?;
        }
    }
    let name = format!("macwidgets: {}", res.def.name);
    conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, name.as_bytes()).map_err(err)?;
    conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_CLASS, AtomEnum::STRING, b"macwidgets\0macwidgets\0").map_err(err)?;
    let gc = conn.generate_id().map_err(err)?;
    conn.create_gc(gc, id, &CreateGCAux::new()).map_err(err)?;
    conn.configure_window(id, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW)).map_err(err)?;
    conn.map_window(id).map_err(err)?;
    conn.configure_window(id, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW)).map_err(err)?;
    let base = res.base.clone();
    Ok(Win { id, gc, inst: inst.clone(), res, w, h, item: Some(item), images: ImageCache::new(&base), argb: argb_visual.is_some() })
}

#[allow(clippy::too_many_arguments)]
fn draw(conn: &RustConnection, w: &mut Win, base: &Data, th: &crate::theme::Theme, fonts: &mut Fonts, symbols: &mut SymbolCache, max_bytes: usize, compositor: bool) -> Result<(), String> {
    let data = data_with_options(base, &w.inst);
    let item = layout::layout(&w.res.def, &Ctx { data: &data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, Some((w.w as f32, w.h as f32)));
    let mut pm = Pixmap::new(w.w, w.h).ok_or("pixmap")?;
    paint::paint(&mut pm, &item, &mut Painter { fonts, symbols, images: &mut w.images, theme: th }, 1.0);
    fonts.trim();
    let depth = if w.argb { 32 } else { 24 };
    if !w.argb {
        // no alpha channel: composite the card onto the black-less shape instead; the SHAPE cut-out hides the corners
        let rects = bx::visible_rects(&pm);
        conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, w.id, 0, 0, &rects).map_err(err)?;
    }
    crate::backend::x11_window::present(conn, w.id, w.gc, &pm, depth, w.argb, max_bytes).map_err(err)?;
    let _ = compositor;
    w.item = Some(item);
    Ok(())
}
