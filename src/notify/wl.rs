//! Wayland front end of decnotify: wlr-layer-shell banners and history panel on one connection and one `poll` loop
//! shared with the D-Bus side (see [`super::Frontend`]).
//!
//! * Every banner is its own `Layer::Top` surface anchored top and right with exclusive zone 0, so the compositor keeps
//!   it clear of bars and panels; the margins carry only the visual gaps (see [`super::geom::InsetPolicy`]). With no
//!   banner on screen there is no surface at all. A surface has an explicit empty input region until its first frame
//!   is painted, then takes input only on the rounded outline of its card.
//! * Banners never take the keyboard (`KeyboardInteractivity::None`); their actions are pointer clicks.
//! * The history panel is a `Layer::Overlay` surface with an exclusive keyboard, and a transparent catcher on every
//!   output below it: a click on a catcher closes the panel, which hands the keyboard back. While it is open the
//!   banner surfaces ignore input (an empty region), so a click on one closes the panel too, as on X11.
//! * Surfaces are laid out in device pixels at the output's scale: a buffer scale for an integer scale, a
//!   wp_viewport for a fractional one. Pointer positions are multiplied back, so hit rectangles match the pixels.
//! * Everything that needs the shared history/config state is queued by the protocol handlers as [`Input`] and handled
//!   in [`Frontend::pump`], which is where the shared [`Stack`] and [`HistWin`] rules run.

use super::geom::{button_of, effective_scale, logical_output_size, logical_rects, margins, pick_output, transparent_plan, wayland_metrics, BarFacts, InsetPolicy, KeyRepeat, Scale, ScrollAccum};
use super::history::{history_dimensions, HistOutcome, HistWin};
use super::model::Notification;
use super::present::{Entry, Pick, Plan, Presenter, Refusal, Releasable, Stats, MAX_TOTAL_BYTES};
use super::view::{opaque_rects, Banner, BannerLayout, Btn, Host, Metrics, Render, Stack, ThemeWake};
use super::{Frontend, Shared, UiEvent, Wake};
use crate::backend::{poll_readable, poll_writable};
use smithay_client_toolkit::reexports::client::backend::WaylandError;
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RepeatInfo},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use std::os::fd::{AsRawFd, RawFd};
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

/// Layer namespace of the banners and the history panel (compositor layer rules match on it).
pub const NAMESPACE: &str = "decebal-notifications";
/// Layer namespace of the transparent click catchers behind the history panel.
pub const CATCHER_NAMESPACE: &str = "decebal-notifications-outside";

/// The stack's handle for a surface.
type SurfId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Banner,
    Panel,
    Catcher,
}

/// What was last told to the compositor about a surface (so only changes are sent).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Sent {
    logical: Option<(u32, u32)>,
    margin: Option<(i32, i32)>,
    scale: Option<Scale>,
}

struct Surf {
    id: SurfId,
    kind: Kind,
    /// `None` while parked: the compositor closed it (its output went away) and no output is there to rebuild it on.
    layer: Option<LayerSurface>,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    /// The output a catcher covers.
    output: Option<wl_output::WlOutput>,
    /// Device pixels the surface is laid out at.
    size: (u32, u32),
    pos: (i32, i32),
    configured: bool,
    /// What the compositor has been told and has committed.
    sent: Sent,
    /// Device size of the buffer last attached (the margins and the input region belong to it).
    committed_size: Option<(u32, u32)>,
    /// The latest painted frame, this surface's bounded buffers and its frame pacing.
    pres: Presenter<Buffer, Pixmap>,
    /// Input outline of the last committed frame, in device pixels.
    shape: Vec<(i32, i32, u32, u32)>,
    region_dirty: bool,
    /// Something waits on a frame callback, a buffer release or a scheduled retry: the loop must wake for it.
    deferred: bool,
    /// A refused allocation was reported (cleared by the next commit).
    refusal_logged: bool,
    /// A catcher's logical size from its configure.
    area: (u32, u32),
    /// The logical size a catcher was last committed for.
    covered: Option<(u32, u32)>,
    /// Size of the transparent buffer a catcher has attached.
    attached: Option<(u32, u32)>,
}

impl Surf {
    /// Destroys the protocol objects in order; the layer surface (and its wl_surface) go with `self`.
    fn release(self) {
        if let Some(v) = &self.viewport {
            v.destroy();
        }
        if let Some(f) = &self.fractional {
            f.destroy();
        }
    }
}

impl Releasable for Buffer {
    fn released(&self) -> bool {
        !self.slot().has_active_buffers()
    }
}

/// Sends the size, margins and scale that differ from what was last committed; true when anything was sent.
/// `margin` is `None` when the margins must stay as they are.
fn send_geometry(layer: &LayerSurface, viewport: Option<&WpViewport>, sent: &mut Sent, logical: (u32, u32), margin: Option<(i32, i32)>, scale: Scale) -> bool {
    let mut changed = false;
    let mut geometry = false;
    if sent.logical != Some(logical) {
        layer.set_size(logical.0, logical.1);
        sent.logical = Some(logical);
        changed = true;
        geometry = true;
    }
    if let Some(margin) = margin {
        if sent.margin != Some(margin) {
            layer.set_margin(margin.0, margin.1, 0, 0);
            sent.margin = Some(margin);
            changed = true;
        }
    }
    if geometry || sent.scale != Some(scale) {
        match scale {
            Scale::Integer(n) => {
                layer.wl_surface().set_buffer_scale(n.max(1));
                if let Some(v) = viewport {
                    v.set_destination(-1, -1);
                }
            }
            Scale::Fractional(_) => {
                layer.wl_surface().set_buffer_scale(1);
                if let Some(v) = viewport {
                    v.set_destination(logical.0 as i32, logical.1 as i32);
                }
            }
        }
        sent.scale = Some(scale);
        changed = true;
    }
    changed
}

/// Shm memory the pool keeps once no buffer is left anywhere (a bigger pool is replaced by a small one).
const POOL_KEEP: usize = 1 << 20;

/// An explicit region: empty with no rectangles.
fn set_region(compositor: &CompositorState, surface: &wl_surface::WlSurface, rects: &[(i32, i32, i32, i32)]) {
    if let Ok(region) = Region::new(compositor) {
        for &(x, y, w, h) in rects {
            region.add(x, y, w, h);
        }
        surface.set_input_region(Some(region.wl_region()));
    }
}

