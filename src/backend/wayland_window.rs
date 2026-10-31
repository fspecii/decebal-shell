//! Wayland window backend: an xdg-shell top-level, a full-screen layer-shell overlay, or a compact popup card (a HUD,
//! the dashboard under a bar). wl_shm buffers, keyboard (with key repeat), pointer and wheel input. Runs on a calloop
//! loop so worker threads can wake it through a channel.
//!
//! A compact card is two surfaces. The *host* is a transparent layer surface covering one output: its configure is
//! that output's logical size, it carries the output's scale, holds the keyboard of an interactive card, and takes
//! the clicks beside the card (a click there closes the card, as the X11 pointer grab does). The *card* is a
//! desynchronised subsurface of the host with card-sized buffers: at most [`MAX_CARD_BUFFERS`] of them, only the
//! changed rectangle is copied and damaged, and a new frame waits for the compositor's frame callback. An
//! interactive card also puts a transparent *catcher* on every other output, so a click anywhere else closes it and
//! hands the keyboard back. A passive card (a HUD) has empty input regions and never takes the keyboard.

use crate::app::Button;
use crate::keysym::{key_event, keysym_char};
use crate::window_app::{Action, Edge, MonitorHint, PopupAnchor, PopupScale, Response, Waker, WindowApp};
use smithay_client_toolkit::reexports::calloop::{
    channel::{self, Event as ChannelEvent},
    EventLoop, LoopHandle,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    globals::{registry_queue_init, GlobalList},
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_subsurface, wl_surface},
    Connection, Dispatch, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_subcompositor, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputInfo, OutputState},
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
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
    subcompositor::SubcompositorState,
};
use std::fmt;
use std::sync::Arc;
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

/// Pixels scrolled per wheel notch.
const WHEEL_STEP: f32 = 100.0;
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
/// Card buffers in flight at most (the compositor may hold one or two); past that a frame waits for a release.
const MAX_CARD_BUFFERS: usize = 3;
/// A committed card frame whose callback never comes (a compositor that drops them) stops pacing after this long.
const FRAME_STALL: Duration = Duration::from_secs(1);
/// How long an interactive card waits for the compositor to give it the keyboard before telling the app it did not.
const GRAB_WAIT: Duration = Duration::from_millis(750);
/// All card buffers busy: look again this soon (the compositor releases them as it composites).
const BUSY_RETRY: Duration = Duration::from_millis(8);
/// Smallest gap kept between a card and the far output edge.
const EDGE_GAP: i32 = 8;
/// Layer namespace of the transparent click catchers on the other outputs (kept apart from the app's own rules).
const CATCHER_NAMESPACE: &str = "decebal-popup-outside";

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

/// Which of the globals this backend uses the compositor offers (one registry roundtrip, see [`probe`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Support {
    pub compositor: bool,
    pub shm: bool,
    pub xdg_shell: bool,
    pub layer_shell: bool,
    pub subcompositor: bool,
    /// Optional: lets a compact card use a fractional scale and a 1x1 transparent host buffer.
    pub viewporter: bool,
    pub fractional_scale: bool,
}

impl Support {
    fn from_interfaces<'a>(names: impl IntoIterator<Item = &'a str>) -> Support {
        let mut s = Support::default();
        for n in names {
            match n {
                "wl_compositor" => s.compositor = true,
                "wl_shm" => s.shm = true,
                "xdg_wm_base" => s.xdg_shell = true,
                "zwlr_layer_shell_v1" => s.layer_shell = true,
                "wl_subcompositor" => s.subcompositor = true,
                "wp_viewporter" => s.viewporter = true,
                "wp_fractional_scale_manager_v1" => s.fractional_scale = true,
                _ => {}
            }
        }
        s
    }

    /// The compositor can show `app`: a compact card needs layer-shell and subsurfaces; anything else an xdg
    /// top-level (an overlay falls back to one without layer-shell).
    pub fn runs<A: WindowApp>(&self, app: &A) -> bool {
        self.compositor && self.shm && if app.wants_compact_popup() { self.layer_shell && self.subcompositor } else { self.xdg_shell }
    }
}

/// One registry roundtrip listing the globals this backend cares about; `None` when no compositor answers.
pub fn probe() -> Option<Support> {
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
    let conn = Connection::connect_to_env().ok()?;
    let (globals, _queue) = registry_queue_init::<Probe>(&conn).ok()?;
    Some(globals.contents().with_list(|list| Support::from_interfaces(list.iter().map(|g| g.interface.as_str()))))
}

/// True when the compositor offers wl_shm and xdg-shell.
pub fn supported() -> bool {
    probe().is_some_and(|s| s.compositor && s.shm && s.xdg_shell)
}

/// True when the compositor also offers wlr-layer-shell and subsurfaces (the overlay and compact card modes).
pub fn overlay_supported() -> bool {
    probe().is_some_and(|s| s.compositor && s.shm && s.layer_shell && s.subcompositor)
}

/// Either a normal xdg window or a full-screen layer-shell overlay. A compact card has neither (see [`Compact`]).
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

// ---------------------------------------------------------------------------------------------------------------
// Pure geometry and state of a compact card (unit tested below).

/// The scale a card is presented at: the output's integer buffer scale, or a fractional one in 120ths
/// (wp-fractional-scale-v1, presented through a wp_viewport).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputScale {
    Integer(i32),
    Fractional(u32),
}

impl OutputScale {
    fn factor(self) -> f64 {
        match self {
            OutputScale::Integer(n) => n.max(1) as f64,
            OutputScale::Fractional(v) => v.max(1) as f64 / 120.0,
        }
    }
}

/// The fractional scale when the compositor sent one and a viewport can present it, else the integer one.
fn effective_scale(integer: i32, fractional: Option<u32>, viewporter: bool) -> OutputScale {
    match fractional {
        Some(v) if viewporter && v > 0 => OutputScale::Fractional(v),
        _ => OutputScale::Integer(integer.max(1)),
    }
}

/// App units (device pixels for [`PopupScale::Applied`]) to logical surface pixels, at least 1.
fn to_logical(mode: PopupScale, scale: OutputScale, v: u32) -> u32 {
    let l = match (mode, scale) {
        (PopupScale::Logical, _) => v,
        (PopupScale::Applied, OutputScale::Integer(n)) => v.div_ceil(n.max(1) as u32),
        (PopupScale::Applied, OutputScale::Fractional(f)) => ((v as u64 * 120 + f.max(1) as u64 / 2) / f.max(1) as u64) as u32,
    };
    l.max(1)
}

