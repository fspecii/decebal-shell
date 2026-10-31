//! Wayland top-level window backend: xdg-shell + wl_shm buffers, keyboard (with key repeat),
//! pointer and wheel input. Runs on a calloop loop so worker threads can wake it through a channel.

use crate::app::Button;
use crate::keysym::{key_event, keysym_char};
use crate::window_app::{Action, Response, Waker, WindowApp};
use smithay_client_toolkit::reexports::calloop::{
    channel::{self, Event as ChannelEvent},
    EventLoop, LoopHandle,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent as SctkKey, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::fmt;
use std::sync::Arc;
use tiny_skia::Pixmap;

/// Pixels scrolled per wheel notch.
const WHEEL_STEP: f32 = 100.0;
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

#[derive(Debug)]
pub enum Error {
    Connect(String),
    Loop(String),
    Buffer(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connect(m) => write!(f, "wayland connect: {m}"),
            Error::Loop(m) => write!(f, "wayland loop: {m}"),
            Error::Buffer(m) => write!(f, "wayland buffer: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// True when the compositor offers wl_shm and xdg-shell.
pub fn supported() -> bool {
    probe(&["xdg_wm_base", "wl_shm", "wl_compositor"])
}

/// True when the compositor also offers wlr-layer-shell (needed for the full-screen overlay mode).
pub fn overlay_supported() -> bool {
    probe(&["zwlr_layer_shell_v1", "wl_shm", "wl_compositor"])
}

fn probe(required: &[&str]) -> bool {
    struct Probe;
    impl
        smithay_client_toolkit::reexports::client::Dispatch<
            smithay_client_toolkit::reexports::client::protocol::wl_registry::WlRegistry,
            smithay_client_toolkit::reexports::client::globals::GlobalListContents,
        > for Probe
    {
        fn event(
            _: &mut Self,
            _: &smithay_client_toolkit::reexports::client::protocol::wl_registry::WlRegistry,
            _: smithay_client_toolkit::reexports::client::protocol::wl_registry::Event,
            _: &smithay_client_toolkit::reexports::client::globals::GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    let Ok(conn) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, _queue)) = registry_queue_init::<Probe>(&conn) else {
        return false;
    };
    globals.contents().with_list(|list| required.iter().all(|name| list.iter().any(|g| g.interface == *name)))
}

/// Either a normal xdg window or a full-screen layer-shell overlay.
enum Surf {
    Window(Window),
    Layer(LayerSurface),
}

impl Surf {
    fn wl(&self) -> &wl_surface::WlSurface {
        match self {
            Surf::Window(w) => w.wl_surface(),
            Surf::Layer(l) => l.wl_surface(),
        }
    }

    fn commit(&self) {
        match self {
            Surf::Window(w) => w.commit(),
            Surf::Layer(l) => l.commit(),
        }
    }
}

/// Where the card of a popup goes on an output `sw` x `sh`, and how big it is: the rules the X11 popups use.
fn popup_card<A: WindowApp>(app: &A, sw: u32, sh: u32) -> (i32, i32, u32, u32) {
    let (iw, ih) = app.initial_size();
    let (cw, ch) = app.content_size().unwrap_or((iw, ih));
    let cw = cw.min(sw.saturating_sub(40)).max(200).min(sw.max(1));
    let ch = ch.min(sh.max(1)).max(1);
    let (sw_i, sh_i) = (sw as i32, sh as i32);
    let x = match app.popup_anchor() {
        Some(a) => a.left(sw_i, cw as i32),
        None => (sw_i - cw as i32) / 2,
    };
    let y = if let Some(a) = app.popup_anchor() {
        a.y(sh_i, ch as i32)
    } else if let Some(f) = app.popup_bottom_margin() {
        (sh_i - ch as i32 - (sh as f32 * f).round() as i32).max(0)
    } else if app.popup_centered() {
        (sh_i - ch as i32) / 2
    } else {
        sh_i / 5
    };
    (x, y, cw, ch)
}

/// Pointer position relative to a card at `(x, y, w, h)` (the whole surface when there is none) and whether it is over the card.
fn card_point(card: Option<(i32, i32, u32, u32)>, pos: (f64, f64)) -> (f32, f32, bool) {
    match card {
        Some((cx, cy, cw, ch)) => {
            let (x, y) = (pos.0 as f32 - cx as f32, pos.1 as f32 - cy as f32);
            (x, y, x >= 0.0 && y >= 0.0 && x < cw as f32 && y < ch as f32)
        }
        None => (pos.0 as f32, pos.1 as f32, true),
    }
}

/// A click outside the card closes an interactive popup that closes on focus loss; a passive one never sees clicks.
fn outside_click_closes(passive: bool, closes_on_focus_loss: bool, inside: bool) -> bool {
    !inside && !passive && closes_on_focus_loss
}

/// An anchored card or a passive indicator (a HUD). The layer surface covers the whole output and stays transparent;
/// the app draws only the card, which is composited at `card`. Covering the output is what lets a click outside the
/// card close an interactive popup, the way the X11 backend's pointer grab does.
struct Popup {
    /// No input at all: the surface's input region is empty, so every click goes through.
    passive: bool,
    /// Card position and size inside the surface.
    card: (i32, i32, u32, u32),
    /// The pointer is over the card (so leaving it can be reported once).
    inside: bool,
    /// The keyboard entered the surface at least once, so a later leave means focus moved elsewhere.
    focused: bool,
}

struct State<A: WindowApp + 'static> {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    surf: Surf,
    loop_handle: LoopHandle<'static, State<A>>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    seat: Option<wl_seat::WlSeat>,
    press_serial: u32,
    mods: Modifiers,
    app: A,
    size: (u32, u32),
    maximized: bool,
    configured: bool,
    dirty: bool,
    exit: bool,
    error: Option<Error>,
    pm: Pixmap,
    popup: Option<Popup>,
    /// AT-SPI2 bridge. Wayland clients cannot learn their global position, so bounds stay window-relative.
    a11y: crate::a11y::host::Host,
}

impl<A: WindowApp + 'static> State<A> {
    fn apply(&mut self, resp: Response) {
        self.dirty |= resp.redraw;
        match resp.action {
            Some(Action::Close) => self.exit = true,
            // Targeted navigation still updates the window; Wayland focus requires a compositor activation token.
            Some(Action::Activate) => {}
            Some(Action::Minimize) => {
                if let Surf::Window(w) = &self.surf {
                    w.set_minimized();
                }
            }
            Some(Action::ToggleMaximize) => {
                if let Surf::Window(w) = &self.surf {
                    if self.maximized {
                        w.unset_maximized();
                    } else {
                        w.set_maximized();
                    }
                }
            }
            Some(Action::Move) => {
                if let (Some(seat), Surf::Window(w)) = (&self.seat, &self.surf) {
                    w.move_(seat, self.press_serial);
                }
            }
            None => {}
        }
    }

    /// Re-place the card (its content size may change while open) and tell the app when its size changed.
    fn place_popup(&mut self) {
        let (sw, sh) = self.size;
        let card = popup_card(&self.app, sw, sh);
        let Some(p) = self.popup.as_mut() else { return };
        let resized = (card.2, card.3) != (p.card.2, p.card.3);
        p.card = card;
        if resized {
            self.app.resize(card.2, card.3);
            self.dirty = true;
        }
    }

    /// Pointer position relative to the card and whether it is over it.
    fn card_point(&self, pos: (f64, f64)) -> (f32, f32, bool) {
        card_point(self.popup.as_ref().map(|p| p.card), pos)
    }

    fn draw_popup(&mut self) {
        self.place_popup();
        let Some((cx, cy, cw, ch)) = self.popup.as_ref().map(|p| p.card) else { return };
        let (w, h) = self.size;
        if self.pm.width() != cw || self.pm.height() != ch {
            match Pixmap::new(cw.max(1), ch.max(1)) {
                Some(pm) => self.pm = pm,
                None => return,
            }
        }
        self.app.draw(&mut self.pm);
        self.a11y.after_draw(&mut self.app);

        let stride = w as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(Error::Buffer(e.to_string()));
                self.exit = true;
                return;
            }
        };
        canvas.fill(0);
        // Blit the card (tiny-skia premultiplied RGBA) into the transparent output-sized buffer as wl_shm Argb8888.
        let (x0, y0) = (cx.max(0) as usize, cy.max(0) as usize);
        let cols = (cw as usize).min((w as usize).saturating_sub(x0));
        for row in 0..(ch as usize).min((h as usize).saturating_sub(y0)) {
            let src = &self.pm.data()[row * cw as usize * 4..][..cols * 4];
            let dst = &mut canvas[((y0 + row) * w as usize + x0) * 4..][..cols * 4];
            for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        }
        self.surf.wl().damage_buffer(0, 0, w as i32, h as i32);
        if let Err(e) = buffer.attach_to(self.surf.wl()) {
            self.error = Some(Error::Buffer(e.to_string()));
            self.exit = true;
            return;
        }
        self.surf.commit();
    }

    fn draw(&mut self) {
        if self.popup.is_some() {
            return self.draw_popup();
        }
        let (w, h) = self.size;
        if self.pm.width() != w || self.pm.height() != h {
            match Pixmap::new(w.max(1), h.max(1)) {
                Some(pm) => self.pm = pm,
                None => return,
            }
        }
        self.app.draw(&mut self.pm);
        self.a11y.after_draw(&mut self.app);

        let stride = w as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(Error::Buffer(e.to_string()));
                self.exit = true;
                return;
            }
        };
        // tiny-skia is premultiplied RGBA; wl_shm Argb8888 is little-endian B,G,R,A premultiplied.
        for (dst, src) in canvas.chunks_exact_mut(4).zip(self.pm.data().chunks_exact(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        self.surf.wl().damage_buffer(0, 0, w as i32, h as i32);
        if let Err(e) = buffer.attach_to(self.surf.wl()) {
            self.error = Some(Error::Buffer(e.to_string()));
            self.exit = true;
            return;
        }
        self.surf.commit();
    }

    fn paint_pending(&mut self) {
        if self.dirty && self.configured && !self.exit {
            self.dirty = false;
            self.draw();
        }
    }

    fn on_wake(&mut self) {
        let resp = self.app.tick();
        self.apply(resp);
        let resp = self.a11y.service(&mut self.app);
        self.apply(resp);
    }

    fn on_key(&mut self, event: &SctkKey) {
        let raw = event.keysym.raw();
        // "Plain" keysym for shortcuts: the character without Shift applied.
        let plain = keysym_char(raw).map(|c| c.to_ascii_lowercase()).and_then(crate::keysym::char_keysym).unwrap_or(raw);
        if let Some(ev) = key_event(raw, plain, event.utf8.as_deref(), self.mods.shift, self.mods.ctrl, self.mods.alt) {
            let resp = self.app.key(&ev);
            self.apply(resp);
        } else {
            let resp = self.app.unmapped_key(raw, self.mods.shift, self.mods.ctrl, self.mods.alt);
            self.apply(resp);
        }
    }

    fn update_output_size(&mut self, output: &wl_output::WlOutput) {
        let dims = self.output_state.info(output).and_then(|i| i.modes.iter().find(|m| m.current).map(|m| m.dimensions));
        if let Some((w, h)) = dims {
            if w > 0 && h > 0 {
                self.app.set_screen(w as u32, h as u32);
            }
        }
    }
}