/// tiny-skia is premultiplied RGBA; wl_shm Argb8888 is little-endian B,G,R,A premultiplied.
fn copy_argb(src: &[u8], dst: &mut [u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
}

/// EAGAIN is routine on a busy compositor (full socket buffer, spurious wake-up), not a failure.
fn would_block(e: &WaylandError) -> bool {
    matches!(e, WaylandError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock)
}

fn werr<E: std::fmt::Display>(e: E) -> String {
    format!("wayland: {e}")
}

/// The Wayland side: globals, surfaces and drawing state. The banner stack drives it through [`Host`].
struct WlHost {
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    output_state: OutputState,
    viewporter: Option<WpViewporter>,
    fractional_manager: Option<WpFractionalScaleManagerV1>,
    qh: QueueHandle<State>,
    r: Render,
    policy: InsetPolicy,
    /// Bar extents captured for the current stack layout; never read config during presentation.
    layout_bar: BarFacts,
    /// `DECNOTIFY_OUTPUT`: always show on the output of that name.
    fixed_output: Option<String>,
    /// The output the compositor put the first surface on; later surfaces follow it until none is left.
    target: Option<wl_output::WlOutput>,
    /// The latest wp-fractional-scale preferred scale (120ths).
    fractional: Option<u32>,
    /// The scale and output size the stack was last laid out for.
    scale: Scale,
    logical: (u32, u32),
    surfaces: Vec<Surf>,
    next_id: SurfId,
    /// Banners ignore input (the history panel is open).
    inert: bool,
    catchers_wanted: bool,
    /// Buffers of destroyed or parked surfaces the compositor has not released yet. They still count against the
    /// byte budget and are dropped (destroyed) the moment they are released.
    graveyard: Vec<Entry<Buffer>>,
    /// The totals of surfaces that no longer exist.
    gone: Stats,
    /// `DECNOTIFY_DEBUG_WAYLAND`: where to write the presentation counters.
    debug_path: Option<std::ffi::OsString>,
    debug_dirty: bool,
}

impl WlHost {
    fn outputs(&self) -> Vec<(Option<String>, wl_output::WlOutput)> {
        self.output_state.outputs().map(|o| (self.output_state.info(&o).and_then(|i| i.name), o)).collect()
    }

    /// The output new surfaces are requested on: the one in use, else the named one, else the compositor's choice.
    fn creation_output(&self) -> Option<wl_output::WlOutput> {
        self.target.clone().or_else(|| {
            let want = self.fixed_output.as_deref()?;
            self.outputs().into_iter().find(|(name, _)| name.as_deref() == Some(want)).map(|(_, o)| o)
        })
    }

    /// The output the stack is sized for: the one in use, else the named one, else the first.
    fn area_output(&self) -> Option<wl_output::WlOutput> {
        self.target.clone().or_else(|| pick_output(&self.outputs(), self.fixed_output.as_deref()))
    }

    fn output_logical(&self) -> (u32, u32) {
        use wl_output::Transform as T;
        self.area_output()
            .and_then(|o| self.output_state.info(&o))
            .and_then(|i| {
                let swap = matches!(i.transform, T::_90 | T::_270 | T::Flipped90 | T::Flipped270);
                logical_output_size(i.logical_size, i.modes.iter().find(|m| m.current).map(|m| m.dimensions), swap, i.scale_factor)
            })
            .unwrap_or((1920, 1080))
    }

    fn current_scale(&self) -> Scale {
        let integer = self.area_output().and_then(|o| self.output_state.info(&o)).map_or(1, |i| i.scale_factor.max(1));
        effective_scale(integer, self.fractional, self.viewporter.is_some())
    }

    /// Takes the scale and output size now in effect for layout; true when they changed.
    fn apply_layout(&mut self) -> bool {
        let (scale, logical) = (self.current_scale(), self.output_logical());
        if (scale, logical) == (self.scale, self.logical) {
            return false;
        }
        self.scale = scale;
        self.logical = logical;
        self.r.s = scale.factor();
        true
    }

    fn read_bar_facts() -> BarFacts {
        let th = crate::theme::th();
        let insets = th.bar_insets();
        BarFacts { bar_bottom: crate::theme::bar_bottom(), pill: th.pill_bar().is_some(), right: insets.right, bottom: insets.bottom }
    }

    fn index(&self, id: SurfId) -> Option<usize> {
        self.surfaces.iter().position(|s| s.id == id)
    }

    fn lookup(&self, surface: &wl_surface::WlSurface) -> Option<(SurfId, Kind)> {
        self.surfaces.iter().find(|s| s.layer.as_ref().is_some_and(|l| l.wl_surface() == surface)).map(|s| (s.id, s.kind))
    }

    fn kind_of(&self, id: SurfId) -> Option<Kind> {
        self.index(id).map(|i| self.surfaces[i].kind)
    }

    fn build_layer(&self, kind: Kind, output: Option<&wl_output::WlOutput>) -> LayerSurface {
        let surface = self.compositor.create_surface(&self.qh);
        let (layer_kind, namespace) = match kind {
            Kind::Banner => (Layer::Top, NAMESPACE),
            Kind::Panel => (Layer::Overlay, NAMESPACE),
            Kind::Catcher => (Layer::Top, CATCHER_NAMESPACE),
        };
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, layer_kind, Some(namespace), output);
        match kind {
            Kind::Catcher => {
                layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                layer.set_size(0, 0);
                layer.set_exclusive_zone(-1);
                layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            }
            Kind::Banner | Kind::Panel => {
                layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
                // 0: the compositor keeps the surface clear of other layers' exclusive zones (bars, panels).
                layer.set_exclusive_zone(0);
                // Only the history panel takes the keyboard; banners never do.
                layer.set_keyboard_interactivity(if kind == Kind::Panel { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
            }
        }
        // Explicitly nothing until the first frame is painted (an unset region would mean the whole surface).
        set_region(&self.compositor, layer.wl_surface(), &[]);
        layer
    }

    /// A viewport (to present a fractional scale) and, for the surfaces the stack is on, the preferred-scale object.
    /// Catchers on other outputs must not report their output's scale as the stack's.
    fn scale_objects(&self, surface: &wl_surface::WlSurface, preferred: bool) -> (Option<WpViewport>, Option<WpFractionalScaleV1>) {
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(surface, &self.qh, ()));
        let fractional = self.fractional_manager.as_ref().filter(|_| preferred).map(|m| m.get_fractional_scale(surface, &self.qh, ()));
        (viewport, fractional)
    }

    /// Creates a surface of `size` device pixels at (`x`, `y`) of the stack area and sends its initial commit.
    fn spawn(&mut self, kind: Kind, size: (u32, u32), pos: (i32, i32), output: Option<wl_output::WlOutput>) -> SurfId {
        let id = self.next_id;
        self.next_id += 1;
        let on = if kind == Kind::Catcher { output.clone() } else { self.creation_output() };
        let layer = self.build_layer(kind, on.as_ref());
        let (viewport, fractional) = self.scale_objects(layer.wl_surface(), kind != Kind::Catcher);
        if kind == Kind::Catcher {
            // The initial commit asks for the configure that carries the output's size.
            layer.commit();
        }
        self.surfaces.push(Surf {
            id,
            kind,
            layer: Some(layer),
            viewport,
            fractional,
            output,
            size,
            pos,
            configured: false,
            sent: Sent::default(),
            committed_size: None,
            pres: Presenter::default(),
            shape: Vec::new(),
            region_dirty: false,
            deferred: false,
            refusal_logged: false,
            area: (0, 0),
            covered: None,
            attached: None,
        });
        // the initial commit of a banner or panel: only state, no buffer
        let i = self.surfaces.len() - 1;
        self.present_surface(i, Instant::now());
        id
    }

    /// A surface goes away: its buffers wait in the graveyard until the compositor releases them.
    fn dispose(&mut self, mut s: Surf) {
        self.graveyard.extend(s.pres.buffers.take_all());
        self.gone.merge(&s.pres.stats);
        self.debug_dirty = true;
        s.release();
    }

    fn remove(&mut self, id: SurfId) {
        if let Some(i) = self.index(id) {
            let s = self.surfaces.remove(i);
            self.dispose(s);
        }
    }

    /// Every byte of shm memory in use: live surfaces plus buffers the compositor still holds of gone ones.
    fn total_bytes(&self) -> usize {
        self.surfaces.iter().map(|s| s.pres.buffers.bytes()).sum::<usize>() + self.graveyard.iter().map(Entry::bytes).sum::<usize>()
    }

    /// Commits whatever is ready, once per event-loop iteration: a banner's or the panel's latest frame (when its frame
    /// callback has come and a buffer is free), margin changes, and the input region. Everything painted in between
    /// was only stored, so a burst of input costs one commit.
    fn present(&mut self, now: Instant) {
        for i in 0..self.surfaces.len() {
            self.present_surface(i, now);
        }
        self.graveyard.retain(|e| !e.buf.released());
        self.shrink_pool();
        if self.debug_dirty {
            self.write_debug();
        }
    }

    /// The nearest moment a deferred surface has to be looked at again (a lost frame callback's fallback or a busy
    /// retry); `None` when the loop may sleep until an event arrives. Never zero, so it cannot spin.
    fn next_wake(&self, now: Instant) -> Option<Duration> {
        self.surfaces.iter().filter(|s| s.deferred || s.pres.frame.is_some()).filter_map(|s| s.pres.pacer.wake_in(now)).min().map(|d| d.max(Duration::from_millis(1)))
    }

    fn callback_done(&mut self, surface: &wl_surface::WlSurface) {
        if let Some(s) = self.surfaces.iter_mut().find(|s| s.layer.as_ref().is_some_and(|l| l.wl_surface() == surface)) {
            s.pres.callback_done();
            self.debug_dirty = true;
        }
    }

    /// With no buffer left anywhere the pool, which only ever grows, is replaced by a small one.
    fn shrink_pool(&mut self) {
        if self.pool.len() > POOL_KEEP && self.graveyard.is_empty() && self.surfaces.iter().all(|s| s.pres.buffers.is_empty()) {
            if let Ok(pool) = SlotPool::new(64 * 1024, &self.shm) {
                self.pool = pool;
            }
        }
    }

    fn write_debug(&mut self) {
        self.debug_dirty = false;
        let Some(path) = &self.debug_path else { return };
        let mut total = self.gone;
        let surfaces: Vec<_> = self
            .surfaces
            .iter()
            .map(|s| {
                total.merge(&s.pres.stats);
                serde_json::json!({
                    "id": s.id, "kind": format!("{:?}", s.kind), "configured": s.configured, "buffers": s.pres.buffers.len(), "bytes": s.pres.buffers.bytes(),
                    "callbacks_outstanding": s.pres.pacer.outstanding(), "frame_pending": s.pres.frame.is_some(), "stats": s.pres.stats.json(),
                })
            })
            .collect();
        let data = serde_json::json!({
            "scale": self.scale.factor(), "surfaces": surfaces, "graveyard": self.graveyard.len(), "total_bytes": self.total_bytes(), "pool_bytes": self.pool.len(),
            "limits": {"buffers": super::present::MAX_BUFFERS, "callbacks": super::present::MAX_CALLBACKS, "surface_bytes": super::present::MAX_SURFACE_BYTES, "total_bytes": MAX_TOTAL_BYTES},
            "totals": total.json(),
        });
        let _ = std::fs::write(path, data.to_string());
    }

    /// One surface's turn. Before its first configure only state goes out. After it, at most one commit per call:
    /// the waiting frame (with its size, scale, margins and input region together, in a released buffer, with a frame
    /// callback), else a margin or input-region change that does not depend on a new frame.
    fn present_surface(&mut self, i: usize, now: Instant) {
        if self.surfaces[i].kind == Kind::Catcher {
            return self.present_catcher(i, now);
        }
        let want_scale = self.scale;
        let m = self.layout_metrics();
        let inert = self.inert && self.surfaces[i].kind == Kind::Banner;
        let budget_left = MAX_TOTAL_BYTES.saturating_sub(self.total_bytes());
        let s = &mut self.surfaces[i];
        s.deferred = false;
        let Some(layer) = s.layer.as_ref() else { return };
        let want_logical = (want_scale.logical(s.size.0), want_scale.logical(s.size.1));
        if !s.configured {
            let margin = margins(want_scale, &m, s.pos.0, s.pos.1, s.size);
            if send_geometry(layer, s.viewport.as_ref(), &mut s.sent, want_logical, Some(margin), want_scale) {
                layer.commit();
            }
            return;
        }
        // buffers of an older size or scale go as soon as the compositor releases them, even with nothing to draw
        s.pres.buffers.reap(Some(s.size));
        let plan = s.pres.plan(now, s.size, budget_left, |pm| (pm.width(), pm.height()));
        match plan {
            Plan::Reuse(_) | Plan::New => {}
            Plan::Refused(why) => {
                if !s.refusal_logged {
                    s.refusal_logged = true;
                    let reason = if why == Refusal::Oversize { "larger than one buffer may be" } else { "past the shm memory budget" };
                    crate::log_warn!("decnotify: a {}x{} frame is {reason}; the surface keeps its last frame", s.size.0, s.size.1);
                }
                self.debug_dirty = true;
            }
            Plan::Deferred | Plan::Nothing | Plan::Stale => {
                // No new frame goes out now. Margin changes need no repaint when size and scale are what the compositor
                // has; the input region keeps following the last committed frame (it only changes with a new one or
                // when the banners go inert).
                let mut changed = false;
                let same = s.sent.logical == Some(want_logical) && s.sent.scale == Some(want_scale);
                let margin = margins(want_scale, &m, s.pos.0, s.pos.1, s.committed_size.unwrap_or(s.size));
                if same && s.sent.margin != Some(margin) {
                    if s.pres.pacer.can_commit(now) {
                        changed |= send_geometry(layer, s.viewport.as_ref(), &mut s.sent, want_logical, Some(margin), want_scale);
                    } else {
                        s.deferred = true;
                    }
                }
                if s.region_dirty {
                    let scale = s.sent.scale.unwrap_or(want_scale);
                    let rects = if inert { Vec::new() } else { logical_rects(&s.shape, scale) };
                    set_region(&self.compositor, layer.wl_surface(), &rects);
                    s.region_dirty = false;
                    changed = true;
                }
                if plan == Plan::Deferred {
                    s.deferred = true;
                }
                if changed {
                    layer.commit();
                    self.debug_dirty = true;
                }
                return;
            }
        }
        if let Plan::Refused(_) = plan {
            return;
        }
        // Draw the frame into the chosen buffer.
        let size = s.size;
        let (w, h) = size;
        let reused = matches!(plan, Plan::Reuse(_));
        let idx;
        let pm;
        match plan {
            Plan::Reuse(k) => {
                let Some(canvas) = s.pres.buffers.get(k).canvas(&mut self.pool) else {
                    s.pres.pacer.busy(now);
                    s.deferred = true;
                    return;
                };
                let Some(frame) = s.pres.take() else { return };
                copy_argb(frame.data(), canvas);
                pm = frame;
                idx = k;
            }
            _ => match self.pool.create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888) {
                Ok((buffer, canvas)) => {
                    let Some(frame) = s.pres.take() else { return };
                    copy_argb(frame.data(), canvas);
                    pm = frame;
                    idx = s.pres.insert(buffer, size);
                }
                Err(e) => {
                    // The frame stays pending; the retry is scheduled, not spun on.
                    if !s.refusal_logged {
                        crate::log_warn!("decnotify: cannot allocate a {w}x{h} shm buffer: {e}");
                        s.refusal_logged = true;
                        s.pres.stats.refused += 1;
                    }
                    s.pres.pacer.busy(now);
                    s.deferred = true;
                    self.debug_dirty = true;
                    return;
                }
            },
        }
        if s.pres.buffers.get(idx).attach_to(layer.wl_surface()).is_err() {
            // not actually free: keep the frame for the next attempt
            s.pres.frame = Some(pm);
            s.pres.pacer.busy(now);
            s.deferred = true;
            return;
        }
        let margin = margins(want_scale, &m, s.pos.0, s.pos.1, size);
        send_geometry(layer, s.viewport.as_ref(), &mut s.sent, want_logical, Some(margin), want_scale);
        layer.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
        let request = s.pres.pacer.should_request();
        if request {
            layer.wl_surface().frame(&self.qh, layer.wl_surface().clone());
        }
        // the input region follows the frame that is now attached
        s.shape = opaque_rects(&pm, 16);
        let rects = if inert { Vec::new() } else { logical_rects(&s.shape, want_scale) };
        set_region(&self.compositor, layer.wl_surface(), &rects);
        s.region_dirty = false;
        layer.commit();
        s.pres.committed(now, request, reused);
        s.committed_size = Some(size);
        s.refusal_logged = false;
        self.debug_dirty = true;
    }

    /// A catcher covers its output with one transparent buffer and takes input everywhere on it.
    fn present_catcher(&mut self, i: usize, now: Instant) {
        let viewporter = self.viewporter.is_some();
        let budget_left = MAX_TOTAL_BYTES.saturating_sub(self.total_bytes());
        let s = &mut self.surfaces[i];
        s.deferred = false;
        let Some(layer) = s.layer.as_ref() else { return };
        if !s.configured || s.area.0 == 0 || s.area.1 == 0 || s.covered == Some(s.area) {
            return;
        }
        let (lw, lh) = s.area;
        let (bw, bh, scale) = transparent_plan(lw, lh, viewporter);
        let size = (bw, bh);
        if s.attached != Some(size) {
            if !s.pres.pacer.can_commit(now) {
                s.deferred = true;
                return;
            }
            let idx = match s.pres.buffers.pick(size, budget_left) {
                Pick::Reuse(k) => {
                    let Some(canvas) = s.pres.buffers.get(k).canvas(&mut self.pool) else {
                        s.pres.pacer.busy(now);
                        s.deferred = true;
                        return;
                    };
                    canvas.fill(0);
                    k
                }
                Pick::New => match self.pool.create_buffer(bw as i32, bh as i32, bw as i32 * 4, wl_shm::Format::Argb8888) {
                    Ok((buffer, canvas)) => {
                        canvas.fill(0);
                        s.pres.insert(buffer, size)
                    }
                    Err(e) => {
                        if !s.refusal_logged {
                            crate::log_warn!("decnotify: cannot allocate a {bw}x{bh} shm buffer: {e}");
                            s.refusal_logged = true;
                            s.pres.stats.refused += 1;
                        }
                        s.pres.pacer.busy(now);
                        s.deferred = true;
                        return;
                    }
                },
                Pick::Busy => {
                    s.pres.pacer.busy(now);
                    s.deferred = true;
                    return;
                }
                Pick::Refused(_) => {
                    if !s.refusal_logged {
                        crate::log_warn!("decnotify: a click catcher buffer was refused by the memory budget");
                        s.refusal_logged = true;
                        s.pres.stats.refused += 1;
                    }
                    s.pres.pacer.busy(now);
                    s.deferred = true;
                    return;
                }
            };
            if s.pres.buffers.get(idx).attach_to(layer.wl_surface()).is_err() {
                s.pres.pacer.busy(now);
                s.deferred = true;
                return;
            }
            s.attached = Some(size);
        }
        if let Some(v) = &s.viewport {
            v.set_destination(lw as i32, lh as i32);
        }
        layer.wl_surface().set_buffer_scale(scale);
        layer.wl_surface().damage_buffer(0, 0, bw as i32, bh as i32);
        set_region(&self.compositor, layer.wl_surface(), &[(0, 0, lw as i32, lh as i32)]);
        layer.commit();
        s.covered = Some(s.area);
        s.pres.pacer.clear_busy();
        s.refusal_logged = false;
        self.debug_dirty = true;
    }

    /// The metrics the stack was laid out with (the applied scale and output size, not the latest events).
    fn layout_metrics(&self) -> Metrics {
        let s = self.scale.factor();
        let out = ((self.logical.0 as f32 * s).round() as i32, (self.logical.1 as f32 * s).round() as i32);
        wayland_metrics(self.policy, s, out, self.layout_bar)
    }

    /// Parks a surface the compositor closed (its output went away). Catchers are simply dropped. Returns the banner
    /// or panel that has to be rebuilt.
    fn closed(&mut self, layer: &LayerSurface) -> Option<(SurfId, Kind)> {
        let i = self.surfaces.iter().position(|s| s.layer.as_ref().is_some_and(|l| l.wl_surface() == layer.wl_surface()))?;
        if self.surfaces[i].kind == Kind::Catcher {
            let s = self.surfaces.remove(i);
            self.dispose(s);
            return None;
        }
        // Its buffers wait in the graveyard; a rebuilt surface starts with an empty set under the same bound.
        let entries = self.surfaces[i].pres.buffers.take_all();
        self.graveyard.extend(entries);
        // The output it was on is unusable (or the compositor is closing it): do not rebuild onto that output.
        self.target = None;
        self.fractional = None;
        let s = &mut self.surfaces[i];
        if let Some(v) = s.viewport.take() {
            v.destroy();
        }
        if let Some(f) = s.fractional.take() {
            f.destroy();
        }
        s.layer = None;
        s.configured = false;
        s.sent = Sent::default();
        s.committed_size = None;
        s.deferred = false;
        // callbacks requested on the closed surface never arrive
        s.pres.pacer = Default::default();
        Some((s.id, s.kind))
    }

    /// Gives every parked banner a new layer surface (on the pinned output, else wherever the compositor puts it).
    /// Needs an output; does nothing without one. Returns whether something was rebuilt.
    fn rebuild_parked(&mut self) -> bool {
        if self.output_state.outputs().next().is_none() {
            return false;
        }
        let parked: Vec<SurfId> = self.surfaces.iter().filter(|s| s.layer.is_none()).map(|s| s.id).collect();
        for id in &parked {
            let Some(i) = self.index(*id) else { continue };
            let output = self.creation_output();
            let layer = self.build_layer(self.surfaces[i].kind, output.as_ref());
            let (viewport, fractional) = self.scale_objects(layer.wl_surface(), true);
            let s = &mut self.surfaces[i];
            s.layer = Some(layer);
            s.viewport = viewport;
            s.fractional = fractional;
            s.region_dirty = true;
        }
        !parked.is_empty()
    }

    /// Banners ignore input while the history panel is open; their regions follow.
    fn set_inert(&mut self, inert: bool) {
        if self.inert == inert {
            return;
        }
        self.inert = inert;
        let banners: Vec<SurfId> = self.surfaces.iter().filter(|s| s.kind == Kind::Banner).map(|s| s.id).collect();
        for id in banners {
            if let Some(i) = self.index(id) {
                self.surfaces[i].region_dirty = true;
            }
        }
    }

    /// A transparent click catcher on every output that has none (while the history panel is open).
    fn make_catchers(&mut self) {
        self.catchers_wanted = true;
        for (_, output) in self.outputs() {
            if !self.surfaces.iter().any(|s| s.kind == Kind::Catcher && s.output.as_ref() == Some(&output)) {
                self.spawn(Kind::Catcher, (0, 0), (0, 0), Some(output));
            }
        }
    }

    fn drop_catchers(&mut self) {
        self.catchers_wanted = false;
        let ids: Vec<SurfId> = self.surfaces.iter().filter(|s| s.kind == Kind::Catcher).map(|s| s.id).collect();
        for id in ids {
            self.remove(id);
        }
    }

    /// Stores a painted frame. Only the latest is kept; it is committed by [`Self::present`], once per loop iteration.
    fn paint_pixmap(&mut self, id: SurfId, pm: Pixmap) {
        if let Some(i) = self.index(id) {
            self.surfaces[i].pres.submit(pm);
            self.debug_dirty = true;
        }
    }
}