/// Logical surface pixels to app units: the buffer size the protocol expects (`round(logical * scale)`).
fn to_app(mode: PopupScale, scale: OutputScale, v: u32) -> u32 {
    let a = match (mode, scale) {
        (PopupScale::Logical, _) => v,
        (PopupScale::Applied, OutputScale::Integer(n)) => v.saturating_mul(n.max(1) as u32),
        (PopupScale::Applied, OutputScale::Fractional(f)) => ((v as u64 * f as u64 + 60) / 120) as u32,
    };
    a.max(1)
}

/// An app's anchor in logical pixels (an [`PopupScale::Applied`] app gives device pixels).
fn logical_anchor(a: PopupAnchor, mode: PopupScale, scale: OutputScale) -> PopupAnchor {
    if mode == PopupScale::Logical {
        return a;
    }
    let k = scale.factor();
    let down = |v: i32| (v as f64 / k).round() as i32;
    PopupAnchor { top: down(a.top), center_x: a.center_x.map(down), edge: a.edge }
}

/// How a card of a given logical size is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CardGeom {
    /// Buffer (and drawn pixmap) size in pixels.
    buffer: (u32, u32),
    /// wl_surface.set_buffer_scale.
    buffer_scale: i32,
    /// wp_viewport destination (the logical size) for a fractional scale; `None` leaves the viewport unset.
    viewport: Option<(u32, u32)>,
}

fn card_geometry(mode: PopupScale, scale: OutputScale, logical: (u32, u32)) -> CardGeom {
    let buffer = (to_app(mode, scale, logical.0), to_app(mode, scale, logical.1));
    match (mode, scale) {
        (PopupScale::Logical, _) => CardGeom { buffer: logical, buffer_scale: 1, viewport: None },
        (PopupScale::Applied, OutputScale::Integer(n)) => CardGeom { buffer, buffer_scale: n.max(1), viewport: None },
        (PopupScale::Applied, OutputScale::Fractional(_)) => CardGeom { buffer, buffer_scale: 1, viewport: Some(logical) },
    }
}

/// What decides where a card goes, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    content: (u32, u32),
    anchor: Option<PopupAnchor>,
    bottom: Option<f32>,
    centered: bool,
}

/// The card's rectangle `(x, y, w, h)` on an output `sw` x `sh` logical pixels: the rules the X11 popups use. The
/// height is clamped to the room the placement leaves (below a top bar's offset, above a bottom margin), so the
/// card never runs off the output.
fn place_card(p: &Placement, sw: u32, sh: u32) -> (i32, i32, u32, u32) {
    let (sw_i, sh_i) = (sw as i32, sh as i32);
    let mut cw = p.content.0.min(sw.saturating_sub(40)).max(200).min(sw.max(1));
    let room_h = match (p.anchor, p.bottom) {
        (Some(a), _) if matches!(a.edge, Edge::Top | Edge::Bottom) => sh_i - a.top.max(0) - EDGE_GAP,
        (Some(a), _) => {
            cw = cw.min((sw_i - a.top.max(0) - EDGE_GAP).max(1) as u32);
            sh_i - 2 * EDGE_GAP
        }
        (None, Some(f)) => sh_i - (sh as f32 * f).round() as i32 - EDGE_GAP,
        (None, None) => sh_i,
    };
    let ch = p.content.1.min(room_h.max(1) as u32).max(1);
    let x = match p.anchor {
        Some(a) => a.left(sw_i, cw as i32),
        None => (sw_i - cw as i32) / 2,
    };
    let y = if let Some(a) = p.anchor {
        a.y(sh_i, ch as i32)
    } else if let Some(f) = p.bottom {
        (sh_i - ch as i32 - (sh as f32 * f).round() as i32).max(0)
    } else if p.centered {
        (sh_i - ch as i32) / 2
    } else {
        sh_i / 5
    };
    (x, y, cw, ch)
}

/// A rectangle of changed pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Damage {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Damage {
    fn full(w: u32, h: u32) -> Damage {
        Damage { x: 0, y: 0, w, h }
    }

    fn union(self, o: Damage) -> Damage {
        let (x0, y0) = (self.x.min(o.x), self.y.min(o.y));
        let (x1, y1) = ((self.x + self.w).max(o.x + o.w), (self.y + self.h).max(o.y + o.h));
        Damage { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
}

/// The bounding box of the pixels that differ between two `w` x `h` RGBA frames; `None` when nothing changed.
fn diff_rect(prev: &[u8], cur: &[u8], w: u32, h: u32) -> Option<Damage> {
    let stride = w as usize * 4;
    if prev.len() != cur.len() || cur.len() < stride * h as usize {
        return Some(Damage::full(w, h));
    }
    fn row(buf: &[u8], stride: usize, r: u32) -> &[u8] {
        &buf[r as usize * stride..][..stride]
    }
    let differs = |r: u32| row(prev, stride, r) != row(cur, stride, r);
    let top = (0..h).find(|&r| differs(r))?;
    let bottom = (top..h).rev().find(|&r| differs(r)).unwrap_or(top);
    let (mut x0, mut x1) = (w, 0);
    for r in top..=bottom {
        let (a, b) = (row(prev, stride, r), row(cur, stride, r));
        if a == b {
            continue;
        }
        let pixels = || a.chunks_exact(4).zip(b.chunks_exact(4));
        if let Some(l) = pixels().position(|(p, q)| p != q) {
            x0 = x0.min(l as u32);
        }
        if let Some(rr) = pixels().rposition(|(p, q)| p != q) {
            x1 = x1.max(rr as u32 + 1);
        }
    }
    Some(Damage { x: x0, y: top, w: x1.saturating_sub(x0).max(1), h: bottom - top + 1 })
}

/// Per card buffer, the region its pixels are behind the latest frame (a buffer last written two frames ago must
/// catch up on both frames' changes). A new buffer is behind everywhere.
#[derive(Debug, Default)]
struct SlotDamage {
    pending: Vec<Option<Damage>>,
}

impl SlotDamage {
    fn clear(&mut self) {
        self.pending.clear();
    }

    /// A new buffer of `full` size; returns its index.
    fn add_slot(&mut self, full: Damage) -> usize {
        self.pending.push(Some(full));
        self.pending.len() - 1
    }

    /// A frame changed `d`: every buffer is now behind there too.
    fn frame(&mut self, d: Damage) {
        for p in &mut self.pending {
            *p = Some(p.map_or(d, |q| q.union(d)));
        }
    }

    /// The region to copy into buffer `i` before presenting it; it is then up to date.
    fn take(&mut self, i: usize) -> Option<Damage> {
        self.pending.get_mut(i).and_then(Option::take)
    }
}

/// Frame-callback pacing: after a commit the next frame waits for the callback, or [`FRAME_STALL`] at most.
#[derive(Debug, Default)]
struct FramePacer {
    pending_since: Option<Instant>,
}

impl FramePacer {
    fn can_draw(&self, now: Instant) -> bool {
        self.pending_since.is_none_or(|t| now.saturating_duration_since(t) >= FRAME_STALL)
    }

    fn committed(&mut self, now: Instant) {
        self.pending_since = Some(now);
    }

    fn done(&mut self) {
        self.pending_since = None;
    }

    /// Time until a stalled callback stops holding back a waiting frame.
    fn wait(&self, now: Instant) -> Option<Duration> {
        self.pending_since.map(|t| FRAME_STALL.saturating_sub(now.saturating_duration_since(t)))
    }
}

/// The keyboard grab of an interactive card, reported to [`WindowApp::input_grabbed`] exactly once: `Some(mask)` when
/// the compositor gave the host the keyboard (after its modifiers arrived), `None` when it did not within
/// [`GRAB_WAIT`] of the first card frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grab {
    /// Passive card, or already reported.
    Off,
    Waiting {
        deadline: Option<Instant>,
    },
    Entered {
        deadline: Option<Instant>,
    },
}

