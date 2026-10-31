//! The Wayland backend of the `decwidgets` daemon: one wlr-layer-shell surface per desktop widget on the bottom layer, so cards sit
//! above the wallpaper and below every application window and panel.
//!
//! Layout, painting, live data and click actions are the ones the X11 host uses ([`super::host`]); only the surfaces and their
//! input differ. The loop polls the Wayland socket plus a wake pipe (data sources, theme changes) and a refresh timer, like the
//! bar backend, so an idle desktop costs nothing.
//!
//! Cards are drawn at the scale the compositor asks for: `wp_fractional_scale_v1` through a `wp_viewport` when offered,
//! otherwise the integer `wl_surface` buffer scale (the preferred scale, or that of the outputs the card is on). The layout,
//! placement and pointer stay in logical (surface) pixels; every leaf the painter draws (text, shapes, strokes, analog clocks,
//! rings, graphs, icons and clipped images) follows the render scale, so no card falls back to a stretched 1x buffer.

use super::config::{self, Instance, Resolved, WidgetsConfig};
use super::data::{Data, Live};
use super::dsl::CommandDef;
use super::host::{config_stamp, data_with_options, dots_under, lay, theme_stamp, wanted, Announcer, Paths, Tally, MAX_CARDS};
use super::layout::{self, Ctx, Item};
use super::paint::{self, FontMetrics, ImageCache, Painter};
use crate::backend::{poll_readable, poll_writable, WakePipe};
use crate::menubar::symbols::SymbolCache;
use crate::text::Fonts;
use smithay_client_toolkit::reexports::client::backend::WaylandError;
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, QueueHandle,
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
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
};

const BTN_LEFT: u32 = 0x110;

/// One desktop widget: its layer surface, definition and the layout of the last frame (for click hit-testing).
struct Card {
    layer: LayerSurface,
    inst: Instance,
    res: Resolved,
    /// Size in surface (logical) pixels.
    w: u32,
    h: u32,
    item: Option<Item>,
    images: ImageCache,
    /// The compositor sent its first configure, so a buffer may be attached.
    configured: bool,
    /// Needs a frame right away (just configured, or its scale changed), not at the next refresh.
    dirty: bool,
    /// The compositor closed the surface; the card is dropped and rebuilt later (bounded, see [`Retry`]).
    closed: bool,
    /// Hash of the pixels last sent, so an unchanged frame is not committed again.
    last_frame: Option<u64>,
    /// Integer scale from `wl_surface.preferred_buffer_scale` or the outputs the surface is on.
    int_scale: i32,
    /// `wp_fractional_scale_v1.preferred_scale` (120 = 1.0), once the compositor has sent one.
    scale120: Option<u32>,
    /// The buffer scale last set on the surface without a viewport.
    applied_scale: i32,
    fractional: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
}

impl Drop for Card {
    fn drop(&mut self) {
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
    }
}

/// A cheap fingerprint of a frame (FNV-1a over the pixels).
fn frame_hash(pixels: &[u8]) -> u64 {
    pixels.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// How a card's buffer relates to its surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufferScale {
    /// The buffer is `n` times the surface (`wl_surface.set_buffer_scale(n)`, or a viewport).
    Integer(i32),
    /// The buffer is `s / 120` times the surface, mapped back by a viewport.
    Fractional(u32),
}

impl BufferScale {
    /// The scale to draw at. A fractional scale needs a viewport.
    fn pick(int_scale: i32, scale120: Option<u32>, viewport: bool) -> BufferScale {
        match scale120 {
            Some(s) if viewport && s > 0 && s % 120 == 0 => BufferScale::Integer((s / 120).clamp(1, 8) as i32),
            Some(s) if viewport && s > 0 => BufferScale::Fractional(s.clamp(60, 960)),
            _ => BufferScale::Integer(int_scale.clamp(1, 8)),
        }
    }