impl Host for WlHost {
    type Win = SurfId;

    fn metrics(&self) -> Metrics {
        self.layout_metrics()
    }

    fn layout(&mut self, n: &Notification) -> BannerLayout {
        let m = self.layout_metrics();
        let mut layout = self.r.layout(&m, n);
        // a surface is a whole number of logical pixels
        (layout.w, layout.h) = self.scale.fit(layout.w, layout.h);
        layout
    }

    fn icon(&mut self, n: &Notification) -> Option<Pixmap> {
        self.r.resolve_icon(n)
    }

    fn create(&mut self, layout: &BannerLayout, x: i32, y: i32) -> Option<SurfId> {
        Some(self.spawn(Kind::Banner, (layout.w, layout.h), (x, y), None))
    }

    fn map(&mut self, _win: SurfId) {}

    fn destroy(&mut self, win: SurfId) {
        self.remove(win);
        self.r.fonts.trim();
    }

    fn resize(&mut self, win: SurfId, w: u32, h: u32) {
        if let Some(i) = self.index(win) {
            self.surfaces[i].size = (w, h);
        }
    }

    fn place(&mut self, win: SurfId, x: i32, y: i32) {
        if let Some(i) = self.index(win) {
            self.surfaces[i].pos = (x, y);
        }
    }

