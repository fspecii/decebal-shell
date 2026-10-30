# Decebal / rdwm: unified window-shadow design

2026-10-30. The source research below guided the implemented X11 SSD carrier.

Implementation checkpoint: `shadow`, `shadow_radius`, `shadow_opacity` and
`shadow_inactive_opacity` are typed rdwm decoration settings, default off. Decebal
maps finite theme strength 0..1.5 to focused/unfocused opacity, preserves a manual
radius, and consults the actual live parser before adding fields. An upgraded
CLI cannot cause an older running WM to reject a theme reload.

Private Xvfb/Picom xrender verification passed 45 native assertions: all four
edge pixels, no caption/body seam, unchanged app interiors, input transparency,
stacking, fullscreen/workspace/minimize/death/disable and compositor stop/start.
First and subsequent rapid moves upload zero shadow pixels. Resize uploads use
bounded peripheral chunks; falloff kernels are cached (full nine-slice corner
caching remains a possible optimization). Two test carrier drawables represented
3,853,184 backing pixel bytes, excluding server overhead and compositor copies.
Actual NVIDIA frame timing, multi-monitor DPI transitions and Wayland remain
separate gates; this is not a claim that the complete premium-effect scope below
has been verified. No live WM was restarted during testing.

## Source evidence recorded before implementation

- `crates/rdwm-x11/src/frames.rs:1–7` explicitly documents no reparenting. `make_frame` (line 256) creates one root-child ARGB InputOutput titlebar and three InputOnly resize strips. `Frame::windows` currently returns those four windows; there is no whole-window drawable.
- `Cmd::Decorate` in `crates/rdwm-core/src/api.rs:354` supplies the correct complete outer frame rectangle (caption and body), after the client's Configure. `Wm::th` / `frame_rect` in `crates/rdwm-core/src/wm/decor.rs:41/71` already define fullscreen/undecorated suppression and geometry.
- `layout_frame`, `map_frame`, `stack_frame_above`, `unmap_frame`, `deco_destroy` in frames.rs (lines 373, 495, 515, 529, 550) own lifecycle; `Cmd::Restack` in `crates/rdwm-x11/src/lib.rs:1962` orders each client followed by its title/strips. New shadow placement must change this ordering rather than add another always-on-top shell surface.
- `draw_frame` (frames.rs:584) caches `(width,title,focus,hover)` and therefore does not rerasterize a caption just because it moves. Preserve that property. `put_pixels` (line 228) currently uploads a complete origin-aligned image; it is unsuitable unchanged for cheap peripheral shadow updates.
- `deco_setup` (frames.rs:130) probes depth-32 TrueColor, SHAPE and compositor ownership. `refresh_compositing` (line 415) throttles ownership reads to one second; currently reached via frame layout. A new shadow needs reliable start/stop handling while geometry is stationary too.
- Actual `~/.config/picom/picom.conf` has GLX, vsync, use-damage, `shadow=false`, no fading, no unredirect and `no-frame-pacing=true`. Its rules enable background blur only for `_DECEBAL_BLUR@=1`. Preserve no-frame-pacing and unrelated options.
- `contrib/picom/decebal-glass.conf` in the shell repo describes blur only. Decebal's `metrics.shadow` is parsed/resolved/captured (`src/theme/model.rs:132,292,557`, `engine/capture.rs:112`), but searching shell source found no runtime consumer. It does not currently set a WM shadow.
- Wayland has native per-window decoration buffers in `crates/rdwm-wayland/src/deco.rs::update_deco_bufs` (line 221) and composes them as render elements in render.rs. The X11 proposal below is not Wayland shadow support.

## Smallest correct X11 implementation

Use **one WM-owned ARGB shadow carrier beneath the complete decorated frame**, with a transparent interior and a premultiplied alpha shadow only outside its silhouette. Keep Picom-generated shadows disabled for the client, caption and carrier. This avoids dependence on Picom interpreting unrelated top-level windows as one object, avoids a caption/body seam, and does not require changing rdwm into a reparenting WM or maintaining a Picom fork.

The carrier is an override-redirect root-child InputOutput window, placed at `(frame.x-margin, frame.y-margin)` and sized `(frame.w+2*margin, frame.h+2*margin)`. Require ARGB, compositor ownership and SHAPE Input support; otherwise omit it (normal decorations/input remain functional). Its input region is empty, not merely an event mask with no events: mouse clicks, drag/drop and scroll must pass through to the real underlying window. Give it an explicit WM class/name and a dedicated owner marker. Do not add it to EWMH client lists, focus traversal or task switchers. Do not reuse ordinary decoration hit routing: the carrier is not a clickable caption/resize edge.

Generate the shadow from the actual union contour: rounded upper caption corners and the presently rectangular body/bottom border. Zero all alpha inside that contour. Do not claim rounded lower application corners—rdwm currently does not clip those. A Gaussian/soft-distance profile over the union contour (one silhouette, not separate caption/body masks) provides a single continuous penumbra. A modest vertical offset is a supported parameter; include offset in margin bounds.

Add a backend-private shadow handle to Frame, separate from `Frame::windows()` above-client ordering. All map/unmap/destroy paths include it. The complete bottom-to-top group is:

`previous group's top → this shadow → this client → this resize strips → this caption`.