    fn factor(self) -> f32 {
        match self {
            BufferScale::Integer(n) => n as f32,
            BufferScale::Fractional(s) => s as f32 / 120.0,
        }
    }

    /// Buffer size for a `w` x `h` surface (fractional sizes round half away from zero, as the protocol asks).
    fn buffer_size(self, w: u32, h: u32) -> (u32, u32) {
        match self {
            BufferScale::Integer(n) => (w * n as u32, h * n as u32),
            BufferScale::Fractional(s) => {
                let f = |v: u32| ((u64::from(v) * u64::from(s) + 60) / 120).max(1) as u32;
                (f(w), f(h))
            }
        }
    }
}

/// Paints a laid-out `w` x `h` card into a buffer of its size at `scale`: the logical layout drawn through the render scale.
fn paint_card(item: &Item, scale: BufferScale, w: u32, h: u32, painter: &mut Painter) -> Option<Pixmap> {
    let (bw, bh) = scale.buffer_size(w, h);
    let mut pm = Pixmap::new(bw, bh)?;
    crate::gfx::with_render_scale(scale.factor(), || paint::paint(&mut pm, item, painter, 1.0));
    Some(pm)
}

/// Wheel and touchpad scrolling turned into widget scroll actions: one per wheel notch, and one per [`SCROLL_STEP`] pixels
/// of continuous scrolling, so a touchpad swipe (dozens of small events) is a step or two, not dozens of actions.
#[derive(Debug, Default)]
struct ScrollAccum {
    acc: f64,
}

/// Logical pixels of continuous scrolling per action.
const SCROLL_STEP: f64 = 50.0;

impl ScrollAccum {
    /// One axis event: the action it completes, -1 (up), 1 (down) or 0 (none yet). Negative values scroll up on every
    /// Wayland pointer.
    fn feed(&mut self, discrete: i32, absolute: f64, stop: bool) -> i32 {
        let step = if discrete != 0 {
            // a wheel: one action per event, however many notches the frame merged
            self.acc = 0.0;
            discrete.signum()
        } else if absolute != 0.0 && absolute.is_finite() {
            if self.acc * absolute < 0.0 {
                // turned around: start over in the new direction
                self.acc = 0.0;
            }
            self.acc += absolute;
            if self.acc.abs() >= SCROLL_STEP {
                let s = self.acc.signum() as i32;
                self.acc = 0.0;
                s
            } else {
                0
            }
        } else {
            0
        };
        if stop {
            // fingers lifted: a new gesture starts from nothing
            self.acc = 0.0;
        }
        step
    }

    fn reset(&mut self) {
        self.acc = 0.0;
    }
}

/// Rebuilds after the compositor closed cards: spaced out and bounded, so a compositor that keeps refusing a surface (a limit
/// on layer surfaces, an output going away) is not answered with new surfaces in a tight loop. A configuration or output
/// change starts over.
#[derive(Debug, Default)]
struct Retry {
    attempts: u32,
    at: Option<Instant>,
}

const RETRY_LIMIT: u32 = 5;

impl Retry {
    fn closed(&mut self, now: Instant) {
        if self.attempts < RETRY_LIMIT {
            self.at = Some(now + Duration::from_millis(250u64 << self.attempts.min(6)));
            self.attempts += 1;
        } else {
            self.at = None;
        }
    }

    /// True once, when the scheduled rebuild is due.
    fn take_due(&mut self, now: Instant) -> bool {
        if self.at.is_some_and(|t| now >= t) {
            self.at = None;
            return true;
        }
        false
    }

    fn pending(&self) -> bool {
        self.at.is_some()
    }

    fn reset(&mut self) {
        *self = Retry::default();
    }
}

/// What cards are placed by: each output's name and logical size, in the compositor's order. Other output changes
/// (description, mode refresh, scale) leave the cards, their surfaces and their data alone.
type OutputLayout = Vec<(Option<String>, (i32, i32))>;