impl Grab {
    fn waiting() -> Grab {
        Grab::Waiting { deadline: None }
    }

    /// The card's first frame went out: start the clock.
    fn mapped(&mut self, now: Instant) {
        match self {
            Grab::Waiting { deadline } | Grab::Entered { deadline } if deadline.is_none() => *deadline = Some(now + GRAB_WAIT),
            _ => {}
        }
    }

    fn entered(&mut self) {
        if let Grab::Waiting { deadline } = *self {
            *self = Grab::Entered { deadline };
        }
    }

    /// Modifiers (or a key) arrived with the keyboard focus: report the grab with this X modifier mask.
    fn modifiers(&mut self, mask: u16) -> Option<Option<u16>> {
        match self {
            Grab::Entered { .. } => {
                *self = Grab::Off;
                Some(Some(mask))
            }
            _ => None,
        }
    }

    /// The wait ran out: report what we have.
    fn poll(&mut self, now: Instant, mask: u16) -> Option<Option<u16>> {
        match *self {
            Grab::Waiting { deadline: Some(d) } if now >= d => {
                *self = Grab::Off;
                Some(None)
            }
            Grab::Entered { deadline: Some(d) } if now >= d => {
                *self = Grab::Off;
                Some(Some(mask))
            }
            _ => None,
        }
    }

    fn wait(&self, now: Instant) -> Option<Duration> {
        match *self {
            Grab::Waiting { deadline: Some(d) } | Grab::Entered { deadline: Some(d) } => Some(d.saturating_duration_since(now)),
            _ => None,
        }
    }
}

/// The X modifier mask (`ShiftMask`, `LockMask`, `ControlMask`, `Mod1Mask`, `Mod2Mask`, `Mod4Mask`) of Wayland modifiers.
fn x_mods(m: &Modifiers) -> u16 {
    let mut mask = 0;
    for (on, bit) in [(m.shift, 0x1), (m.caps_lock, 0x2), (m.ctrl, 0x4), (m.alt, 0x8), (m.num_lock, 0x10), (m.logo, 0x40)] {
        if on {
            mask |= bit;
        }
    }
    mask
}

/// An output as the popup monitor hint sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OutputDesc {
    name: Option<String>,
    /// Logical position and size in the compositor's layout.
    rect: Option<(i32, i32, i32, i32)>,
}

impl OutputDesc {
    fn of(info: &OutputInfo) -> OutputDesc {
        OutputDesc { name: info.name.clone(), rect: info.logical_position.zip(info.logical_size).map(|((x, y), (w, h))| (x, y, w, h)) }
    }
}

/// The output a hint names: by output name, else the one whose logical rectangle holds the point. `None` leaves the
/// choice to the compositor (its focused output).
fn pick_output(outputs: &[OutputDesc], hint: &MonitorHint) -> Option<usize> {
    if let Some(name) = hint.output.as_deref() {
        if let Some(i) = outputs.iter().position(|o| o.name.as_deref() == Some(name)) {
            return Some(i);
        }
    }
    let (px, py) = hint.point?;
    outputs.iter().position(|o| o.rect.is_some_and(|(x, y, w, h)| px >= x && py >= y && px < x + w && py < y + h))
}

/// An output's size in logical pixels: xdg-output's logical size, else the current mode turned by the transform and
/// divided by the integer scale. Never the raw physical mode.
fn logical_output_size(logical: Option<(i32, i32)>, mode: Option<(i32, i32)>, transform: wl_output::Transform, scale: i32) -> Option<(u32, u32)> {
    if let Some((w, h)) = logical.filter(|&(w, h)| w > 0 && h > 0) {
        return Some((w as u32, h as u32));
    }
    let (w, h) = mode.filter(|&(w, h)| w > 0 && h > 0)?;
    use wl_output::Transform as T;
    let (w, h) = if matches!(transform, T::_90 | T::_270 | T::Flipped90 | T::Flipped270) { (h, w) } else { (w, h) };
    let s = scale.max(1);
    Some(((w / s).max(1) as u32, (h / s).max(1) as u32))
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Buffer size and scale of a fully transparent surface `w` x `h` logical: 1 x 1 stretched by a viewport, else the
/// smallest buffer an integer buffer scale stretches exactly (`w / g` x `h / g` at scale `g = gcd(w, h)`).
fn transparent_plan(w: u32, h: u32, viewporter: bool) -> (u32, u32, i32) {
    if viewporter {
        return (1, 1, 1);
    }
    let g = gcd(w.max(1), h.max(1)).clamp(1, i32::MAX as u32);
    (w.max(1) / g, h.max(1) / g, g as i32)
}

/// Which card buffer the next frame goes into: a released one, else a new one while fewer than `max` exist, else
/// none (all are with the compositor: wait for a release instead of allocating more).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotChoice {
    Reuse(usize),
    New,
    Busy,
}

fn pick_slot(free: &[bool], max: usize) -> SlotChoice {
    match free.iter().position(|&f| f) {
        Some(i) => SlotChoice::Reuse(i),
        None if free.len() < max => SlotChoice::New,
        None => SlotChoice::Busy,
    }
}