    fn paint(&mut self, b: &Banner<SurfId>) {
        let pm = self.r.paint_banner(b);
        self.paint_pixmap(b.win, pm);
        self.r.fonts.trim();
    }

    /// Nothing is sent here: commits are coalesced into [`WlHost::present`], once per loop iteration.
    fn flush(&mut self) {}
}

/// What a protocol handler saw; handled in [`Frontend::pump`] where the shared state is available.
#[derive(Debug, Clone, PartialEq)]
enum Input {
    Enter(SurfId),
    Leave(SurfId),
    /// Position in device pixels of the surface.
    Motion(SurfId, f32, f32),
    Press(SurfId, Btn, f32, f32),
    /// Whole wheel steps; positive scrolls down.
    Scroll(SurfId, i32),
    Sym {
        sym: u32,
        shift: bool,
    },
    /// The keyboard left this surface.
    KeyboardLeft(SurfId),
    /// The compositor closed this surface.
    Lost(SurfId, Kind),
}

struct State {
    registry_state: RegistryState,
    seat_state: SeatState,
    host: WlHost,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    inbox: Vec<Input>,
    mods: Modifiers,
    repeat: KeyRepeat,
    wheel: ScrollAccum,
    /// An output appeared: catchers and parked banners may need creating.
    outputs_changed: bool,
}

