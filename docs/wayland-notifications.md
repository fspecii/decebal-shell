# Native Wayland notifications

`decnotify` is the `org.freedesktop.Notifications` daemon on X11 **and** on a Wayland compositor with
wlr-layer-shell. The old "leave Wayland sessions to mako" opt-out (`DECNOTIFY_FORCE`) is gone: the display
server is chosen by `backend::session` like every other Decebal program (`DECEBAL_BACKEND=x11|wayland` forces one;
`XDG_SESSION_TYPE=wayland` prefers Wayland; a compositor without layer-shell falls back to XWayland when there
is a `DISPLAY`). With no usable display `decnotify` exits 0 without requesting the bus name.

> **Status: source only.** Nothing here has been compiled, unit-tested or run yet. The gates below are the
> acceptance list for the root qualification; none of them has a result.

## What is shared and what is not

| Part | Where | X11 | Wayland |
| --- | --- | --- | --- |
| D-Bus service, history store, DND / sound / mute / transient / critical rules | `notify/mod.rs`, `model.rs`, `sound.rs` | same code | same code |
| Queue (max 5 visible), replacement, retained deadlines, hover pause + grace, expiry/close reasons, middle = dismiss all, resident actions, stack layout | `notify/view.rs` (`Stack`) | via `Host` | via `Host` |
| History panel: layout, Tab / Shift+Tab / Enter / Space / Delete / Escape, wheel pan + row scroll, hover, Clear / DND / Retry, painting | `notify/history.rs` | same code | same code |
| Scales, input regions, key repeat, wheel accumulation, bar-inset policy | `notify/geom.rs` | n/a | pure, unit tested in every build |
| Bounded, coalesced frame presentation (buffers, frame callbacks, budgets) | `notify/present.rs` | n/a | pure state machine, unit tested in every build |
| Windows, pixels, native input | `notify/ui.rs` (x11rb) / `notify/wl.rs` (SCTK) | override-redirect windows | wlr-layer-shell surfaces |

The bus loop (`notify::serve`) is one generic function over the `Frontend` trait. Each front end sleeps in one
`poll` over the bus fd, its display connection and the theme watcher; there is no runtime per notification, no
thread, and no second connection.

## Surface contract (Wayland)

* **Banners**: one `Layer::Top` surface each, namespace `decebal-notifications`, anchored top + right, **exclusive
  zone 0**, **keyboard interactivity none**. A banner can be clicked but never takes keyboard focus. With nothing
  to show there is **no surface at all**. Every surface has an **explicit empty input region** from creation until
  its first frame is committed; then it takes input only on the rounded outline of the card (corners pass clicks
  through). Layer surfaces are not windows: nothing is tiled.
* **History panel**: a `Layer::Overlay` surface with an **exclusive keyboard**, plus a transparent *catcher*
  (`decebal-notifications-outside`, `Layer::Top`, all anchors, zone -1) on **every output**. A click on a catcher
  closes the panel, which destroys the surfaces and hands the keyboard back; Escape does the same. While the panel
  is open banner surfaces get an empty input region, so a click on one also closes the panel (as on X11, where the
  pointer grab swallows it). Losing the keyboard to another client (lock screen) closes the panel.
* **Where**: top right. With zone 0 the compositor keeps surfaces clear of other layers' exclusive zones (decbar,
  pill bars, waybar), so the margins are only the visual gaps (8 / 12 px, 12 px right, scaled). The theme's bar
  sizes only bound the stack's height and width. `DECNOTIFY_WAYLAND_INSETS=theme` instead adds the theme's bar
  insets to the margins exactly like the X11 placement, for bars that reserve no exclusive zone.
  `DECNOTIFY_OUTPUT=NAME` pins every surface to one output; otherwise the first surface goes where the compositor
  puts it (its focused output) and later ones follow until nothing is shown.
* **Scale**: layout is in device pixels at the output's scale. An integer scale uses `set_buffer_scale`; a
  fractional one (wp-fractional-scale-v1 + wp-viewporter) a viewport destination with buffer scale 1. Surface
  sizes are snapped so the buffer is exactly `round(logical x scale)`. Pointer positions are multiplied back, so
  hit rectangles are the painted pixels. A scale, size or output change re-lays everything out keeping ids,
  timers and hover (`Stack::reflow`). An output that disappears closes its surfaces; banners are rebuilt on
  another output (or parked until one exists), the history panel is closed.
* **Keyboard repeat** is done client side from `wl_keyboard.repeat_info` (the panel only).

## Presentation (Wayland): bounded and coalesced

Painting never talks to the compositor. `Host::paint` only stores the newest `Pixmap` of a surface (an older unshown
one is replaced and counted as *coalesced*), `Host::flush` does nothing, and `WlHost::present` commits once per
event-loop turn, right before the loop sleeps and again after each dispatch (so a callback or release that just
arrived is acted on at once). A burst of input is therefore one commit per surface, not one per event.