/// The outputs that still need a catcher: every output but the card's, once that is known, for a card that closes
/// on an outside click.
fn missing_catchers<T: PartialEq + Clone>(outputs: &[T], target: Option<&T>, have: &[T], closes_outside: bool) -> Vec<T> {
    match target {
        Some(t) if closes_outside => outputs.iter().filter(|o| *o != t && !have.contains(*o)).cloned().collect(),
        _ => Vec::new(),
    }
}

/// Losing the keyboard closes an interactive card that had it and closes on focus loss.
fn focus_leave_closes(had_focus: bool, passive: bool, closes_on_focus_loss: bool) -> bool {
    had_focus && !passive && closes_on_focus_loss
}

/// A click that does not land on the card closes an interactive popup that closes on focus loss.
fn outside_click_closes(passive: bool, closes_on_focus_loss: bool, on_card: bool) -> bool {
    !on_card && !passive && closes_on_focus_loss
}

/// Converts the rectangle `r` of a tiny-skia (premultiplied RGBA) pixmap `w` wide into wl_shm Argb8888 (little-endian
/// B, G, R, A premultiplied) in `dst`, a buffer of the same width.
fn copy_rect(src: &[u8], dst: &mut [u8], w: u32, r: Damage) {
    let stride = w as usize * 4;
    for row in r.y..r.y + r.h {
        let off = row as usize * stride + r.x as usize * 4;
        let len = r.w as usize * 4;
        let (Some(s), Some(d)) = (src.get(off..off + len), dst.get_mut(off..off + len)) else { break };
        for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Compact card surfaces.

/// A transparent full-output layer surface: the host's backdrop or a catcher. Committed once per size.
#[derive(Default)]
struct Backdrop {
    viewport: Option<WpViewport>,
    buffer: Option<Buffer>,
    size: (u32, u32),
}

impl Backdrop {
    /// Attaches a transparent buffer covering `size` logical pixels (the surface is committed by the caller).
    fn cover<A: WindowApp + 'static>(
        &mut self,
        surface: &wl_surface::WlSurface,
        size: (u32, u32),
        viewporter: Option<&WpViewporter>,
        pool: &mut SlotPool,
        qh: &QueueHandle<State<A>>,
    ) -> Result<(), Error> {
        if size == self.size && self.buffer.is_some() {
            return Ok(());
        }
        let (bw, bh, scale) = transparent_plan(size.0, size.1, viewporter.is_some());
        let (buffer, canvas) = pool.create_buffer(bw as i32, bh as i32, bw as i32 * 4, wl_shm::Format::Argb8888).map_err(|e| Error::Buffer(e.to_string()))?;
        canvas.fill(0);
        if let Some(vp) = viewporter {
            let viewport = self.viewport.get_or_insert_with(|| vp.get_viewport(surface, qh, ()));
            viewport.set_destination(size.0 as i32, size.1 as i32);
        }
        surface.set_buffer_scale(scale);
        buffer.attach_to(surface).map_err(|e| Error::Buffer(e.to_string()))?;
        surface.damage_buffer(0, 0, bw as i32, bh as i32);
        self.buffer = Some(buffer);
        self.size = size;
        Ok(())
    }
}

/// A transparent surface on another output that closes an interactive card when clicked.
struct Catcher {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    backdrop: Backdrop,
}

struct Compact {
    passive: bool,
    /// A click beside the card (or on another output) closes it.
    closes_outside: bool,
    mode: PopupScale,
    host: LayerSurface,
    backdrop: Backdrop,
    /// The output's logical size from the host's configure (0 x 0 until then).
    output_size: (u32, u32),
    /// The output the host is on: the hint's, or the one the compositor put it on (learned on enter).
    target: Option<wl_output::WlOutput>,
    _fractional: Option<WpFractionalScaleV1>,
    integer_scale: i32,
    fractional_scale: Option<u32>,
    /// The scale the app and the geometry were last set up for.
    applied: Option<OutputScale>,
    screen_sent: Option<(u32, u32)>,
    card_surface: wl_surface::WlSurface,
    card_sub: wl_subsurface::WlSubsurface,
    card_viewport: Option<WpViewport>,
    /// Logical position of the card on the host last sent.
    position_sent: Option<(i32, i32)>,
    /// The host has state (the card's position) to commit.
    host_dirty: bool,
    geom: Option<CardGeom>,
    /// Size last given to the app (app units).
    draw_size: (u32, u32),
    pool: SlotPool,
    buffers: Vec<Buffer>,
    damage: SlotDamage,
    /// The last presented frame (tiny-skia RGBA), to find what the next one changed.
    prev: Vec<u8>,
    pacer: FramePacer,
    retry_at: Option<Instant>,
    /// The pointer is over the card (so leaving it can be reported once).
    inside: bool,
    grab: Grab,
    /// The host holds the keyboard focus right now.
    focused: bool,
    catchers: Vec<Catcher>,
}

impl Compact {
    fn scale(&self, viewporter: bool) -> OutputScale {
        effective_scale(self.integer_scale, self.fractional_scale, viewporter)
    }

    fn pointer_factor(&self) -> f32 {
        match (self.mode, self.applied) {
            (PopupScale::Applied, Some(s)) => s.factor() as f32,
            _ => 1.0,
        }
    }
}

struct State<A: WindowApp + 'static> {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    subcompositor: Option<SubcompositorState>,
    layer_shell: Option<LayerShell>,
    viewporter: Option<WpViewporter>,
    fractional_manager: Option<WpFractionalScaleManagerV1>,
    shm: Shm,
    pool: SlotPool,
    /// Small pool for the transparent host and catcher buffers.
    aux_pool: SlotPool,
    qh: QueueHandle<State<A>>,
    surf: Option<Surf>,
    compact: Option<Compact>,
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
    /// The output a top-level window is on (its size is the app's screen).
    entered_output: Option<wl_output::WlOutput>,
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
                if let Some(Surf::Window(w)) = &self.surf {
                    w.set_minimized();
                }
            }
            Some(Action::ToggleMaximize) => {
                if let Some(Surf::Window(w)) = &self.surf {
                    if self.maximized {
                        w.unset_maximized();
                    } else {
                        w.set_maximized();
                    }
                }
            }
            Some(Action::Move) => {
                if let (Some(seat), Some(Surf::Window(w))) = (&self.seat, &self.surf) {
                    w.move_(seat, self.press_serial);
                }
            }
            None => {}
        }
    }

    fn fail(&mut self, e: Error) {
        self.error = Some(e);
        self.exit = true;
    }

    fn draw(&mut self) {
        let Some(surf) = self.surf.as_ref() else { return };
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
            Err(e) => return self.fail(Error::Buffer(e.to_string())),
        };
        // tiny-skia is premultiplied RGBA; wl_shm Argb8888 is little-endian B,G,R,A premultiplied.
        for (dst, src) in canvas.chunks_exact_mut(4).zip(self.pm.data().chunks_exact(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        surf.wl().damage_buffer(0, 0, w as i32, h as i32);
        if let Err(e) = buffer.attach_to(surf.wl()) {
            return self.fail(Error::Buffer(e.to_string()));
        }
        surf.commit();
    }

    fn paint_pending(&mut self) {
        if self.compact.is_some() {
            return self.paint_card();
        }
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

    /// A top-level window's screen: the logical size of the output it is on (before it is on one, any output's).
    fn update_output_size(&mut self, output: &wl_output::WlOutput) {
        if self.compact.is_some() || self.entered_output.as_ref().is_some_and(|o| o != output) {
            return;
        }
        let size = self.output_state.info(output).and_then(|i| logical_output_size(i.logical_size, i.modes.iter().find(|m| m.current).map(|m| m.dimensions), i.transform, i.scale_factor));
        if let Some((w, h)) = size {
            self.app.set_screen(w, h);
        }
    }

    // --- compact card -----------------------------------------------------------------------------------------

    /// Creates the host layer surface (on the hinted output, else where the compositor puts it) and the card
    /// subsurface. Called once the output list is known.
    fn start_compact(&mut self) -> Result<(), Error> {
        let qh = self.qh.clone();
        let (Some(layer_shell), Some(subcompositor)) = (self.layer_shell.as_ref(), self.subcompositor.as_ref()) else {
            return Err(Error::Connect("a popup card needs zwlr_layer_shell_v1 and wl_subcompositor".into()));
        };
        let outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
        let descs: Vec<OutputDesc> = outputs.iter().map(|o| self.output_state.info(o).map(|i| OutputDesc::of(&i)).unwrap_or_default()).collect();
        let target = pick_output(&descs, &self.app.popup_monitor()).map(|i| outputs[i].clone());
        let passive = self.app.popup_passive();
        let closes_outside = !passive && self.app.popup_closes_on_focus_loss();

        let surface = self.compositor.create_surface(&qh);
        let host = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some(self.app.app_id()), target.as_ref());
        host.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        host.set_size(0, 0);
        host.set_exclusive_zone(-1);
        // The keyboard stays with an interactive card while it is open; every click outside it closes it, which ends the grab.
        host.set_keyboard_interactivity(if passive { KeyboardInteractivity::None } else { KeyboardInteractivity::Exclusive });
        let (card_sub, card_surface) = subcompositor.create_subsurface(host.wl_surface().clone(), &qh);
        card_sub.set_desync();
        if passive {
            // Empty input regions let every click through to the windows below (a subsurface has its own region).
            let region = Region::new(&self.compositor).map_err(|e| Error::Connect(format!("wl_region: {e}")))?;
            host.wl_surface().set_input_region(Some(region.wl_region()));
            card_surface.set_input_region(Some(region.wl_region()));
        }
        let fractional = self.fractional_manager.as_ref().map(|m| m.get_fractional_scale(host.wl_surface(), &qh, ()));
        let integer_scale = target.as_ref().and_then(|o| self.output_state.info(o)).map_or(1, |i| i.scale_factor.max(1));
        host.commit();

        let mode = self.app.popup_scale();
        self.app.set_overlay(true);
        self.app.set_popup_origin(0, 0);
        self.app.set_popup(true);
        self.app.set_popup_alpha(true);
        let pool = SlotPool::new(4096, &self.shm).map_err(|e| Error::Buffer(e.to_string()))?;
        self.compact = Some(Compact {
            passive,
            closes_outside,
            mode,
            host,
            backdrop: Backdrop::default(),
            output_size: (0, 0),
            target,
            _fractional: fractional,
            integer_scale,
            fractional_scale: None,
            applied: None,
            screen_sent: None,
            card_surface,
            card_sub,
            card_viewport: None,
            position_sent: None,
            host_dirty: false,
            geom: None,
            draw_size: (0, 0),
            pool,
            buffers: Vec::new(),
            damage: SlotDamage::default(),
            prev: Vec::new(),
            pacer: FramePacer::default(),
            retry_at: None,
            inside: false,
            grab: if passive { Grab::Off } else { Grab::waiting() },
            focused: false,
            catchers: Vec::new(),
        });
        self.ensure_catchers();
        Ok(())
    }

    /// An interactive card whose output is known gets a catcher on every other output.
    fn ensure_catchers(&mut self) {
        let qh = self.qh.clone();
        let Some(c) = self.compact.as_mut() else { return };
        let Some(layer_shell) = self.layer_shell.as_ref() else { return };
        let outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
        let have: Vec<wl_output::WlOutput> = c.catchers.iter().map(|k| k.output.clone()).collect();
        for output in missing_catchers(&outputs, c.target.as_ref(), &have, c.closes_outside) {
            let surface = self.compositor.create_surface(&qh);
            let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some(CATCHER_NAMESPACE), Some(&output));
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            c.catchers.push(Catcher { output, layer, backdrop: Backdrop::default() });
        }
    }

    fn host_configure(&mut self, size: (u32, u32)) {
        let qh = self.qh.clone();
        let Some(c) = self.compact.as_mut() else { return };
        let size = (size.0.max(1), size.1.max(1));
        c.output_size = size;
        if let Err(e) = c.backdrop.cover(c.host.wl_surface(), size, self.viewporter.as_ref(), &mut self.aux_pool, &qh) {
            return self.fail(e);
        }
        c.host.commit();
        self.configured = true;
        self.dirty = true;
    }

    fn catcher_configure(&mut self, layer: &LayerSurface, size: (u32, u32)) {
        let qh = self.qh.clone();
        let Some(c) = self.compact.as_mut() else { return };
        let Some(k) = c.catchers.iter_mut().find(|k| k.layer.wl_surface() == layer.wl_surface()) else { return };
        if let Err(e) = k.backdrop.cover(k.layer.wl_surface(), (size.0.max(1), size.1.max(1)), self.viewporter.as_ref(), &mut self.aux_pool, &qh) {
            return self.fail(e);
        }
        k.layer.commit();
    }

    /// Re-places the card for the current scale, output and content size; tells the app about changed sizes and
    /// drops the buffers when the presentation changed. Returns the card buffer geometry.
    fn layout_card(&mut self) -> Option<CardGeom> {
        let viewporter = self.viewporter.is_some();
        let c = self.compact.as_mut()?;
        let scale = c.scale(viewporter);
        let rescaled = c.applied != Some(scale);
        if rescaled && c.mode == PopupScale::Applied {
            self.app.set_scale(scale.factor() as f32);
        }
        c.applied = Some(scale);
        let mode = c.mode;
        let screen = (to_app(mode, scale, c.output_size.0), to_app(mode, scale, c.output_size.1));
        if c.screen_sent != Some(screen) {
            c.screen_sent = Some(screen);
            self.app.set_screen(screen.0, screen.1);
        }
        let content = self.app.content_size().unwrap_or_else(|| self.app.initial_size());
        let placement = Placement {
            content: (to_logical(mode, scale, content.0), to_logical(mode, scale, content.1)),
            anchor: self.app.popup_anchor().map(|a| logical_anchor(a, mode, scale)),
            bottom: self.app.popup_bottom_margin(),
            centered: self.app.popup_centered(),
        };
        let card = place_card(&placement, c.output_size.0, c.output_size.1);
        let geom = card_geometry(mode, scale, (card.2, card.3));
        if geom.buffer != c.draw_size {
            c.draw_size = geom.buffer;
            self.app.resize(geom.buffer.0, geom.buffer.1);
        }
        if c.geom != Some(geom) {
            c.geom = Some(geom);
            c.buffers.clear();
            c.damage.clear();
            c.prev.clear();
            c.card_surface.set_buffer_scale(geom.buffer_scale);
            match (geom.viewport, self.viewporter.as_ref()) {
                (Some((w, h)), Some(vp)) => c.card_viewport.get_or_insert_with(|| vp.get_viewport(&c.card_surface, &self.qh, ())).set_destination(w as i32, h as i32),
                _ => {
                    if let Some(v) = &c.card_viewport {
                        v.set_destination(-1, -1);
                    }
                }
            }
            // Give the memory of a much larger earlier card back (buffers in flight stay valid on the compositor side).
            let bytes = geom.buffer.0 as usize * geom.buffer.1 as usize * 4;
            if c.pool.len() > bytes.saturating_mul(MAX_CARD_BUFFERS * 2) {
                if let Ok(p) = SlotPool::new(bytes, &self.shm) {
                    c.pool = p;
                }
            }
        }
        if c.position_sent != Some((card.0, card.1)) {
            c.position_sent = Some((card.0, card.1));
            c.card_sub.set_position(card.0, card.1);
            // A subsurface position is the parent's state: the host commits it after the card's new frame.
            c.host_dirty = true;
        }
        Some(geom)
    }

    /// Draws the card when something changed and the compositor is ready for another frame.
    fn paint_card(&mut self) {
        let now = Instant::now();
        self.poll_grab(now);
        if !(self.dirty && self.configured) || self.exit {
            return;
        }
        let Some(c) = self.compact.as_ref() else { return };
        if c.output_size.0 == 0 || !c.pacer.can_draw(now) || c.retry_at.is_some_and(|t| now < t) {
            return; // stays dirty: the frame callback, a buffer release or the stall timeout brings it back
        }
        let Some(geom) = self.layout_card() else { return };
        self.present_card(now, geom);
        if let Some(c) = self.compact.as_mut().filter(|c| c.host_dirty) {
            c.host_dirty = false;
            c.host.commit();
        }
    }

    /// Draws one card frame into a free buffer and commits what changed (nothing when the picture is the same).
    fn present_card(&mut self, now: Instant, geom: CardGeom) {
        let (bw, bh) = geom.buffer;
        let Some(c) = self.compact.as_mut() else { return };

        // A free buffer first, so a frame is only drawn when it can be presented.
        let mut free = [false; MAX_CARD_BUFFERS];
        for (f, b) in free.iter_mut().zip(&c.buffers) {
            *f = b.canvas(&mut c.pool).is_some();
        }
        let idx = match pick_slot(&free[..c.buffers.len().min(MAX_CARD_BUFFERS)], MAX_CARD_BUFFERS) {
            SlotChoice::Reuse(i) => i,
            SlotChoice::New => match c.pool.create_buffer(bw as i32, bh as i32, bw as i32 * 4, wl_shm::Format::Argb8888) {
                Ok((buffer, _)) => {
                    c.buffers.push(buffer);
                    c.damage.add_slot(Damage::full(bw, bh))
                }
                Err(e) => return self.fail(Error::Buffer(e.to_string())),
            },
            SlotChoice::Busy => {
                c.retry_at = Some(now + BUSY_RETRY);
                return;
            }
        };
        c.retry_at = None;
        self.dirty = false;

        if self.pm.width() != bw || self.pm.height() != bh {
            match Pixmap::new(bw.max(1), bh.max(1)) {
                Some(pm) => self.pm = pm,
                None => return,
            }
        }
        self.app.draw(&mut self.pm);
        self.a11y.after_draw(&mut self.app);

        let Some(c) = self.compact.as_mut() else { return };
        let cur = self.pm.data();
        let changed = if c.prev.len() != cur.len() {
            c.prev.clear();
            c.prev.extend_from_slice(cur);
            Some(Damage::full(bw, bh))
        } else {
            let d = diff_rect(&c.prev, cur, bw, bh);
            if let Some(d) = d {
                let stride = bw as usize * 4;
                for row in d.y..d.y + d.h {
                    let off = row as usize * stride + d.x as usize * 4;
                    let len = d.w as usize * 4;
                    c.prev[off..off + len].copy_from_slice(&cur[off..off + len]);
                }
            }
            d
        };
        let Some(d) = changed else { return }; // the same picture as on screen: no commit at all
        c.damage.frame(d);
        let region = c.damage.take(idx).unwrap_or(d);
        let Some(canvas) = c.buffers[idx].canvas(&mut c.pool) else { return };
        copy_rect(cur, canvas, bw, region);
        c.card_surface.damage_buffer(d.x as i32, d.y as i32, d.w as i32, d.h as i32);
        c.card_surface.frame(&self.qh, c.card_surface.clone());
        if let Err(e) = c.buffers[idx].attach_to(&c.card_surface) {
            return self.fail(Error::Buffer(e.to_string()));
        }
        c.card_surface.commit();
        c.pacer.committed(now);
        c.grab.mapped(now);
    }

    /// Reports the keyboard grab once its outcome is known.
    fn report_grab(&mut self, outcome: Option<Option<u16>>) {
        if let Some(mods) = outcome {
            if mods.is_none() {
                crate::log_warn!("wayland: the compositor did not give the popup the keyboard; a click outside closes it");
            }
            let resp = self.app.input_grabbed(mods);
            self.apply(resp);
        }
    }

    fn poll_grab(&mut self, now: Instant) {
        let mask = x_mods(&self.mods);
        let outcome = self.compact.as_mut().and_then(|c| c.grab.poll(now, mask));
        self.report_grab(outcome);
    }

    /// How long the loop may sleep before a card timer (grab wait, stalled frame, busy buffers) needs it.
    fn next_wake(&self, now: Instant) -> Option<Duration> {
        let c = self.compact.as_ref()?;
        let mut t = c.grab.wait(now);
        if self.dirty {
            for w in [c.pacer.wait(now), c.retry_at.map(|r| r.saturating_duration_since(now))].into_iter().flatten() {
                t = Some(t.map_or(w, |t| t.min(w)));
            }
        }
        t
    }

    fn is_host(&self, surface: &wl_surface::WlSurface) -> bool {
        self.compact.as_ref().is_some_and(|c| c.host.wl_surface() == surface)
    }

    fn rescale(&mut self) {
        if let Some(c) = self.compact.as_ref() {
            if c.applied != Some(c.scale(self.viewporter.is_some())) {
                self.dirty = true;
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
    let support = globals.contents().with_list(|l| Support::from_interfaces(l.iter().map(|g| g.interface.as_str())));

    let (w, h) = app.initial_size();
    let overlay = app.overlay() && support.layer_shell;
    // A card anchored somewhere, or a passive indicator: not the full-screen overlays (Spotlight, the switcher, ...), which stay as they were.
    let compact = overlay && app.wants_compact_popup();
    let layer_shell = if overlay { Some(LayerShell::bind(&globals, &qh).map_err(|e| Error::Connect(format!("layer shell: {e}")))?) } else { None };
    let surf = if compact {
        None // the host and card surfaces are made once the outputs are known (see `start_compact`)
    } else if let Some(layer_shell) = &layer_shell {
        let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some(app.app_id()), None);
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();
        Some(Surf::Layer(layer))
    } else {
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|e| Error::Connect(format!("xdg-shell: {e}")))?;
        // We draw our own title bar, so ask for client-side decorations.
        let window = xdg_shell.create_window(compositor.create_surface(&qh), WindowDecorations::RequestClient, &qh);
        window.set_title(app.title());
        window.set_app_id(app.app_id());
        window.set_min_size(Some((480, 360)));
        window.commit();
        Some(Surf::Window(window))
    };
    let mut app = app;
    if !compact {
        app.set_overlay(overlay);
    }

    let subcompositor = if compact { Some(SubcompositorState::bind(compositor.wl_compositor().clone(), &globals, &qh).map_err(|e| Error::Connect(format!("wl_subcompositor: {e}")))?) } else { None };
    let (viewporter, fractional_manager) = if compact { bind_scaling(&globals, &qh) } else { (None, None) };
    let pool = SlotPool::new(if compact { 4096 } else { (w.max(1) * h.max(1) * 4) as usize }, &shm).map_err(|e| Error::Buffer(e.to_string()))?;
    let aux_pool = SlotPool::new(64, &shm).map_err(|e| Error::Buffer(e.to_string()))?;
    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        subcompositor,
        layer_shell,
        viewporter,
        fractional_manager,
        shm,
        pool,
        aux_pool,
        qh: qh.clone(),
        surf,
        compact: None,
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
        entered_output: None,
        a11y: crate::a11y::host::Host::inert(),
    };

    // Learn the outputs (names, logical geometry, scale) before placing a card or the app starts searching.
    queue.roundtrip(&mut state).map_err(|e| Error::Connect(e.to_string()))?;
    queue.roundtrip(&mut state).map_err(|e| Error::Connect(e.to_string()))?;
    if compact {
        state.start_compact()?;
        state.app.resize(w, h);
        if let Some(c) = state.compact.as_mut() {
            c.draw_size = (w, h);
        }
    }

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
    if !state.configured && !compact {
        state.app.resize(w, h);
    }
    state.app.set_waker(waker.clone());
    state.a11y = crate::a11y::host::Host::start(&state.app, waker);

    WaylandSource::new(conn.clone(), queue).insert(loop_handle).map_err(|e| Error::Loop(e.to_string()))?;

    // The roundtrips may already have delivered configure. Calloop dispatches before its
    // callback, so a quiet compositor need not send another event to release that first
    // dirty frame. Paint it now, after app/AT initialization and before blocking dispatch.
    state.paint_pending();
    while !state.exit {
        // Only a compact card has timers (grab wait, frame stall, busy buffers); everything else sleeps until an event.
        let timeout = state.next_wake(Instant::now());
        event_loop.dispatch(timeout, &mut state).map_err(|e| Error::Loop(e.to_string()))?;
        state.paint_pending();
    }
    match state.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Optional globals of a compact card: wp_viewporter and wp-fractional-scale-v1 (only useful together).