`Cmd::Restack` must place the carrier immediately above the previous group's top, then this client above its own carrier, and continue existing caption/strip stacking. Initial map should explicitly place the carrier below its own client before exposing it. Never raise every shadow globally. Hidden/minimized/workspace-switched/fullscreen/released/destroyed clients must not leave a mapped carrier. Existing th=0 destroys decoration, so v1 can intentionally support only server-decorated windows; CSD-native and undecorated apps need an explicit separate policy rather than falsely claiming universal coverage.

## Performance contract

- Precreate/rasterize at map or theme/size/focus change; a plain move sends ConfigureWindow geometry only. No image allocation, blur convolution, PutImage, synchronous X reply or server grab per drag event. Preserve `queue_xevent` coalescing and resize-sync logic in lib.rs.
- Cache corner/edge kernels by radius/scale/strength/focus, not screen position. On resize, render/reuse nine-slice edge pieces; add a bounded rectangle uploader (destination x/y) beside put_pixels. Clear the carrier backing to transparent on size change, upload only peripheral strips/corners. Avoid constructing/uploading a full `(w+2m)*(h+2m)` CPU image per pointer motion. The server/compositor still allocates one full carrier backing: account for that memory in measurement.
- Focus only changes the alpha-strength cache; title/hover updates must not rerender a shadow. Reducing effects/strength=0 should unmap/free carrier buffers rather than keep a hidden expensive renderer.
- Move client + carrier + caption in the same existing command batch/flush; don't add independent timers or animate geometry separately. Test the first drag after idle explicitly.
- Detect compositor start/stop while windows are stationary (e.g. ownership notification or bounded existing event-loop housekeeping). No fake opaque shadow fallback without a compositor. Clear ownership safely on exit/restart.

## Theme/config integration

Add a real bounded typed shadow config to rdwm-deco/core/schema/docs (enabled, radius/spread, offset, active/inactive strength), with sensible hard limits and default-off until the private pixel/performance tests pass. Map Decebal's existing shadow metric to that config through the theme engine; keep per-theme disable and accessibility reduce-effects semantics explicit. Avoid serializing an unsupported string into raw `extra`. A native preview can use the same contour/kernel renderer, labelled as composited sample, once real runtime exists.

A compositor rule for the WM carrier/caption can explicitly specify `shadow=false`, `blur-background=false`, `corner-radius=0`, no fade; use a marker for managed clients with WM-owned shadow if users enable global shadows. Picom's installed primary manual (`/usr/share/man/man1/picom.1.gz`, WINDOW RULES) confirms modern per-window `shadow` rules and warns that using `rules` supersedes legacy shadow-exclude options. Merge into the existing rules array rather than appending a second competing rules key. Do not change no-frame-pacing. Do not rely on `_NET_FRAME_EXTENTS` to join these separate windows; it is only metadata here.

## Acceptance gates

1. Unit: silhouette union and zero interior; premultiplication; dark/light parameters; cache key excludes x/y/title/hover but includes w/h/focus/config/scale; clamped extreme sizes; full disable/no-compositor fallback.
2. Private Xvfb/Xorg with real Picom: inspect pixels around all four edges and the caption/body join, compare alpha falloff, verify no seam/no double-shadow. Validate stacking across two overlapping apps, lower shadows never covering upper client, dock/rail ordering, menus and transient dialogs.
3. Real input delivery: click/scroll through every peripheral shadow edge onto a second app; resize edges/caption buttons still work; carrier never gets focus or appears in `_NET_CLIENT_LIST`.
4. Lifecycle: rapid fullscreen toggle, minimization, workspace switch, theme reload, compositor stop/start, app crash/WM clean shutdown; query root children for leaked carrier XIDs.
5. Performance: record Configure vs PutImage counts and upload bytes during 10s rapid move/resize with same-size and changing-size windows. Pure movement must produce zero shadow uploads after warm map; first new drag must not rerasterize. Compare p95/p99 input-to-geometry and compositor frame timing against current no-frame-pacing baseline on the NVIDIA machine; inspect drag/resize video for black regions and detached shadows. Measure resident/server pixmap memory across 1/10/30 windows and verify disabled themes release carriers.
6. Separate Wayland follow-up: compose the same cached nine-slice shadow geometry behind each window subtree in render.rs with expanded damage bounds, output-scale aware buffers, zero input surface; test GLES and pixman. No Picom or X11-carrier claim applies there.

## Main risks / scope

One additional ARGB drawable per server-decorated app costs server/GPU memory; excessive blur margins magnify it. Client transparency reveals interior mistakes immediately. Geometry batching is not atomic across distinct X windows, so fast-drag pixels must be verified rather than inferred from X request counts. Fullscreen, transients, multi-monitor scale changes and compositor restarts are mandatory, not optional polish. Native CSD shadows/corners remain a separate case. This proposal is the smallest real seam-free X11 path identified from the current architecture; enabling `shadow=true` globally is not equivalent.

## Using the implemented X11 effect

After upgrading, start a new rdwm session before expecting the backend effect.
A running older WM remains unchanged. Applying a theme in the new session maps
`metrics.shadow` (0..1.5) to active/inactive opacity; zero disables and frees
carriers. The manually configured `decoration.shadow_radius` is preserved.
A new CLI checks the running parser, so theme application remains compatible
with the older WM until the session changes. There is no live WM restart during
installation. Wayland and undecorated/CSD windows do not use this effect yet.

Run `make e2e-shadows` in rdwm for native raster/input/lifecycle gates, and
`make e2e-theme-shadows` in the shell checkout for theme-to-runtime integration.