fn output_layout(outputs: &OutputState) -> OutputLayout {
    outputs.outputs().filter_map(|o| outputs.info(&o)).map(|info| (info.name.clone(), placement_size(&info).unwrap_or((0, 0)))).collect()
}

fn placement_size(info: &smithay_client_toolkit::output::OutputInfo) -> Option<(i32, i32)> {
    info.logical_size.or_else(|| info.modes.iter().find(|m| m.current).map(|m| m.dimensions)).filter(|s| s.0 > 0 && s.1 > 0)
}

struct State {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    pointer: Option<wl_pointer::WlPointer>,
    cards: Vec<Card>,
    fractional_manager: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    /// An output appeared, went away or changed: compare the layout and place the cards again if it differs.
    outputs_changed: bool,
    /// The compositor closed a card's surface.
    closed: bool,
    scroll: ScrollAccum,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn would_block(e: &WaylandError) -> bool {
    matches!(e, WaylandError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock)
}

/// Logical size and name of the output a card belongs on: the one named by `monitor`, else the first (the primary).
fn output_for(state: &State, monitor: &str) -> Option<(wl_output::WlOutput, (i32, i32), i32)> {
    let pick = |named: bool| {
        state.output_state.outputs().find_map(|o| {
            let info = state.output_state.info(&o)?;
            if named && info.name.as_deref() != Some(monitor) {
                return None;
            }
            let size = placement_size(&info)?;
            Some((o, size, info.scale_factor))
        })
    };
    let wants_named = !monitor.is_empty() && monitor != "primary";
    (if wants_named { pick(true) } else { None }).or_else(|| pick(false))
}

/// Draws one card: the same layout and painter as on X11, at the card's buffer scale, into a wl_shm buffer.
fn draw_card(card: &mut Card, pool: &mut SlotPool, base: &Data, th: &crate::theme::Theme, fonts: &mut Fonts, symbols: &mut SymbolCache) -> Result<(), String> {
    let data = data_with_options(base, &card.inst);
    let item = layout::layout(&card.res.def, &Ctx { data: &data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, Some((card.w as f32, card.h as f32)));
    let scale = BufferScale::pick(card.int_scale, card.scale120, card.viewport.is_some());
    let background_opacity = paint::background_opacity(&data);
    let images = &mut card.images;
    let pm = paint_card(&item, scale, card.w, card.h, &mut Painter { fonts, symbols, images, theme: th, background_opacity }).ok_or("pixmap")?;
    let (bw, bh) = (pm.width(), pm.height());
    card.item = Some(item);
    let hash = frame_hash(pm.data()) ^ ((u64::from(bw) << 32) | u64::from(bh));
    if card.last_frame == Some(hash) && !card.dirty {
        return Ok(());
    }
    card.last_frame = Some(hash);
    let (w, h) = (bw as i32, bh as i32);
    let (buffer, canvas) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888).map_err(err)?;
    // tiny-skia is premultiplied RGBA; wl_shm Argb8888 is little-endian B,G,R,A premultiplied.
    for (dst, src) in canvas.chunks_exact_mut(4).zip(pm.data().chunks_exact(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    let surface = card.layer.wl_surface();
    if card.viewport.is_none() {
        // with a viewport the buffer scale stays 1 and the destination (set once) maps any buffer onto the card
        let n = match scale {
            BufferScale::Integer(n) => n,
            BufferScale::Fractional(_) => 1,
        };
        if n != card.applied_scale {
            surface.set_buffer_scale(n);
            card.applied_scale = n;
        }
    }
    surface.damage_buffer(0, 0, w, h);
    buffer.attach_to(surface).map_err(err)?;
    card.layer.commit();
    card.dirty = false;
    Ok(())
}

/// What a build is for and what it made of the configuration.
struct Built {
    used: BTreeSet<String>,
    commands: Vec<CommandDef>,
    config: u64,
    tally: Tally,
}

/// Tears the cards down and builds them again from the configuration.
#[allow(clippy::too_many_arguments)]
fn rebuild(state: &mut State, qh: &QueueHandle<State>, compositor: &CompositorState, layer_shell: &LayerShell, paths: &Paths, data: &Data, fonts: &Fonts) -> Built {
    state.cards.clear();
    state.closed = false;
    let config = config_stamp(&paths.shell_json);
    let cfg = WidgetsConfig::load(&paths.shell_json);
    let mut built = Built { used: BTreeSet::new(), commands: Vec::new(), config, tally: Tally::default() };
    for inst in wanted(&cfg) {
        built.tally.wanted += 1;
        if state.cards.len() >= MAX_CARDS {
            built.tally.broken += 1;
            continue;
        }
        let res = match config::resolve(inst, &paths.themes_dir, cfg.allow_exec) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("decwidgets: {}: {e}", inst.use_);
                built.tally.broken += 1;
                continue;
            }
        };
        // The data a card reads is collected even while it has nowhere to go, so an output that disappears for a moment
        // (a monitor waking, a mode change) does not reset its history.
        built.used.extend(res.def.paths.iter().cloned());
        built.commands.extend(res.def.commands.iter().cloned());
        let Some((output, (mw, mh), output_scale)) = output_for(state, &inst.monitor) else {
            eprintln!("decwidgets: {}: no output to place it on yet", inst.use_);
            built.tally.unplaced += 1;
            continue;
        };
        let item = lay(&res.def, &data_with_options(data, inst), fonts);
        let (w, h) = (item.rect.w.ceil().max(8.0) as u32, item.rect.h.ceil().max(8.0) as u32);
        let (x, y) = config::place_clear(&inst.anchor, inst.x, inst.y, (0, 0, mw, mh), w as i32, h as i32, crate::theme::th().bar_insets());
        let surface = compositor.create_surface(qh);
        // a fractional scale only takes effect through a viewport; without one the integer buffer scale is used
        let viewport = state.viewporter.as_ref().map(|v| v.get_viewport(&surface, qh, ()));
        if let Some(v) = &viewport {
            v.set_destination(w as i32, h as i32);
        }
        let fractional = match (&state.fractional_manager, &viewport) {
            (Some(m), Some(_)) => Some(m.get_fractional_scale(&surface, qh, surface.clone())),
            _ => None,
        };
        let layer = layer_shell.create_layer_surface(qh, surface, Layer::Bottom, Some("decwidgets"), Some(&output));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin(y, 0, 0, x);
        layer.set_size(w, h);
        // Not reserved space, and not pushed aside by panels: the position above is from the output's edge.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        let base = res.base.clone();
        state.cards.push(Card {
            layer,
            inst: inst.clone(),
            res,
            w,
            h,
            item: Some(item),
            images: ImageCache::new(&base),
            configured: false,
            dirty: false,
            closed: false,
            last_frame: None,
            int_scale: output_scale.max(1),
            scale120: None,
            applied_scale: 1,
            fractional,
            viewport,
        });
    }
    if built.tally.wanted as usize > MAX_CARDS {
        eprintln!("decwidgets: {} desktop widgets are enabled; only the first {MAX_CARDS} are shown", built.tally.wanted);
    }
    built
}

pub(super) fn run(paths: Paths, mut announcer: Announcer) -> Result<(), String> {
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
        fractional_manager: globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok(),
        viewporter: globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok(),
        outputs_changed: false,
        closed: false,
        scroll: ScrollAccum::default(),
    };
    // Learn the outputs (their sizes place the cards) and the seat before the first build.
    queue.roundtrip(&mut state).map_err(err)?;
    queue.roundtrip(&mut state).map_err(err)?;
    state.outputs_changed = false;

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
    // the output layout, configuration and outcome of the last build
    let mut placed_on: OutputLayout = Vec::new();
    let mut built = (0u64, Tally::default());
    let mut closed_since_build = 0u32;
    let mut retry = Retry::default();