pub struct WlUi {
    // Kept alive for the queue's lifetime.
    _conn: Connection,
    queue: EventQueue<State>,
    state: State,
    stack: Stack<SurfId>,
    hist: Option<HistWin<SurfId>>,
    theme: ThemeWake,
    theme_generation: u64,
}

impl WlUi {
    pub fn connect() -> Result<WlUi, String> {
        let conn = Connection::connect_to_env().map_err(werr)?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(werr)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
        let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| format!("layer shell: {e}"))?;
        let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
        let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
        let fractional_manager = viewporter.as_ref().and_then(|_| globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok());
        let pool = SlotPool::new(64 * 1024, &shm).map_err(werr)?;
        let theme = ThemeWake::start()?;
        let theme_generation = crate::theme::live::generation();
        let host = WlHost {
            compositor,
            layer_shell,
            shm,
            pool,
            output_state: OutputState::new(&globals, &qh),
            viewporter,
            fractional_manager,
            qh: qh.clone(),
            r: Render::new(1.0)?,
            layout_bar: WlHost::read_bar_facts(),
            policy: InsetPolicy::from_env(std::env::var("DECNOTIFY_WAYLAND_INSETS").ok().as_deref()),
            fixed_output: std::env::var("DECNOTIFY_OUTPUT").ok().filter(|v| !v.is_empty()),
            target: None,
            fractional: None,
            scale: Scale::Integer(1),
            logical: (1920, 1080),
            surfaces: Vec::new(),
            next_id: 1,
            inert: false,
            catchers_wanted: false,
            graveyard: Vec::new(),
            gone: Stats::default(),
            debug_path: std::env::var_os("DECNOTIFY_DEBUG_WAYLAND").filter(|v| !v.is_empty()),
            debug_dirty: false,
        };
        let mut state = State {
            registry_state: RegistryState::new(&globals),
            seat_state: SeatState::new(&globals, &qh),
            host,
            pointer: None,
            keyboard: None,
            inbox: Vec::new(),
            mods: Modifiers::default(),
            repeat: KeyRepeat::default(),
            wheel: ScrollAccum::default(),
            outputs_changed: false,
        };
        // Learn the outputs (names, logical geometry, scale) and the seat's devices before the first banner.
        queue.roundtrip(&mut state).map_err(werr)?;
        queue.roundtrip(&mut state).map_err(werr)?;
        state.host.apply_layout();
        let mut stack = Stack::new();
        stack.generation = theme_generation;
        Ok(WlUi { _conn: conn, queue, state, stack, hist: None, theme, theme_generation })
    }

    /// Lays everything out again (theme, font scale, output size or scale change); ids, timers and hover are kept.
    fn reflow(&mut self, sh: &Shared) {
        self.state.host.layout_bar = WlHost::read_bar_facts();
        self.stack.reflow(&mut self.state.host);
        self.refresh_history(sh);
    }

    // ---- history panel

    fn close_history(&mut self) {
        if let Some(hw) = self.hist.take() {
            let host = &mut self.state.host;
            Host::destroy(host, hw.win);
            host.drop_catchers();
            host.set_inert(false);
            self.state.repeat.clear();
            self.state.wheel.reset();
        }
    }

    fn redraw_history(&mut self, sh: &Shared) {
        let host = &mut self.state.host;
        let m = host.layout_metrics();
        let scale = host.scale;
        let dims = history_dimensions(&host.r.fonts, &m, sh, &|w, h| scale.fit(w, h));
        let (x, y) = (m.x_for(dims.w), m.top);
        let s = host.r.s;
        let Some(hw) = &mut self.hist else { return };
        let (w, h) = (dims.w, dims.h);
        if hw.relayout(dims, x, y, sh, s) {
            host.resize(hw.win, w, h);
            host.place(hw.win, x, y);
        }
        let (scroll, rows) = (hw.scroll, hw.rows);
        host.r.prepare_history(sh, scroll, rows);
        let Some(hw) = &self.hist else { return };
        let pm = host.r.paint_history(sh, hw);
        host.paint_pixmap(hw.win, pm);
        host.r.fonts.trim();
        hw.debug(sh, hw.win);
    }

    fn apply_history(&mut self, outcome: HistOutcome, sh: &Shared) {
        match outcome {
            HistOutcome::Idle => {}
            HistOutcome::Redraw => self.redraw_history(sh),
            HistOutcome::Close => self.close_history(),
        }
    }

    fn is_panel(&self, id: SurfId) -> bool {
        self.hist.as_ref().is_some_and(|h| h.win == id)
    }

    // ---- input

    fn handle(&mut self, input: Input, sh: &mut Shared, out: &mut Vec<UiEvent>) {
        match input {
            Input::Enter(id) => {
                if let Some(i) = self.stack.index_of(id) {
                    self.stack.enter(&mut self.state.host, i);
                }
            }
            Input::Leave(id) => {
                if let Some(i) = self.stack.index_of(id) {
                    self.stack.leave(&mut self.state.host, i, sh.cfg.banner_seconds);
                } else if self.hist.as_mut().filter(|h| h.win == id).is_some_and(|h| h.clear_hover()) {
                    self.redraw_history(sh);
                }
            }
            Input::Motion(id, x, y) => {
                if let Some(i) = self.stack.index_of(id) {
                    self.stack.motion(&mut self.state.host, i, x, y);
                } else if self.hist.as_mut().filter(|h| h.win == id).is_some_and(|h| h.hover_at(x, y, sh)) {
                    self.redraw_history(sh);
                }
            }
            Input::Press(id, button, x, y) => {
                if let Some(i) = self.stack.index_of(id) {
                    out.extend(self.stack.press(&mut self.state.host, i, button, x, y));
                } else if self.is_panel(id) {
                    let Some(hw) = self.hist.as_mut() else { return };
                    let outcome = hw.click(x, y, sh);
                    self.apply_history(outcome, sh);
                } else if self.state.host.kind_of(id) == Some(Kind::Catcher) && self.hist.is_some() {
                    // a click beside the panel (or on another output) closes it and gives the keyboard back
                    self.close_history();
                }
            }
            Input::Scroll(id, steps) => {
                let s = self.state.host.r.s;
                if let Some(hw) = self.hist.as_mut().filter(|h| h.win == id) {
                    for _ in 0..steps.unsigned_abs().min(8) {
                        hw.scroll_step(steps < 0, sh, s);
                    }
                    self.redraw_history(sh);
                }
            }
            Input::Sym { sym, shift } => {
                let s = self.state.host.r.s;
                let Some(hw) = self.hist.as_mut() else { return };
                let outcome = hw.key(sym, shift, sh, s);
                self.apply_history(outcome, sh);
            }
            Input::KeyboardLeft(id) => {
                // The keyboard went to another client (a lock screen, another exclusive surface): the panel goes away.
                if self.is_panel(id) {
                    self.close_history();
                }
            }
            Input::Lost(id, kind) => {
                if kind == Kind::Panel && self.is_panel(id) {
                    self.close_history();
                }
            }
        }
    }
}

