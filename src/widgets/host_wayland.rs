//! The Wayland backend of the `decwidgets` daemon: one wlr-layer-shell surface per desktop widget on the bottom layer, so cards sit
//! above the wallpaper and below every application window and panel.
//!
//! Layout, painting, live data and click actions are the ones the X11 host uses ([`super::host`]); only the surfaces and their
//! input differ. The loop polls the Wayland socket plus a wake pipe (data sources, theme changes) and a refresh timer, like the
//! bar backend, so an idle desktop costs nothing.

use super::config::{self, Instance, Place, Resolved, WidgetsConfig};
use super::data::{Data, Live};
use super::dsl::CommandDef;
use super::host::{data_with_options, dots_under, lay, ready_file, theme_stamp, Paths};
use super::layout::{self, Ctx, Item};
use super::paint::{self, FontMetrics, ImageCache, Painter};
use crate::backend::{poll_readable, poll_writable, WakePipe};
use crate::menubar::symbols::SymbolCache;
use crate::text::Fonts;
use smithay_client_toolkit::reexports::client::backend::WaylandError;
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::collections::BTreeSet;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

const BTN_LEFT: u32 = 0x110;

/// One desktop widget: its layer surface, definition and the layout of the last frame (for click hit-testing).
struct Card {
    layer: LayerSurface,
    inst: Instance,
    res: Resolved,
    w: u32,
    h: u32,
    item: Option<Item>,
    images: ImageCache,
    /// The compositor sent its first configure, so a buffer may be attached.
    configured: bool,
    /// Needs a frame right away (just configured), not at the next refresh.
    dirty: bool,
    /// Hash of the pixels last sent, so an unchanged frame is not committed again.
    last_frame: Option<u64>,
}

/// A cheap fingerprint of a frame (FNV-1a over the pixels).
fn frame_hash(pixels: &[u8]) -> u64 {
    pixels.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

struct State {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    pointer: Option<wl_pointer::WlPointer>,
    cards: Vec<Card>,
    /// An output appeared, went away or changed size: cards must be placed again.
    outputs_changed: bool,
    /// The compositor closed a card's surface (its output went away).
    closed: bool,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn would_block(e: &WaylandError) -> bool {
    matches!(e, WaylandError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock)
}

/// Removes the readiness file when the host ends, so `status` never reports a dead host as ready.
struct ReadyGuard;

impl ReadyGuard {
    fn announce() -> ReadyGuard {
        let _ = std::fs::write(ready_file(), std::process::id().to_string());
        ReadyGuard
    }
}

impl Drop for ReadyGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(ready_file());
    }
}

/// Logical size and name of the output a card belongs on: the one named by `monitor`, else the first (the primary).
fn output_for(state: &State, monitor: &str) -> Option<(wl_output::WlOutput, (i32, i32))> {
    let pick = |named: bool| {
        state.output_state.outputs().find_map(|o| {
            let info = state.output_state.info(&o)?;
            if named && info.name.as_deref() != Some(monitor) {
                return None;
            }
            let size = info.logical_size.or_else(|| info.modes.iter().find(|m| m.current).map(|m| m.dimensions))?;
            (size.0 > 0 && size.1 > 0).then_some((o, size))
        })
    };
    let wants_named = !monitor.is_empty() && monitor != "primary";
    (if wants_named { pick(true) } else { None }).or_else(|| pick(false))
}

/// Draws one card: the same layout and painter as on X11, into a wl_shm buffer.
fn draw_card(card: &mut Card, pool: &mut SlotPool, base: &Data, th: &crate::theme::Theme, fonts: &mut Fonts, symbols: &mut SymbolCache) -> Result<(), String> {
    let data = data_with_options(base, &card.inst);
    let item = layout::layout(&card.res.def, &Ctx { data: &data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, Some((card.w as f32, card.h as f32)));
    let mut pm = Pixmap::new(card.w, card.h).ok_or("pixmap")?;
    paint::paint(&mut pm, &item, &mut Painter { fonts, symbols, images: &mut card.images, theme: th, background_opacity: paint::background_opacity(&data) }, 1.0);
    card.item = Some(item);
    let hash = frame_hash(pm.data());
    if card.last_frame == Some(hash) && !card.dirty {
        return Ok(());
    }
    card.last_frame = Some(hash);
    let (w, h) = (card.w as i32, card.h as i32);
    let (buffer, canvas) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888).map_err(err)?;
    // tiny-skia is premultiplied RGBA; wl_shm Argb8888 is little-endian B,G,R,A premultiplied.
    for (dst, src) in canvas.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    let surface = card.layer.wl_surface();
    surface.damage_buffer(0, 0, w, h);
    buffer.attach_to(surface).map_err(err)?;
    card.layer.commit();
    card.dirty = false;
    Ok(())
}