fn bind_scaling<A: WindowApp + 'static>(globals: &GlobalList, qh: &QueueHandle<State<A>>) -> (Option<WpViewporter>, Option<WpFractionalScaleManagerV1>) {
    let viewporter = globals.bind::<WpViewporter, _, _>(qh, 1..=1, ()).ok();
    let fractional = viewporter.as_ref().and_then(|_| globals.bind::<WpFractionalScaleManagerV1, _, _>(qh, 1..=1, ()).ok());
    (viewporter, fractional)
}

impl<A: WindowApp + 'static> CompositorHandler for State<A> {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        if self.is_host(surface) {
            if let Some(c) = self.compact.as_mut() {
                c.integer_scale = factor.max(1);
            }
            self.rescale();
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(c) = self.compact.as_mut().filter(|c| c.card_surface == *surface) {
            c.pacer.done();
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, output: &wl_output::WlOutput) {
        if self.is_host(surface) {
            // Without a hint the compositor chose the output: now the catchers know where not to go.
            if let Some(c) = self.compact.as_mut().filter(|c| c.target.is_none()) {
                c.target = Some(output.clone());
                self.ensure_catchers();
            }
            return;
        }
        if self.compact.is_none() && self.surf.as_ref().is_some_and(|s| s.wl() == surface) {
            self.entered_output = Some(output.clone());
            self.update_output_size(output);
        }
    }
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl<A: WindowApp + 'static> OutputHandler for State<A> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if self.compact.is_some() {
            self.ensure_catchers(); // a monitor plugged in while an interactive card is open
        } else {
            self.update_output_size(&output);
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.update_output_size(&output);
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if let Some(c) = self.compact.as_mut() {
            c.catchers.retain(|k| k.output != output);
        }
    }
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(c) = self.compact.as_mut() {
            let before = c.catchers.len();
            c.catchers.retain(|k| k.layer.wl_surface() != layer.wl_surface());
            if c.catchers.len() != before {
                return; // only a catcher's output went away
            }
        }
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let (nw, nh) = configure.new_size;
        if self.compact.is_some() {
            self.app.set_rounded(false);
            // The host's size is its output's logical size; the app only ever sees its card.
            let size = (nw, nh);
            if self.is_host(layer.wl_surface()) {
                self.host_configure(size);
            } else {
                self.catcher_configure(layer, size);
            }
            return;
        }
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
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        self.a11y.set_window(None, Some(true));
        if self.is_host(surface) {
            if let Some(c) = self.compact.as_mut() {
                c.focused = true;
                c.grab.entered();
            }
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        self.a11y.set_window(None, Some(false));
        if !self.is_host(surface) {
            return;
        }
        // Focus moved to another surface (a lock screen, another exclusive client): an interactive card goes away,
        // as an X11 popup does on FocusOut.
        let closes = self.app.popup_closes_on_focus_loss();
        let leaves = self.compact.as_mut().is_some_and(|c| focus_leave_closes(std::mem::replace(&mut c.focused, false), c.passive, closes));
        if leaves {
            self.exit = true;
        }
    }

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: SctkKey) {
        let mask = x_mods(&self.mods);
        let outcome = self.compact.as_mut().and_then(|c| c.grab.modifiers(mask));
        self.report_grab(outcome);
        self.on_key(&event);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: SctkKey) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: u32) {
        self.mods = modifiers;
        let mask = x_mods(&self.mods);
        let outcome = self.compact.as_mut().and_then(|c| c.grab.modifiers(mask));
        self.report_grab(outcome);
    }
}