impl Frontend for WlUi {
    fn show(&mut self, n: Notification, secs: u32) {
        self.stack.show(&mut self.state.host, n, secs);
    }

    fn close(&mut self, id: u32, reason: u32) -> Vec<UiEvent> {
        self.stack.close(&mut self.state.host, id, reason)
    }

    fn history_open(&self) -> bool {
        self.hist.is_some()
    }

    fn toggle_history(&mut self, sh: &Shared) -> Vec<UiEvent> {
        if self.hist.is_some() {
            self.close_history();
            return Vec::new();
        }
        // Opening the history dismisses the banners and the queue (reason 2) before the panel appears.
        let events = self.stack.close_all(&mut self.state.host);
        let host = &mut self.state.host;
        let m = host.layout_metrics();
        let scale = host.scale;
        let dims = history_dimensions(&host.r.fonts, &m, sh, &|w, h| scale.fit(w, h));
        let (x, y) = (m.x_for(dims.w), m.top);
        host.set_inert(true);
        host.make_catchers();
        let id = host.spawn(Kind::Panel, (dims.w, dims.h), (x, y), None);
        self.hist = Some(HistWin::new(id, dims, x, y));
        self.redraw_history(sh);
        events
    }

    fn refresh_history(&mut self, sh: &Shared) {
        if self.hist.is_some() {
            self.redraw_history(sh);
        }
    }