    loop {
        queue.dispatch_pending(&mut state).map_err(err)?;
        let now = Instant::now();
        if state.closed {
            announcer.invalidate()?;
            // Drop the surfaces the compositor took away and rebuild later, not now: rebuilding at once answers a compositor
            // that refuses the surface (too many layers, a vanished output) with the same surface in a tight loop.
            state.closed = false;
            let before = state.cards.len();
            state.cards.retain(|c| !c.closed);
            closed_since_build += (before - state.cards.len()) as u32;
            retry.closed(now);
        }
        if state.outputs_changed {
            state.outputs_changed = false;
            if output_layout(&state.output_state) != placed_on {
                rebuild_now = true;
                retry.reset();
            }
        }
        if retry.take_due(now) {
            rebuild_now = true;
        }
        if rebuild_now {
            announcer.invalidate()?;
            rebuild_now = false;
            placed_on = output_layout(&state.output_state);
            closed_since_build = 0;
            let b = rebuild(&mut state, &qh, &compositor, &layer_shell, &paths, &data, &fonts);
            // A rebuild (new theme, wallpaper, colours or outputs) usually leaves the data the cards need unchanged: keep the
            // running collectors and their history. Only a different set of data groups or commands replaces (and so stops)
            // them, and only switching every card off ends them.
            live = match live.take() {
                _ if b.tally.wanted == 0 => None,
                Some(l) if l.matches(&b.used, &b.commands) => Some(l),
                _ => Some(Live::new(&b.used, &b.commands, waker.clone())),
            };
            if state.cards.is_empty() {
                fonts.trim();
                crate::mem::trim();
            }
            built = (b.config, b.tally);
            stamp.1 = theme_stamp(&paths);
            next_refresh = Instant::now();
        }

        let refresh_due = Instant::now() >= next_refresh;
        if refresh_due {
            if let Some(l) = live.as_mut() {
                l.update(&mut data);
            }
        }
        // Everything on a refresh; only the cards that were just configured or rescaled otherwise.
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
        // Settled once every card has its first frame and no rebuild is pending: say what is on screen. Cards with errors,
        // without an output or closed by the compositor make the outcome degraded; only zero enabled cards is idle.
        if !retry.pending() && state.cards.iter().all(|c| c.configured && !c.dirty) {
            let mut tally = built.1;
            tally.shown = state.cards.len() as u32;
            tally.unplaced += closed_since_build;
            announcer.settle(built.0, tally);
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
        let wake_at = retry.at.map_or(next_refresh, |t| t.min(next_refresh));
        let timeout = wake_at.saturating_duration_since(Instant::now()).min(Duration::from_millis(500));
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
            retry.reset();
        }
        if redraw {
            next_refresh = Instant::now();
        }
    }
}