/// Tears the cards down and builds them again from the configuration. Returns the data paths and commands they use.
#[allow(clippy::too_many_arguments)]
fn rebuild(state: &mut State, qh: &QueueHandle<State>, compositor: &CompositorState, layer_shell: &LayerShell, paths: &Paths, data: &Data, fonts: &Fonts) -> (BTreeSet<String>, Vec<CommandDef>) {
    state.cards.clear();
    state.outputs_changed = false;
    state.closed = false;
    let cfg = WidgetsConfig::load(&paths.shell_json);
    let mut used = BTreeSet::new();
    let mut commands = Vec::new();
    for inst in cfg.items.iter().filter(|i| cfg.enabled && i.enabled && i.place == Place::Desktop) {
        let res = match config::resolve(inst, &paths.themes_dir, cfg.allow_exec) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("decwidgets: {}: {e}", inst.use_);
                continue;
            }
        };
        let Some((output, (mw, mh))) = output_for(state, &inst.monitor) else {
            eprintln!("decwidgets: {}: no output to place it on yet", inst.use_);
            continue;
        };
        let item = lay(&res.def, &data_with_options(data, inst), fonts);
        let (w, h) = (item.rect.w.ceil().max(8.0) as u32, item.rect.h.ceil().max(8.0) as u32);
        let (x, y) = config::place_clear(&inst.anchor, inst.x, inst.y, (0, 0, mw, mh), w as i32, h as i32, crate::theme::th().bar_insets());
        used.extend(res.def.paths.iter().cloned());
        commands.extend(res.def.commands.iter().cloned());
        let surface = compositor.create_surface(qh);
        let layer = layer_shell.create_layer_surface(qh, surface, Layer::Bottom, Some("decwidgets"), Some(&output));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin(y, 0, 0, x);
        layer.set_size(w, h);
        // Not reserved space, and not pushed aside by panels: the position above is from the output's edge.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        let base = res.base.clone();
        state.cards.push(Card { layer, inst: inst.clone(), res, w, h, item: Some(item), images: ImageCache::new(&base), configured: false, dirty: false, last_frame: None });
    }
    (used, commands)
}