    fn refresh_theme(&mut self, sh: &Shared) {
        self.theme.drain();
        let Some(generation) = self.state.host.r.retheme(self.theme_generation) else { return };
        self.theme_generation = generation;
        self.stack.generation = generation;
        // The icon cache dropped both successful and missing lookups; retained banners keep ids, expiry and hover.
        self.stack.reicon(&mut self.state.host);
        self.reflow(sh);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.stack.next_deadline()
    }

    fn expire(&mut self, now: Instant) -> Vec<UiEvent> {
        self.stack.expire(&mut self.state.host, now)
    }

    fn wait(&mut self, bus: RawFd, timeout: Option<Duration>) -> Result<Wake, String> {
        loop {
            self.queue.dispatch_pending(&mut self.state).map_err(werr)?;
            // One commit per surface per loop turn, right before sleeping: everything painted since the last turn
            // (a burst of input events included) has been coalesced into the latest frame. Frame callbacks and buffer
            // releases that just arrived are what lets a waiting frame out.
            let now = Instant::now();
            self.state.host.present(now);
            match self.queue.flush() {
                Ok(()) => {}
                Err(e) if would_block(&e) => {
                    // The socket buffer is full: wait for the compositor to drain it, then retry.
                    if let Some(guard) = self.queue.prepare_read() {
                        poll_writable(guard.connection_fd().as_raw_fd(), Duration::from_millis(100)).map_err(werr)?;
                    }
                    continue;
                }
                Err(e) => return Err(werr(e)),
            }
            let Some(guard) = self.queue.prepare_read() else { continue };
            let repeat = self.state.repeat.wait(now);
            // A deferred frame is looked at again when its callback's fallback or its busy retry is due.
            let timeout = [timeout, repeat, self.state.host.next_wake(now)].into_iter().flatten().min();
            let fds = [guard.connection_fd().as_raw_fd(), bus, self.theme.fd()];
            let ready = poll_readable(&fds, timeout).map_err(werr)?;
            if ready[0] {
                match guard.read() {
                    Ok(_) => {}
                    Err(e) if would_block(&e) => {}
                    Err(e) => return Err(werr(e)),
                }
            } else {
                drop(guard);
            }
            return Ok(Wake { bus: ready[1], theme: ready[2] });
        }
    }

