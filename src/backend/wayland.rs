//! Wayland backend: a wlr-layer-shell surface backed by wl_shm buffers.

use super::{Anchor as SpecAnchor, SurfaceSpec};
use crate::app::{App, Button};
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
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
use std::fmt;
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
impl smithay_client_toolkit::reexports::client::Dispatch<smithay_client_toolkit::reexports::client::protocol::wl_registry::WlRegistry, smithay_client_toolkit::reexports::client::globals::GlobalListContents> for Probe {
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

pub fn run<A: App + 'static>(app: A, spec: SurfaceSpec) -> Result<(), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
    let (globals, mut queue) = registry_queue_init(&conn).map_err(|e| Error::Connect(e.to_string()))?;
    let qh = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| Error::Connect(format!("wl_compositor: {e}")))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| Error::Connect(format!("layer shell: {e}")))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| Error::Connect(format!("wl_shm: {e}")))?;

    let (w, h) = app.size();
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some(spec.namespace), None);
    let (anchor, margins) = match spec.anchor {
        SpecAnchor::Bottom => (Anchor::BOTTOM, (0, 0, spec.margin, 0)),
        SpecAnchor::Top => (Anchor::TOP, (spec.margin, 0, 0, 0)),
    };
    layer.set_anchor(anchor);
    layer.set_margin(margins.0, margins.1, margins.2, margins.3);
    layer.set_size(w, h);
    // -1 floats above other panels' reserved space instead of stacking against it.
    layer.set_exclusive_zone(if spec.reserve > 0 { spec.reserve } else { -1 });
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let pool = SlotPool::new((w * h * 4) as usize, &shm).map_err(|e| Error::Buffer(e.to_string()))?;
    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        pool,
        layer,
        pointer: None,
        app,
        width: w,
        height: h,
        configured: false,
        dirty: false,
        exit: false,
        error: None,
        pm: Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error::Buffer("pixmap".into()))?,
    };

    loop {
        queue.blocking_dispatch(&mut state).map_err(|e| Error::Dispatch(e.to_string()))?;
        if state.dirty && state.configured {
            state.dirty = false;
            state.draw();
        }
        if let Some(e) = state.error.take() {
            return Err(e);
        }
        if state.exit {
            return Ok(());
        }
    }
}

struct State<A: App> {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    app: A,
    width: u32,
    height: u32,
    configured: bool,
    dirty: bool,
    exit: bool,
    error: Option<Error>,
    pm: Pixmap,
}

impl<A: App + 'static> State<A> {
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
        if let Some(r) = self.app.input_rect() {
            if let Ok(region) = Region::new(&self.compositor) {
                region.add(r.x, r.y, r.w, r.h);
                self.layer.wl_surface().set_input_region(Some(region.wl_region()));
            }
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
        if nw != 0 && nh != 0 {
            self.width = nw;
            self.height = nh;
        }
        if !self.configured {
            self.configured = true;
            self.apply_input_region();
        }
        self.dirty = true;
    }
}

impl<A: App + 'static> SeatHandler for State<A> {
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
                PointerEventKind::Press { button, .. } => {
                    let b = match button {
                        0x110 => Button::Left,
                        0x112 => Button::Middle,
                        _ => Button::Right,
                    };
                    self.app.pointer_button(x, y, b)
                }
                _ => false,
            };
            self.dirty |= redraw;
        }
    }
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
delegate_layer!(@<A: App + 'static> State<A>);
delegate_registry!(@<A: App + 'static> State<A>);

impl<A: App + 'static> ProvidesRegistryState for State<A> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