pub fn run<A: WindowApp + 'static>(app: A) -> Result<(), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
    let (globals, mut queue) = registry_queue_init(&conn).map_err(|e| Error::Connect(e.to_string()))?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, State<A>> = EventLoop::try_new().map_err(|e| Error::Loop(e.to_string()))?;
    let loop_handle = event_loop.handle();

    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| Error::Connect(format!("wl_compositor: {e}")))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| Error::Connect(format!("wl_shm: {e}")))?;

    let (w, h) = app.initial_size();
    let surface = compositor.create_surface(&qh);
    let overlay = app.overlay() && globals.contents().with_list(|l| l.iter().any(|g| g.interface == "zwlr_layer_shell_v1"));
    // A card anchored somewhere, or a passive indicator: not the full-screen overlays (Spotlight, the switcher, ...), which stay as they were.
    let compact = overlay && app.wants_compact_popup();
    let passive = compact && app.popup_passive();
    if passive {
        // An empty input region lets every click through to the windows below.
        let region = Region::new(&compositor).map_err(|e| Error::Connect(format!("wl_region: {e}")))?;
        surface.set_input_region(Some(region.wl_region()));
    }
    let surf = if overlay {
        let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| Error::Connect(format!("layer shell: {e}")))?;
        let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some(app.app_id()), None);
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(if passive { KeyboardInteractivity::None } else { KeyboardInteractivity::Exclusive });
        layer.commit();
        Surf::Layer(layer)
    } else {
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|e| Error::Connect(format!("xdg-shell: {e}")))?;
        // We draw our own title bar, so ask for client-side decorations.
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
        window.set_title(app.title());
        window.set_app_id(app.app_id());
        window.set_min_size(Some((480, 360)));
        window.commit();
        Surf::Window(window)
    };
    let mut app = app;
    app.set_overlay(overlay);
    if compact {
        app.set_scale(super::ui_scale());
        app.set_popup_origin(0, 0);
        app.set_popup(true);
        app.set_popup_alpha(true);
    }

    let pool = SlotPool::new((w * h * 4) as usize, &shm).map_err(|e| Error::Buffer(e.to_string()))?;
    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        surf,
        loop_handle: loop_handle.clone(),
        keyboard: None,
        pointer: None,
        seat: None,
        press_serial: 0,
        mods: Modifiers::default(),
        app,
        size: (w, h),
        maximized: false,
        configured: false,
        dirty: false,
        exit: false,
        error: None,
        pm: Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error::Buffer("pixmap".into()))?,
        popup: compact.then_some(Popup { passive, card: (0, 0, w, h), inside: false, focused: false }),
        a11y: crate::a11y::host::Host::inert(),
    };

    // Learn the output size (for "fits my screen") before the app starts searching.
    queue.roundtrip(&mut state).map_err(|e| Error::Connect(e.to_string()))?;
    queue.roundtrip(&mut state).map_err(|e| Error::Connect(e.to_string()))?;

    // Worker threads wake us through this channel.
    let (tx, rx) = channel::channel::<()>();
    loop_handle
        .insert_source(rx, |event, _, state: &mut State<A>| {
            if let ChannelEvent::Msg(()) = event {
                state.on_wake();
            }
        })
        .map_err(|e| Error::Loop(e.to_string()))?;
    let waker: Waker = Arc::new(move || {
        let _ = tx.send(());
    });
    if !state.configured {
        state.app.resize(w, h);
    }
    state.app.set_waker(waker.clone());
    state.a11y = crate::a11y::host::Host::start(&state.app, waker);

    WaylandSource::new(conn.clone(), queue).insert(loop_handle).map_err(|e| Error::Loop(e.to_string()))?;

    // The roundtrips may already have delivered configure. Calloop dispatches before its
    // callback, so a quiet compositor need not send another event to release that first
    // dirty frame. Paint it now, after app/AT initialization and before blocking dispatch.
    state.paint_pending();
    if !state.exit {
        let signal = event_loop.get_signal();
        event_loop
            .run(None, &mut state, |state| {
                state.paint_pending();
                if state.exit {
                    signal.stop();
                }
            })
            .map_err(|e| Error::Loop(e.to_string()))?;
    }
    match state.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