    fn pump(&mut self, sh: &mut Shared) -> Result<Vec<UiEvent>, String> {
        self.queue.dispatch_pending(&mut self.state).map_err(werr)?;
        if let Some(sym) = self.state.repeat.due(Instant::now()) {
            let shift = self.state.mods.shift;
            self.state.inbox.push(Input::Sym { sym, shift });
        }
        let mut relayout = false;
        if std::mem::take(&mut self.state.outputs_changed) {
            if self.state.host.catchers_wanted {
                self.state.host.make_catchers();
            }
            relayout |= self.state.host.rebuild_parked();
        }
        // A new output, scale or size: the stack is laid out again with the same ids, timers and hover.
        if self.state.host.apply_layout() {
            self.stack.reicon(&mut self.state.host);
            relayout = true;
        }
        let inputs = std::mem::take(&mut self.state.inbox);
        let mut out = Vec::new();
        for input in inputs {
            if let Input::Lost(id, kind) = &input {
                // a closed banner is rebuilt (on another output when its own is gone) and drawn again
                if *kind == Kind::Banner && self.state.host.index(*id).is_some() {
                    self.state.host.rebuild_parked();
                    relayout = true;
                }
            }
            self.handle(input, sh, &mut out);
        }
        if relayout {
            self.reflow(sh);
        }
        // Nothing on screen: forget the output so the next banner follows the compositor's focus again.
        if self.stack.banners.is_empty() && self.hist.is_none() && self.state.host.surfaces.is_empty() && self.state.host.fixed_output.is_none() {
            self.state.host.target = None;
            self.state.host.fractional = None;
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- protocol handlers

impl CompositorHandler for State {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        // The frame callback of a committed frame: the surface may present its newest one on the next turn.
        self.host.callback_done(surface);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, output: &wl_output::WlOutput) {
        // The compositor chose where the first surface went (its focused output): later surfaces follow it.
        if self.host.target.is_none() && self.host.fixed_output.is_none() && self.host.lookup(surface).is_some_and(|(_, kind)| kind != Kind::Catcher) {
            self.host.target = Some(output.clone());
        }
    }
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.host.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.outputs_changed = true;
    }
    // A resize, rescale or rotation shows up in the output info; `pump` compares it with what the stack was laid out for.
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if self.host.target.as_ref() == Some(&output) {
            self.host.target = None;
            self.host.fractional = None;
        }
        let gone: Vec<SurfId> = self.host.surfaces.iter().filter(|s| s.kind == Kind::Catcher && s.output.as_ref() == Some(&output)).map(|s| s.id).collect();
        for id in gone {
            self.host.remove(id);
        }
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some((id, kind)) = self.host.closed(layer) {
            self.inbox.push(Input::Lost(id, kind));
        }
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let Some((id, kind)) = self.host.lookup(layer.wl_surface()) else { return };
        let Some(i) = self.host.index(id) else { return };
        self.host.debug_dirty = true;
        let s = &mut self.host.surfaces[i];
        s.configured = true;
        if kind == Kind::Catcher {
            s.area = configure.new_size;
        }
        // The frame that waited for the first configure (and the catcher's transparent buffer) goes out in the next
        // `present`, before the loop sleeps.
    }
}

impl SeatHandler for State {
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
                self.repeat.clear();
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let f = self.host.scale.factor();
        for event in events {
            let Some((id, _)) = self.host.lookup(&event.surface) else { continue };
            // surface-local logical pixels back to the device pixels the layout and the hit rectangles use
            let (x, y) = (event.position.0 as f32 * f, event.position.1 as f32 * f);
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.inbox.push(Input::Enter(id));
                    self.inbox.push(Input::Motion(id, x, y));
                }
                PointerEventKind::Leave { .. } => {
                    self.wheel.reset();
                    self.inbox.push(Input::Leave(id));
                }
                PointerEventKind::Motion { .. } => self.inbox.push(Input::Motion(id, x, y)),
                PointerEventKind::Press { button, .. } => {
                    if let Some(button) = button_of(button) {
                        self.inbox.push(Input::Press(id, button, x, y));
                    }
                }
                PointerEventKind::Release { .. } => {}
                PointerEventKind::Axis { vertical, .. } => {
                    if vertical.stop {
                        self.wheel.reset();
                    }
                    let steps = self.wheel.push(vertical.discrete, vertical.absolute);
                    if steps != 0 {
                        self.inbox.push(Input::Scroll(id, steps));
                    }
                }
            }
        }
    }
}

impl KeyboardHandler for State {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        self.repeat.clear();
        if let Some((id, _)) = self.host.lookup(surface) {
            self.inbox.push(Input::KeyboardLeft(id));
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        let sym = event.keysym.raw();
        self.repeat.press(event.raw_code, sym, Instant::now());
        self.inbox.push(Input::Sym { sym, shift: self.mods.shift });
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.repeat.release(event.raw_code);
    }
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: u32) {
        self.mods = modifiers;
    }
    fn update_repeat_info(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, info: RepeatInfo) {
        match info {
            RepeatInfo::Repeat { rate, delay } => self.repeat.set_info(rate.get(), delay),
            RepeatInfo::Disable => self.repeat.disable(),
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.host.shm
    }
}

impl Dispatch<WpViewporter, ()> for State {
    fn event(_: &mut Self, _: &WpViewporter, _: wp_viewporter::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WpViewport, ()> for State {
    fn event(_: &mut Self, _: &WpViewport, _: wp_viewport::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for State {
    fn event(_: &mut Self, _: &WpFractionalScaleManagerV1, _: wp_fractional_scale_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WpFractionalScaleV1, ()> for State {
    fn event(state: &mut Self, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.host.fractional = Some(scale).filter(|&s| s > 0);
        }
    }
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_pointer!(State);
delegate_keyboard!(State);
delegate_layer!(State);
delegate_registry!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
