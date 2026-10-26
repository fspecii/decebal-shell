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
    compositor::{CompositorHandler, CompositorState},
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
}

impl<A: WindowApp + 'static> State<A> {
    fn apply(&mut self, resp: Response) {
        self.dirty |= resp.redraw;
        match resp.action {
            Some(Action::Close) => self.exit = true,
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

    fn draw(&mut self) {
        let (w, h) = self.size;
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

    fn on_wake(&mut self) {
        let resp = self.app.tick();
        self.apply(resp);
    }

    fn on_key(&mut self, event: &SctkKey) {
        let raw = event.keysym.raw();
        // "Plain" keysym for shortcuts: the character without Shift applied.
        let plain = keysym_char(raw).map(|c| c.to_ascii_lowercase()).and_then(crate::keysym::char_keysym).unwrap_or(raw);
        if let Some(ev) = key_event(raw, plain, event.utf8.as_deref(), self.mods.shift, self.mods.ctrl, self.mods.alt) {
            let resp = self.app.key(&ev);
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
    let surf = if overlay {
        let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| Error::Connect(format!("layer shell: {e}")))?;
        let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some(app.app_id()), None);
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
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
    state.app.set_waker(waker);

    WaylandSource::new(conn.clone(), queue).insert(loop_handle).map_err(|e| Error::Loop(e.to_string()))?;

    let signal = event_loop.get_signal();
    event_loop
        .run(None, &mut state, |state| {
            if state.dirty && state.configured && !state.exit {
                state.dirty = false;
                state.draw();
            }
            if state.exit {
                signal.stop();
            }
        })
        .map_err(|e| Error::Loop(e.to_string()))?;
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
        self.exit = true;
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
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}

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
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            let resp = match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.app.pointer_motion(x, y),
                PointerEventKind::Leave { .. } => self.app.pointer_leave(),
                PointerEventKind::Press { button, serial, .. } => {
                    self.press_serial = serial;
                    match button_of(button) {
                        Some(b) => self.app.pointer_button(x, y, b, true),
                        None => Response::NONE,
                    }
                }
                PointerEventKind::Release { button, .. } => match button_of(button) {
                    Some(b) => self.app.pointer_button(x, y, b, false),
                    None => Response::NONE,
                },
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.discrete != 0 { vertical.discrete as f32 * WHEEL_STEP } else { vertical.absolute as f32 };
                    if dy == 0.0 {
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

    #[test]
    fn mouse_buttons_map() {
        assert_eq!(button_of(BTN_LEFT), Some(Button::Left));
        assert_eq!(button_of(BTN_RIGHT), Some(Button::Right));
        assert_eq!(button_of(BTN_MIDDLE), Some(Button::Middle));
        assert_eq!(button_of(0x113), None, "side buttons are ignored");
    }
}