pub fn run(paths: Paths) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(err)?;
    let (globals, mut queue) = registry_queue_init(&conn).map_err(err)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| format!("layer shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
    let pool = SlotPool::new(64 * 1024, &shm).map_err(err)?;
    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        pointer: None,
        cards: Vec::new(),
        outputs_changed: false,
        closed: false,
    };
    // Learn the outputs (their sizes place the cards) and the seat before the first build.
    queue.roundtrip(&mut state).map_err(err)?;
    queue.roundtrip(&mut state).map_err(err)?;

    let (pipe, waker) = WakePipe::new().map_err(err)?;
    {
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w.wake());
    }
    let mut fonts = Fonts::load().ok_or("no usable font found")?;
    let mut symbols = SymbolCache::default();
    let mut live: Option<Live> = None;
    let mut data = Data::empty();
    let mut stamp = (crate::config::Stamp::of(&paths.shell_json), 0u64);
    let mut next_refresh = Instant::now();
    let mut rebuild_now = true;
    let mut ready: Option<ReadyGuard> = None;

    loop {
        queue.dispatch_pending(&mut state).map_err(err)?;
        if rebuild_now || state.outputs_changed || state.closed {
            rebuild_now = false;
            let (used, commands) = rebuild(&mut state, &qh, &compositor, &layer_shell, &paths, &data, &fonts);
            // A rebuild (new theme, wallpaper or colours) usually leaves the data the cards need unchanged: keep the running
            // collectors and their history. Only a different set of data groups or commands replaces (and so stops) them.
            live = match live.take() {
                _ if state.cards.is_empty() => None,
                Some(l) if l.matches(&used, &commands) => Some(l),
                _ => Some(Live::new(&used, &commands, waker.clone())),
            };
            if state.cards.is_empty() {
                fonts.trim();
                crate::mem::trim();
            }
            stamp.1 = theme_stamp(&paths);
            next_refresh = Instant::now();
        }

        let refresh_due = Instant::now() >= next_refresh;
        if refresh_due {
            if let Some(l) = live.as_mut() {
                l.update(&mut data);
            }
        }
        // Everything on a refresh; only the cards that just got their first configure otherwise.
        if refresh_due || state.cards.iter().any(|c| c.configured && c.dirty) {
            let th = crate::theme::th();
            let State { cards, pool, .. } = &mut state;
            for card in cards.iter_mut().filter(|c| c.configured && (refresh_due || c.dirty)) {
                draw_card(card, pool, &data, &th, &mut fonts, &mut symbols)?;
            }
        }
        if refresh_due {
            let th = crate::theme::th();
            let ms = state.cards.iter().map(|c| c.res.def.refresh_ms).min().unwrap_or(1000);
            let ms = if crate::glass::Material::from_theme(&th).enabled() { ms.min(250) } else { ms };
            next_refresh = Instant::now() + Duration::from_millis(u64::from(ms));
        }
        // Ready once every card has had its first frame (or there are none to show).
        if ready.is_none() && state.cards.iter().all(|c| c.configured && !c.dirty) {
            ready = Some(ReadyGuard::announce());
        }

        match queue.flush() {
            Ok(()) => {}
            Err(e) if would_block(&e) => {
                // The socket buffer is full: wait for the compositor to drain it, then retry.
                poll_writable(conn.backend().poll_fd().as_raw_fd(), Duration::from_millis(100)).map_err(err)?;
                continue;
            }
            Err(e) => return Err(err(e)),
        }
        let Some(guard) = queue.prepare_read() else { continue };
        let timeout = next_refresh.saturating_duration_since(Instant::now()).min(Duration::from_millis(500));
        let fds = [guard.connection_fd().as_raw_fd(), pipe.fd()];
        let readable = poll_readable(&fds, Some(timeout)).map_err(err)?;
        if readable[0] {
            match guard.read() {
                Ok(_) => {}
                Err(e) if would_block(&e) => {}
                Err(e) => return Err(err(e)),
            }
        } else {
            drop(guard);
        }
        let mut redraw = false;
        if readable[1] {
            pipe.drain();
            redraw = crate::theme::take_changed();
        }
        // the config or the selected theme changed on disk
        let now = (crate::config::Stamp::of(&paths.shell_json), theme_stamp(&paths));
        if now != stamp {
            stamp = now;
            rebuild_now = true;
        }
        if redraw {
            next_refresh = Instant::now();
        }
    }
}

/// Which way a wheel notch scrolls: negative `dy` is up (content moves down), as on every Wayland pointer.
fn scrolls_up(dy: f64) -> bool {
    dy < 0.0
}

impl State {
    fn card_of(&mut self, surface: &wl_surface::WlSurface) -> Option<&mut Card> {
        self.cards.iter_mut().find(|c| c.layer.wl_surface() == surface)
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.outputs_changed = true;
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.outputs_changed = true;
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.outputs_changed = true;
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        // The compositor took a card away (usually its output): build them again for what is there now.
        self.closed = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if let Some(card) = self.cards.iter_mut().find(|c| c.layer.wl_surface() == layer.wl_surface()) {
            card.configured = true;
            card.dirty = true;
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            let Some(item) = self.card_of(&event.surface).and_then(|c| c.item.clone()) else { continue };
            match event.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } => {
                    if let Some(hit) = layout::hit(&item, x, y) {
                        if let Some(a) = &hit.on_click {
                            super::actions::run(a);
                        }
                    } else if let Some(n) = dots_under(&item, x, y) {
                        super::actions::run(&super::actions::Action::Workspace(super::actions::Ws::N(n)));
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.discrete != 0 { vertical.discrete as f64 } else { vertical.absolute };
                    if dy == 0.0 {
                        continue;
                    }
                    let up = scrolls_up(dy);
                    if let Some(hit) = layout::hit_scroll(&item, x, y, up) {
                        if let Some(a) = if up { hit.on_scroll_up.as_ref() } else { hit.on_scroll_down.as_ref() } {
                            super::actions::run(a);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_pointer!(State);
delegate_layer!(State);
delegate_registry!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_frame_has_the_same_fingerprint_and_a_changed_one_does_not() {
        let a = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut b = a.clone();
        assert_eq!(frame_hash(&a), frame_hash(&b));
        b[7] = 9;
        assert_ne!(frame_hash(&a), frame_hash(&b));
    }

    #[test]
    fn a_negative_wheel_value_scrolls_up() {
        assert!(scrolls_up(-1.0) && scrolls_up(-12.5));
        assert!(!scrolls_up(1.0) && !scrolls_up(0.0));
    }
}