impl State {
    fn card_of(&mut self, surface: &wl_surface::WlSurface) -> Option<&mut Card> {
        self.cards.iter_mut().find(|c| c.layer.wl_surface() == surface)
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        if let Some(card) = self.card_of(surface) {
            if card.int_scale != factor.max(1) {
                card.int_scale = factor.max(1);
                card.dirty = true;
            }
        }
    }
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        // The compositor took a card away (its output went, or it refuses more layers): drop it; the loop rebuilds later.
        if self.card_of(layer.wl_surface()).map(|card| card.closed = true).is_some() {
            self.closed = true;
        }
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if let Some(card) = self.card_of(layer.wl_surface()) {
            card.configured = true;
            card.dirty = true;
        }
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for State {
    fn event(_: &mut Self, _: &WpFractionalScaleManagerV1, _: wp_fractional_scale_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WpFractionalScaleV1, wl_surface::WlSurface> for State {
    fn event(state: &mut Self, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, surface: &wl_surface::WlSurface, _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if let Some(card) = state.card_of(surface) {
                if card.scale120 != Some(scale) {
                    card.scale120 = Some(scale);
                    card.dirty = true;
                }
            }
        }
    }
}

impl Dispatch<WpViewporter, ()> for State {
    fn event(_: &mut Self, _: &WpViewporter, _: wp_viewporter::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WpViewport, ()> for State {
    fn event(_: &mut Self, _: &WpViewport, _: wp_viewport::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
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
            if matches!(event.kind, PointerEventKind::Enter { .. } | PointerEventKind::Leave { .. }) {
                // a scroll that began on another card (or outside) does not carry over
                self.scroll.reset();
                continue;
            }
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
                    let step = self.scroll.feed(vertical.discrete, vertical.absolute, vertical.stop);
                    if step == 0 {
                        continue;
                    }
                    let up = step < 0;
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
    fn a_wheel_notch_is_one_action_in_its_direction() {
        let mut s = ScrollAccum::default();
        assert_eq!(s.feed(-1, -15.0, false), -1, "negative is up");
        assert_eq!(s.feed(1, 15.0, false), 1);
        assert_eq!(s.feed(3, 45.0, false), 1, "a frame that merged notches is still one action");
        assert_eq!(s.feed(0, 0.0, false), 0);
    }

    #[test]
    fn one_touchpad_swipe_is_a_few_actions_not_dozens() {
        let mut s = ScrollAccum::default();
        // a typical short swipe: 40 events of 3 px, then the fingers lift
        let actions: Vec<i32> = (0..40).map(|_| s.feed(0, 3.0, false)).filter(|a| *a != 0).collect();
        assert_eq!(actions, vec![1, 1], "120 px of scrolling is two steps of {SCROLL_STEP}");
        assert_eq!(s.feed(0, 0.0, true), 0);
        // the remainder of a finished gesture does not leak into the next one
        assert_eq!((0..16).map(|_| s.feed(0, 3.0, false)).filter(|a| *a != 0).count(), 0);
        // turning around starts over in the new direction
        let mut s = ScrollAccum::default();
        assert_eq!((0..15).map(|_| s.feed(0, 3.0, false)).sum::<i32>(), 0);
        assert_eq!((0..17).map(|_| s.feed(0, -3.0, false)).collect::<Vec<_>>().iter().filter(|a| **a == -1).count(), 1);
        assert_eq!(ScrollAccum::default().feed(0, f64::NAN, false), 0);
    }

    #[test]
    fn closed_surfaces_are_rebuilt_with_growing_delays_and_then_given_up() {
        let t0 = Instant::now();
        let mut r = Retry::default();
        assert!(!r.pending() && !r.take_due(t0));
        let mut delays = Vec::new();
        for _ in 0..RETRY_LIMIT {
            r.closed(t0);
            let at = r.at.expect("a retry is scheduled");
            delays.push(at - t0);
            assert!(!r.take_due(t0), "never at once: a compositor that refuses the surface is not answered in a tight loop");
            assert!(r.take_due(at) && !r.pending(), "due exactly once");
        }
        assert!(delays.windows(2).all(|d| d[1] > d[0]), "{delays:?}");
        r.closed(t0);
        assert!(!r.pending(), "after {RETRY_LIMIT} attempts it stops until something changes");
        r.reset();
        r.closed(t0);
        assert!(r.pending(), "an output or configuration change starts over");
    }

    #[test]
    fn the_buffer_follows_the_protocol_scale() {
        use BufferScale::*;
        assert_eq!(BufferScale::pick(1, None, false), Integer(1));
        assert_eq!(BufferScale::pick(2, None, false), Integer(2), "integer scale without fractional-scale");
        assert_eq!(BufferScale::pick(2, Some(180), false), Integer(2), "a fractional scale needs a viewport");
        assert_eq!(BufferScale::pick(1, Some(180), true), Fractional(180));
        assert_eq!(BufferScale::pick(1, Some(240), true), Integer(2));
        assert_eq!(Integer(2).buffer_size(100, 41), (200, 82));
        assert_eq!(Fractional(150).buffer_size(100, 41), (125, 51), "1.25 x 41 = 51.25 rounds to 51");
        assert_eq!(Fractional(180).buffer_size(101, 9), (152, 14), "151.5 and 13.5 round away from zero");
        assert!((Fractional(150).factor() - 1.25).abs() < 1e-6);
    }

    #[test]
    fn rings_graphs_and_clocks_are_drawn_at_the_buffer_scale() {
        use super::super::dsl::ColorRef;
        use super::super::layout::{Leaf, Rect};
        use crate::theme::model::Rgb;
        let Some(mut fonts) = Fonts::load() else { return eprintln!("(no fonts here: skipped)") };
        let th = crate::theme::builtin::def("nord").unwrap().resolve("nord", true, &crate::theme::model::Selection::default());
        let red = ColorRef::Fixed(Rgb(255, 0, 0), 1.0);
        let leaf = |x: f32, leaf: Leaf| Item {
            kind: "x",
            rect: Rect { x, y: 0.0, w: 40.0, h: 40.0 },
            leaf,
            background: None,
            radius: 0.0,
            border: None,
            border_width: 0.0,
            opacity: 1.0,
            on_click: None,
            on_scroll_up: None,
            on_scroll_down: None,
            children: Vec::new(),
        };
        let mut card = leaf(0.0, Leaf::None);
        card.rect.w = 120.0;
        card.children.push(leaf(0.0, Leaf::Ring { frac: 1.0, size: 40.0, thickness: 6.0, color: red.clone(), track: red.clone() }));
        card.children.push(leaf(40.0, Leaf::Graph { values: vec![0.5; 60], max: 1.0, color: red.clone(), fill: false }));
        card.children.push(leaf(
            80.0,
            Leaf::AnalogClock {
                hour: 0.25,
                minute: 0.0,
                second: 0.0,
                size: 40.0,
                color: red.clone(),
                track: red,
                second_color: ColorRef::Token("accent".into(), 1.0),
                numerals: false,
                seconds: false,
            },
        ));
        for scale in [BufferScale::Integer(1), BufferScale::Fractional(150), BufferScale::Integer(2)] {
            let k = scale.factor();
            let mut symbols = SymbolCache::default();
            let mut images = ImageCache::default();
            let pm = paint_card(&card, scale, 120, 40, &mut Painter { fonts: &mut fonts, symbols: &mut symbols, images: &mut images, theme: &th, background_opacity: 1.0 }).unwrap();
            assert_eq!((pm.width(), pm.height()), scale.buffer_size(120, 40));
            assert_eq!(crate::gfx::render_scale(), 1.0, "the render scale ends with the card");
            // thin strokes centred on a pixel edge at 1x cover about half of the pixels either side
            let lit = |x: f32, y: f32| pm.pixel((x * k) as u32, (y * k) as u32).unwrap().red() > 100;
            // ring: 40 px across, 6 px thick, centred in its 40 x 40 box; empty in the middle
            assert!(lit(20.0, 3.0) && lit(37.0, 20.0) && lit(20.0, 37.0) && lit(3.0, 20.0), "ring at {k}");
            assert!(!lit(20.0, 20.0), "ring hole at {k}");
            // graph: a flat line at half height spanning its box
            assert!(lit(42.0, 20.0) && lit(78.0, 20.0), "graph line at {k}");
            assert!(!lit(60.0, 8.0) && !lit(60.0, 32.0), "graph only on its line at {k}");
            // clock: hour hand to 3 o'clock, minute hand to 12, nothing towards 9
            assert!(lit(108.0, 20.0) && lit(100.0, 10.0), "clock hands at {k}");
            assert_eq!(pm.pixel((90.0 * k) as u32, (20.0 * k) as u32).unwrap().alpha(), 0, "no hand towards 9 at {k}");
        }
    }
}