impl<A: WindowApp + 'static> State<A> {
    fn card_pointer(&mut self, event: &PointerEvent) {
        let Some(c) = self.compact.as_ref() else { return };
        let on_card = event.surface == c.card_surface;
        let ours = on_card || event.surface == *c.host.wl_surface() || c.catchers.iter().any(|k| event.surface == *k.layer.wl_surface());
        if !ours {
            return;
        }
        let k = c.pointer_factor();
        let (x, y) = (event.position.0 as f32 * k, event.position.1 as f32 * k);
        let outside_closes = outside_click_closes(c.passive, c.closes_outside, on_card);
        let resp = match event.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                let was_inside = self.compact.as_mut().is_some_and(|c| std::mem::replace(&mut c.inside, on_card));
                match (on_card, was_inside) {
                    (true, _) => self.app.pointer_motion(x, y),
                    (false, true) => self.app.pointer_leave(),
                    (false, false) => Response::NONE,
                }
            }
            PointerEventKind::Leave { .. } => {
                let was_inside = self.compact.as_mut().is_some_and(|c| on_card && std::mem::replace(&mut c.inside, false));
                if was_inside {
                    self.app.pointer_leave()
                } else {
                    Response::NONE
                }
            }
            PointerEventKind::Press { button, serial, .. } => {
                self.press_serial = serial;
                if on_card {
                    button_of(button).map_or(Response::NONE, |b| self.app.pointer_button(x, y, b, true))
                } else {
                    // A click beside the card, or on another output, closes an interactive card.
                    if outside_closes {
                        self.exit = true;
                    }
                    Response::NONE
                }
            }
            // The implicit grab sends a drag that began on the card back to the card, even outside it.
            PointerEventKind::Release { button, .. } if on_card => button_of(button).map_or(Response::NONE, |b| self.app.pointer_button(x, y, b, false)),
            PointerEventKind::Release { .. } => Response::NONE,
            PointerEventKind::Axis { vertical, .. } => {
                let dy = if vertical.discrete != 0 { vertical.discrete as f32 * WHEEL_STEP } else { vertical.absolute as f32 };
                if dy == 0.0 || !on_card {
                    Response::NONE
                } else {
                    self.app.scroll(x, y, dy)
                }
            }
        };
        self.apply(resp);
    }
}