impl<A: WindowApp + 'static> CompositorHandler for State<A> {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, output: &wl_output::WlOutput) {
        self.update_output_size(output);
    }
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl<A: WindowApp + 'static> OutputHandler for State<A> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.update_output_size(&output);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.update_output_size(&output);
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl<A: WindowApp + 'static> WindowHandler for State<A> {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        let response = self.app.close_requested();
        self.apply(response);
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window, configure: WindowConfigure, _: u32) {
        let (nw, nh) = configure.new_size;
        let new = (nw.map(|v| v.get()).unwrap_or(self.size.0), nh.map(|v| v.get()).unwrap_or(self.size.1));
        self.maximized = configure.is_maximized();
        let rounded = !(configure.is_maximized() || configure.is_fullscreen() || configure.is_tiled());
        self.app.set_rounded(rounded);
        if new != self.size || !self.configured {
            self.size = new;
            self.app.resize(new.0, new.1);
        }
        self.configured = true;
        self.dirty = true;
    }
}

impl<A: WindowApp + 'static> LayerShellHandler for State<A> {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let (nw, nh) = configure.new_size;
        let new = (if nw > 0 { nw } else { self.size.0 }, if nh > 0 { nh } else { self.size.1 });
        self.app.set_rounded(false);
        if self.popup.is_some() {
            // The surface is the output; the app only ever sees its card.
            let first = !self.configured;
            self.size = new;
            if first {
                self.app.set_screen(new.0, new.1);
            }
            self.place_popup();
            self.configured = true;
            self.dirty = true;
            if first {
                let resp = self.app.input_grabbed(Some(0));
                self.apply(resp);
            }
            return;
        }
        if new != self.size || !self.configured {
            self.size = new;
            self.app.resize(new.0, new.1);
        }
        self.configured = true;
        self.dirty = true;
    }
}