Per banner or panel surface (`notify/present.rs`):

* at most **3 shm buffers**, *counting buffers of an older size or scale that the compositor has not released*. A buffer
  is reused only when released; a size or scale change that finds all three held waits for a release instead of
  allocating; a released buffer of another size is destroyed at once (`BufferSet::reap`). SCTK's `SlotPool` retains an
  active buffer whose handle is dropped, so no active buffer is ever dropped: the handles are kept in the set.
* the frame is attached together with its size, scale/viewport, margins and **input region**, and asks for a **frame
  callback**; the next frame waits for it. The fallback is `FRAME_STALL` (1 s): after that a frame goes out anyway, never
  more than `MAX_CALLBACKS` (2) callbacks are outstanding, and past that commits proceed without asking for more.
* when all buffers are busy the retry is scheduled (16 ms, then every 250 ms after a second) and fed to the `poll`
  timeout (`WlHost::next_wake`, never below 1 ms); releases and callbacks wake the loop sooner. Nothing spins.
* the input region and margins stay those of the **last committed frame** until a new frame is attached; a margin-only
  change (a banner above closed) is sent without a repaint when size and scale are unchanged.
* budgets: 16 MiB per buffer, 32 MiB per surface, 96 MiB for the daemon, **including** the *graveyard* (buffers of
  destroyed or parked surfaces the compositor still holds, dropped the moment they are released). A refused allocation
  is logged once per episode (`log_warn`) and counted; the surface keeps its last frame. A failed shm allocation is
  logged and retried on the schedule above. Normal cards are card-sized (about 0.1-4 MiB); nothing is output-sized.
  A catcher keeps its single transparent buffer (1x1 with a viewport) and is not repainted.
* when no buffer is left anywhere, a pool that grew past 1 MiB is replaced by a small one.

`DECNOTIFY_DEBUG_WAYLAND=FILE` writes the counters (per surface and totals: buffers and bytes held, created/reused,
commits, coalesced, busy, stale, refused, callbacks requested/done, maxima, graveyard, pool size, limits) for the fixture.

## Environment

| Variable | Effect |
| --- | --- |
| `DECEBAL_BACKEND=x11\|wayland` | force the display server (never silently switched) |
| `DECNOTIFY_OUTPUT=NAME` | Wayland: always use this output |
| `DECNOTIFY_WAYLAND_INSETS=theme` | Wayland: add the theme's bar insets to the margins |
| `DECNOTIFY_DEBUG_BANNERS=FILE`, `DECNOTIFY_DEBUG_HISTORY=FILE` | write geometry/state JSON (ids, rects, deadlines, scale); no notification text for banners |
| `DECNOTIFY_DEBUG_WAYLAND=FILE` | Wayland: write the presentation counters (above) |

## Behaviour that must not change (and where it is held)

Close reasons 1 (expired) / 2 (dismissed, including history opening and middle click) / 3 (`CloseNotification`) and
`ActionInvoked` ordering are produced in `Stack::{expire,close_all,press,close}`, the same functions X11 uses.
Removing a history row only edits the history (`HistWin::activate` returns an outcome, never an event), so it cannot
replay an action. Muted, transient, critical, DND and sound decisions are in `Daemon::dispatch`, untouched.

## Unit tests (written, not run)

`view.rs`: stack limits, queue order, replacement, retained deadlines, hover/grace, expiry, press semantics, resident,
reflow, input outline, placement origin. `history.rs`: tab order and wrap, Escape / Delete / Enter / Space, row removal,
wheel pan then scroll, hover, relayout bounds. `geom.rs`: fit/logical idempotence at integer and fractional scales,
inset policy, margins, input-region conversion, transparent catcher plan, wheel accumulation, key repeat, output pick.
`present.rs`: latest-frame-only coalescing, frame-callback wait and the stall fallback, the callback bound, delayed
releases never passing 3 buffers, busy retry back-off (no zero-length sleeps), size and scale churn staying bounded with
old-size buffers counted, stale frames dropped, oversize and budget refusals, reuse only of released same-size buffers.
`tests/wayland-notification-fixture.py`: parsers of the native fixture.

## Native fixture (draft, not run)

`tests/e2e/wayland_notifications.py --bin <immutable decnotify> --out <NEW dir of <= 60 bytes> [--phases ...]`.
Private HOME/XDG tree, a private session bus from a fixed configuration, an owned headless Sway with two outputs
(1.5 and 1), `DISPLAY` unset, `WAYLAND_DEBUG=client`, kernel peer-credential ownership checks, pidfd cleanup, no
deletion. Phases: `contract` (layer/anchor/zone/keyboard/empty region/no tiled window), `bus` (second daemon exits 0
and takes nothing), `queue`, `actions` (click without losing the sink's focus), `history` (Escape, same-output and
other-output outside clicks, keyboard hand-back, no action replay), `persistence` (unwritable state directory, error
card, Retry click), `dnd` (and a recording `canberra-gtk-play`), `themes`, `scales` (2, 1, 1.5 and an output added),
`burst` (600 pointer motions in one IPC call and 150 Tab presses: far fewer commits, buffers and callbacks within
the bounds on the wire and in the daemon's counters), `churn` (batched scale changes with banners and the panel up,
then everything closed: no surfaces, no held buffers, small pool, bounded RSS growth), `pinned`.