impl<A: WindowApp + 'static> PointerHandler for State<A> {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if self.compact.is_some() {
                self.card_pointer(event);
                continue;
            }
            if self.surf.as_ref().is_none_or(|s| event.surface != *s.wl()) {
                continue;
            }
            let (x, y) = (event.position.0 as f32, event.position.1 as f32);
            let resp = match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.app.pointer_motion(x, y),
                PointerEventKind::Leave { .. } => self.app.pointer_leave(),
                PointerEventKind::Press { button, serial, .. } => {
                    self.press_serial = serial;
                    button_of(button).map_or(Response::NONE, |b| self.app.pointer_button(x, y, b, true))
                }
                PointerEventKind::Release { button, .. } => button_of(button).map_or(Response::NONE, |b| self.app.pointer_button(x, y, b, false)),
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

impl<A: WindowApp + 'static> Dispatch<WpViewporter, ()> for State<A> {
    fn event(_: &mut Self, _: &WpViewporter, _: wp_viewporter::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl<A: WindowApp + 'static> Dispatch<WpViewport, ()> for State<A> {
    fn event(_: &mut Self, _: &WpViewport, _: wp_viewport::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl<A: WindowApp + 'static> Dispatch<WpFractionalScaleManagerV1, ()> for State<A> {
    fn event(_: &mut Self, _: &WpFractionalScaleManagerV1, _: wp_fractional_scale_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl<A: WindowApp + 'static> Dispatch<WpFractionalScaleV1, ()> for State<A> {
    /// Only the host surface has a fractional-scale object; the card is on the same output.
    fn event(state: &mut Self, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if let Some(c) = state.compact.as_mut() {
                c.fractional_scale = Some(scale).filter(|&s| s > 0);
            }
            state.rescale();
        }
    }
}

delegate_compositor!(@<A: WindowApp + 'static> State<A>);
delegate_subcompositor!(@<A: WindowApp + 'static> State<A>);
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
mod tests;