impl<A: WindowApp + 'static> SeatHandler for State<A> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        self.seat = Some(seat.clone());
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let handle = self.loop_handle.clone();
            let kb = self.seat_state.get_keyboard_with_repeat(qh, &seat, None, handle, Box::new(|state: &mut State<A>, _kbd: &wl_keyboard::WlKeyboard, event: SctkKey| state.on_key(&event)));
            self.keyboard = kb.ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Keyboard => {
                if let Some(k) = self.keyboard.take() {
                    k.release();
                }
            }
            Capability::Pointer => {
                if let Some(p) = self.pointer.take() {
                    p.release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl<A: WindowApp + 'static> KeyboardHandler for State<A> {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        self.a11y.set_window(None, Some(true));
        if let Some(p) = self.popup.as_mut() {
            p.focused = true;
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {
        self.a11y.set_window(None, Some(false));
        // Focus moved to another window: an interactive popup goes away, as an X11 popup does on FocusOut.
        let leaves = self.popup.as_ref().is_some_and(|p| p.focused && !p.passive) && self.app.popup_closes_on_focus_loss();
        if leaves {
            self.exit = true;
        }
    }

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: SctkKey) {
        self.on_key(&event);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: SctkKey) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: u32) {
        self.mods = modifiers;
    }
}

impl<A: WindowApp + 'static> PointerHandler for State<A> {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if event.surface != *self.surf.wl() {
                continue;
            }
            let (x, y, inside) = self.card_point(event.position);
            let outside_closes = self.popup.as_ref().is_some_and(|p| outside_click_closes(p.passive, self.app.popup_closes_on_focus_loss(), inside));
            let resp = match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let was_inside = self.popup.as_ref().map(|p| p.inside);
                    if let Some(p) = self.popup.as_mut() {
                        p.inside = inside;
                    }
                    match (inside, was_inside) {
                        (true, _) => self.app.pointer_motion(x, y),
                        (false, Some(true)) => self.app.pointer_leave(),
                        (false, _) => Response::NONE,
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if let Some(p) = self.popup.as_mut() {
                        p.inside = false;
                    }
                    self.app.pointer_leave()
                }
                PointerEventKind::Press { button, serial, .. } => {
                    self.press_serial = serial;
                    if !inside {
                        // A click outside the card of an interactive popup closes it.
                        if outside_closes {
                            self.exit = true;
                        }
                        Response::NONE
                    } else {
                        match button_of(button) {
                            Some(b) => self.app.pointer_button(x, y, b, true),
                            None => Response::NONE,
                        }
                    }
                }
                PointerEventKind::Release { button, .. } => match button_of(button) {
                    Some(b) => self.app.pointer_button(x, y, b, false),
                    None => Response::NONE,
                },
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.discrete != 0 { vertical.discrete as f32 * WHEEL_STEP } else { vertical.absolute as f32 };
                    if dy == 0.0 || !inside {
                        Response::NONE
                    } else {
                        self.app.scroll(x, y, dy)
                    }
                }
            };
            self.apply(resp);
        }
    }
}

