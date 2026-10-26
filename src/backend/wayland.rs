//! Wayland backend: a wlr-layer-shell surface backed by wl_shm buffers.
//!
//! The event loop polls the Wayland socket plus a wake pipe, so the app can use timers
//! (`App::tick`) and worker threads (`Waker`) without busy-waiting.

use super::{poll_readable, Anchor as SpecAnchor, SurfaceSpec, WakePipe};
use crate::app::{App, Button, InputRect, Key};
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::fmt;
use std::os::fd::AsRawFd;
use std::time::Instant;
use tiny_skia::Pixmap;

#[derive(Debug)]
pub enum Error {
    Connect(String),
    Dispatch(String),
    Buffer(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connect(m) => write!(f, "wayland connect: {m}"),
            Error::Dispatch(m) => write!(f, "wayland dispatch: {m}"),
            Error::Buffer(m) => write!(f, "wayland buffer: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// True when the compositor offers wl_shm and wlr-layer-shell.
pub fn supported() -> bool {
    let Ok(conn) = Connection::connect_to_env() else { return false };
    let Ok((globals, queue)) = registry_queue_init::<Probe>(&conn) else { return false };
    let _ = queue;
    globals.contents().with_list(|list| {
        let has = |name: &str| list.iter().any(|g| g.interface == name);
        has("zwlr_layer_shell_v1") && has("wl_shm")
    })
}

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

fn dispatch_err<E: fmt::Display>(e: E) -> Error {
    Error::Dispatch(e.to_string())
}

pub fn run<A: App + 'static>(mut app: A, spec: SurfaceSpec) -> Result<(), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
    let (globals, mut queue) = registry_queue_init(&conn).map_err(|e| Error::Connect(e.to_string()))?;
    let qh = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| Error::Connect(format!("wl_compositor: {e}")))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| Error::Connect(format!("layer shell: {e}")))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| Error::Connect(format!("wl_shm: {e}")))?;

    let (wake, waker) = WakePipe::new().map_err(|e| Error::Connect(format!("wake pipe: {e}")))?;
    app.set_waker(waker);

    let (w, h) = app.size();
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some(spec.namespace), None);
    let (edge, margins) = match spec.anchor {
        SpecAnchor::Bottom => (Anchor::BOTTOM, (0, 0, spec.margin, 0)),
        SpecAnchor::Top => (Anchor::TOP, (spec.margin, 0, 0, 0)),
    };
    layer.set_anchor(if spec.stretch { edge | Anchor::LEFT | Anchor::RIGHT } else { edge });
    layer.set_margin(margins.0, margins.1, margins.2, margins.3);
    layer.set_size(w, h);
    // -1 floats above other panels' reserved space; a positive value keeps windows clear of the bar
    // and stays fixed even when the surface grows to hold a menu.
    layer.set_exclusive_zone(if spec.reserve > 0 { spec.reserve } else { -1 });
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let pool = SlotPool::new((w.max(1) * h.max(1) * 4) as usize, &shm).map_err(|e| Error::Buffer(e.to_string()))?;
    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        pool,
        layer,
        pointer: None,
        keyboard: None,
        app,
        requested: (w, h),
        width: w.max(1),
        height: h.max(1),
        configured: false,
        dirty: false,
        exit: false,
        error: None,
        keyboard_on: false,
        input_rects: Vec::new(),
        pm: Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error::Buffer("pixmap".into()))?,
    };

    let mut next_tick: Option<Instant> = Some(Instant::now());
    loop {
        queue.dispatch_pending(&mut state).map_err(dispatch_err)?;
        state.sync();
        state.maybe_draw();
        if let Some(e) = state.error.take() {
            return Err(e);
        }
        if state.exit {
            return Ok(());
        }
        queue.flush().map_err(dispatch_err)?;

        let Some(guard) = queue.prepare_read() else { continue };
        let timeout = next_tick.map(|t| t.saturating_duration_since(Instant::now()));
        let fds = [guard.connection_fd().as_raw_fd(), wake.fd()];
        let ready = poll_readable(&fds, timeout).map_err(dispatch_err)?;
        if ready[0] {
            guard.read().map_err(dispatch_err)?;
        } else {
            drop(guard);
        }
        if ready[1] {
            wake.drain();
        }
        let due = next_tick.map(|t| Instant::now() >= t).unwrap_or(false);
        if ready[1] || due {
            let tick = state.app.tick();
            state.dirty |= tick.redraw;
            next_tick = tick.next.map(|d| Instant::now() + d);
        }
    }
}

struct State<A: App + 'static> {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    app: A,
    /// Size last sent to the compositor with set_size.
    requested: (u32, u32),
    /// Size the compositor actually gave us.
    width: u32,
    height: u32,
    configured: bool,
    dirty: bool,
    exit: bool,
    error: Option<Error>,
    keyboard_on: bool,
    input_rects: Vec<InputRect>,
    pm: Pixmap,
}