## Known limits (honest list)

* Nothing has been built or run. Compile errors, SCTK API mismatches and fixture mistakes are likely and expected.
  This includes the presentation rework: the bounds are proven only by the pure model's unit tests, not on a compositor.
* The coalescing relies on one `present` per loop turn; the loop wakes for input, bus traffic, the theme watcher, key
  repeat and the deferred-frame deadline only, so a frame can wait up to one frame callback (about one display
  refresh, at most 1 s when callbacks are lost) behind the previous one.
* The whole frame is copied into the buffer (no damage tracking) and the full surface is damaged: cheap for cards,
  but the largest history panel at 2x is about 3.7 MiB per commit.
* Compositors that never send frame callbacks to an occluded or dpms-off output limit a surface to one commit per
  second there (and two outstanding callbacks).
* `Frontend::pump` re-evaluates the output scale and size on every loop turn; no dedicated resize event is used.
* The first banner is laid out for a guessed output (the first one, or `DECNOTIFY_OUTPUT`) and re-laid out when the
  compositor reports where it put the surface; a one-frame mis-scaled first buffer is possible on a mixed-scale layout.
* Exclusive-zone handling is assumed from the layer-shell contract; a bar that does not reserve space needs
  `DECNOTIFY_WAYLAND_INSETS=theme`. Zones are modelled only to bound the height; a very tall stack on a screen with a
  large bar may overlap it by that bar's size.
* No cursor-shape protocol is used; the pointer image over a banner is whatever the compositor keeps.
* Smooth-scroll wheels that report only `axis_value120` accumulate through absolute pixels (20 px per step).
* Hover highlight inside the history panel clears when the pointer leaves; X11 keeps it (pointer grab).
* Closing the panel when the keyboard is taken by another client is a Wayland-only rule (X11 grabs the keyboard).
* A catcher covers bars on its output while the panel is open (Layer::Top, zone -1); clicks there close the panel.
* The unit tests cannot toggle Do Not Disturb from the panel (it writes the user's settings file); the native
  `dnd` phase covers the CLI path and the panel toggle is not clicked by the draft fixture.
* No portal work, no accessibility bridge for the Wayland banners (the X11 ones had none either).

## Follow-ups outside this change

`README.md` (still says X11 only), `contrib/decebal-notify.service`, `contrib/dbus/org.freedesktop.Notifications.service`
(both say decnotify refuses to run under Wayland), the `make e2e` notification target, packaging, and the
decsettings notification pane text should be revisited after qualification.

## Focused presentation verification (2026-11-01)

The exact `src/notify/present.rs` at c02a3d9 was compiled as a standalone
Rust test target with the existing cached `serde_json` dependency. All 13
presentation tests passed, including delayed releases, resize/scale churn,
callback fallback, latest-frame coalescing, and memory-budget refusal.
The module SHA-256 before and after was
`e3a694daf021daa2a2acf0c7466372ff7d90c2f77419d4cc82bbbb977ff6eb2d`.
Evidence: `/var/tmp/decebal-notify-present-unit-retry2/result.json` and
`tests.log` in that directory. The resource runner restored the shared pause.

This proves the presentation state-machine tests only. It does not qualify
the whole crate, SCTK transport, compositor behavior, or native rendering.
The preceding standalone attempt failed because its harness omitted the
`serde_json` dependency; it is retained as failed harness evidence.

### Source review repairs awaiting qualification (2026-11-01)

Wayland presentation now uses bar extents captured for the stack's layout.
Only initial setup and reflow read bar configuration; pointer/frame/release
presentation turns no longer call the config-reading helper. Margins therefore
stay tied to the current stack layout. A native config-open count remains needed.

A newly released buffer now bypasses the busy sleep deadline, while the frame
callback gate remains intact. A regression releases a buffer 1 ms into the
250 ms back-off and requires immediate reuse. Failure-path log throttling and
the distinction between logical buffer bytes and SlotPool allocation remain
open review findings. These latest edits have only been parsed/formatted so far.

Presentation verification after the repair: all 14 exact `present.rs` tests
passed, including immediate reuse after release during long back-off. Module
SHA-256 `dbe5adaf9ab544293a1b1f1fc6f53f4c7acb2ef89cc6aa976e75e369302c3e22`;
evidence `/var/tmp/decebal-notify-present-unit-repair/result.json`. The wl.rs
layout-cache change is not compiled or runtime-qualified by this module run.