fn button_of(code: u32) -> Option<Button> {
    match code {
        BTN_LEFT => Some(Button::Left),
        BTN_RIGHT => Some(Button::Right),
        BTN_MIDDLE => Some(Button::Middle),
        _ => None,
    }
}

impl<A: WindowApp + 'static> ShmHandler for State<A> {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(@<A: WindowApp + 'static> State<A>);
delegate_output!(@<A: WindowApp + 'static> State<A>);
delegate_shm!(@<A: WindowApp + 'static> State<A>);
delegate_seat!(@<A: WindowApp + 'static> State<A>);
delegate_keyboard!(@<A: WindowApp + 'static> State<A>);
delegate_pointer!(@<A: WindowApp + 'static> State<A>);
delegate_xdg_shell!(@<A: WindowApp + 'static> State<A>);
delegate_xdg_window!(@<A: WindowApp + 'static> State<A>);
delegate_layer!(@<A: WindowApp + 'static> State<A>);
delegate_registry!(@<A: WindowApp + 'static> State<A>);

impl<A: WindowApp + 'static> ProvidesRegistryState for State<A> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Just enough app to place a card.
    struct Stub {
        size: (u32, u32),
        anchor: Option<crate::window_app::PopupAnchor>,
        bottom: Option<f32>,
        passive: bool,
        fullscreen: bool,
    }

    impl WindowApp for Stub {
        fn title(&self) -> String {
            "stub".into()
        }
        fn overlay(&self) -> bool {
            true
        }
        fn popup_anchor(&self) -> Option<crate::window_app::PopupAnchor> {
            self.anchor
        }
        fn popup_bottom_margin(&self) -> Option<f32> {
            self.bottom
        }
        fn popup_passive(&self) -> bool {
            self.passive
        }
        fn popup_fullscreen(&self) -> bool {
            self.fullscreen
        }
        fn initial_size(&self) -> (u32, u32) {
            self.size
        }
        fn set_waker(&mut self, _: Waker) {}
        fn set_screen(&mut self, _: u32, _: u32) {}
        fn resize(&mut self, _: u32, _: u32) {}
        fn set_rounded(&mut self, _: bool) {}
        fn draw(&mut self, _: &mut Pixmap) {}
        fn pointer_motion(&mut self, _: f32, _: f32) -> Response {
            Response::NONE
        }
        fn pointer_leave(&mut self) -> Response {
            Response::NONE
        }
        fn pointer_button(&mut self, _: f32, _: f32, _: Button, _: bool) -> Response {
            Response::NONE
        }
        fn scroll(&mut self, _: f32, _: f32, _: f32) -> Response {
            Response::NONE
        }
        fn key(&mut self, _: &crate::window_app::KeyEvent) -> Response {
            Response::NONE
        }
        fn tick(&mut self) -> Response {
            Response::NONE
        }
    }

    fn stub() -> Stub {
        Stub { size: (400, 300), anchor: None, bottom: None, passive: false, fullscreen: false }
    }

    #[test]
    fn a_card_under_a_bar_hangs_from_the_anchor_and_stays_on_screen() {
        let anchor = crate::window_app::PopupAnchor { top: 30, center_x: Some(1000), edge: crate::window_app::Edge::Top };
        let (x, y, w, h) = popup_card(&Stub { anchor: Some(anchor), ..stub() }, 1920, 1080);
        assert_eq!((x, y, w, h), (800, 30, 400, 300));
        let anchor = crate::window_app::PopupAnchor { top: 30, center_x: Some(1915), edge: crate::window_app::Edge::Top };
        let (x, ..) = popup_card(&Stub { anchor: Some(anchor), ..stub() }, 1920, 1080);
        assert_eq!(x, 1920 - 400 - 8, "kept 8 px from the right edge");
    }

    #[test]
    fn a_hud_sits_centred_above_the_bottom_edge() {
        let (x, y, w, h) = popup_card(&Stub { size: (240, 120), bottom: Some(0.1), passive: true, ..stub() }, 1920, 1080);
        assert_eq!((x, y, w, h), (840, 1080 - 120 - 108, 240, 120));
    }

    #[test]
    fn a_card_larger_than_a_small_output_is_clamped_not_overflowing() {
        let (x, y, w, h) = popup_card(&Stub { size: (4000, 4000), bottom: Some(0.05), ..stub() }, 800, 600);
        assert_eq!((w, h), (760, 600));
        assert!(x >= 0 && y >= 0 && x + w as i32 <= 800 && y + h as i32 <= 600, "{x},{y},{w},{h}");
    }

    #[test]
    fn only_cards_and_huds_are_compact_popups() {
        assert!(!stub().wants_compact_popup(), "a plain overlay (Spotlight, the switcher) keeps the full-screen path");
        assert!(Stub { passive: true, ..stub() }.wants_compact_popup());
        assert!(Stub { bottom: Some(0.1), ..stub() }.wants_compact_popup());
        let anchor = crate::window_app::PopupAnchor { top: 0, center_x: None, edge: crate::window_app::Edge::Top };
        assert!(Stub { anchor: Some(anchor), ..stub() }.wants_compact_popup());
        assert!(!Stub { anchor: Some(anchor), fullscreen: true, ..stub() }.wants_compact_popup(), "a full-screen popup stays full-screen");
    }

    #[test]
    fn the_pointer_is_translated_into_the_card() {
        let card = Some((800, 30, 400, 300));
        assert_eq!(card_point(card, (810.0, 40.0)), (10.0, 10.0, true));
        assert_eq!(card_point(card, (800.0, 30.0)), (0.0, 0.0, true), "the top-left pixel is inside");
        assert!(!card_point(card, (1200.0, 100.0)).2, "the right edge is outside");
        assert!(!card_point(card, (900.0, 330.0)).2, "the bottom edge is outside");
        assert!(!card_point(card, (799.5, 100.0)).2 && !card_point(card, (900.0, 29.5)).2);
        assert!(!card_point(card, (10.0, 10.0)).2, "the rest of the output is outside");
        assert_eq!(card_point(None, (5.0, 6.0)), (5.0, 6.0, true), "no card: the surface is the window");
    }

    #[test]
    fn a_click_outside_closes_only_an_interactive_popup() {
        assert!(outside_click_closes(false, true, false));
        assert!(!outside_click_closes(false, true, true), "a click on the card is the app's");
        assert!(!outside_click_closes(true, true, false), "a passive HUD takes no input");
        assert!(!outside_click_closes(false, false, false), "apps that keep the grab on purpose stay open");
    }

    #[test]
    fn mouse_buttons_map() {
        assert_eq!(button_of(BTN_LEFT), Some(Button::Left));
        assert_eq!(button_of(BTN_RIGHT), Some(Button::Right));
        assert_eq!(button_of(BTN_MIDDLE), Some(Button::Middle));
        assert_eq!(button_of(0x113), None, "side buttons are ignored");
    }
}