impl<A: App + 'static> State<A> {
    /// Pushes app-driven changes (size, keyboard focus mode, input region) to the compositor.
    fn sync(&mut self) {
        let want = self.app.size();
        if want != self.requested {
            self.requested = want;
            self.layer.set_size(want.0, want.1);
            self.layer.commit();
            // The redraw happens when the compositor answers with a configure.
        }
        let kb = self.app.wants_keyboard();
        if kb != self.keyboard_on {
            self.keyboard_on = kb;
            self.layer.set_keyboard_interactivity(if kb { KeyboardInteractivity::OnDemand } else { KeyboardInteractivity::None });
            self.layer.commit();
        }
        let rects = self.app.input_rects();
        if rects != self.input_rects {
            self.input_rects = rects;
            self.apply_input_region();
            self.dirty = true;
        }
    }

    fn size_matches(&self) -> bool {
        // A stretched surface (width 0) matches whatever width the compositor chose.
        (self.requested.0 == 0 || self.requested.0 == self.width) && self.requested.1 == self.height
    }

    fn maybe_draw(&mut self) {
        if self.dirty && self.configured && self.size_matches() {
            self.dirty = false;
            self.draw();
        }
    }

    fn draw(&mut self) {
        let (w, h) = (self.width, self.height);
        if self.pm.width() != w || self.pm.height() != h {
            match Pixmap::new(w.max(1), h.max(1)) {
                Some(pm) => self.pm = pm,
                None => return,
            }
        }
        self.app.draw(&mut self.pm);

        let stride = w as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(Error::Buffer(e.to_string()));
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
        self.layer.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
        if let Err(e) = buffer.attach_to(self.layer.wl_surface()) {
            self.error = Some(Error::Buffer(e.to_string()));
            return;
        }
        self.layer.commit();
    }

    fn apply_input_region(&self) {
        if self.input_rects.is_empty() {
            self.layer.wl_surface().set_input_region(None);
            return;
        }
        if let Ok(region) = Region::new(&self.compositor) {
            for r in &self.input_rects {
                region.add(r.x, r.y, r.w, r.h);
            }
            self.layer.wl_surface().set_input_region(Some(region.wl_region()));
        }
    }
}

impl<A: App + 'static> CompositorHandler for State<A> {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl<A: App + 'static> OutputHandler for State<A> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl<A: App + 'static> LayerShellHandler for State<A> {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let (nw, nh) = configure.new_size;
        if nw != 0 {
            self.width = nw;
        }
        if nh != 0 {
            self.height = nh;
        }
        self.configured = true;
        self.app.resized(self.width, self.height);
        self.input_rects = self.app.input_rects();
        self.apply_input_region();
        self.dirty = true;
    }
}

impl<A: App + 'static> SeatHandler for State<A> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
            }
            Capability::Keyboard if self.keyboard.is_none() => {
                self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
            }
            _ => {}
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Pointer => {
                if let Some(p) = self.pointer.take() {
                    p.release();
                }
            }
            Capability::Keyboard => {
                if let Some(k) = self.keyboard.take() {
                    k.release();
                }
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

fn map_button(code: u32) -> Button {
    match code {
        0x110 => Button::Left,
        0x112 => Button::Middle,
        _ => Button::Right,
    }
}

impl<A: App + 'static> PointerHandler for State<A> {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            let redraw = match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.app.pointer_motion(x, y),
                PointerEventKind::Leave { .. } => self.app.pointer_leave(),
                PointerEventKind::Press { button, .. } => self.app.pointer_button(x, y, map_button(button)),
                PointerEventKind::Release { button, .. } => self.app.pointer_release(x, y, map_button(button)),
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.discrete != 0 { vertical.discrete as f32 } else { (vertical.absolute / 10.0) as f32 };
                    dy != 0.0 && self.app.scroll(x, y, dy)
                }
            };
            self.dirty |= redraw;
        }
    }
}

impl<A: App + 'static> KeyboardHandler for State<A> {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if event.keysym == Keysym::Escape {
            self.dirty |= self.app.key(Key::Escape);
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: u32) {}
}

impl<A: App + 'static> ShmHandler for State<A> {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(@<A: App + 'static> State<A>);
delegate_output!(@<A: App + 'static> State<A>);
delegate_shm!(@<A: App + 'static> State<A>);
delegate_seat!(@<A: App + 'static> State<A>);
delegate_pointer!(@<A: App + 'static> State<A>);
delegate_keyboard!(@<A: App + 'static> State<A>);
delegate_layer!(@<A: App + 'static> State<A>);
delegate_registry!(@<A: App + 'static> State<A>);

impl<A: App + 'static> ProvidesRegistryState for State<A> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
