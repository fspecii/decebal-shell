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

## Environment

| Variable | Effect |
| --- | --- |
| `DECEBAL_BACKEND=x11\|wayland` | force the display server (never silently switched) |
| `DECNOTIFY_OUTPUT=NAME` | Wayland: always use this output |
| `DECNOTIFY_WAYLAND_INSETS=theme` | Wayland: add the theme's bar insets to the margins |
| `DECNOTIFY_DEBUG_BANNERS=FILE`, `DECNOTIFY_DEBUG_HISTORY=FILE` | write geometry/state JSON (ids, rects, deadlines, scale); no notification text for banners |

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
`tests/wayland-notification-fixture.py`: parsers of the native fixture.

## Native fixture (draft, not run)

`tests/e2e/wayland_notifications.py --bin <immutable decnotify> --out <NEW dir of <= 60 bytes> [--phases ...]`.
Private HOME/XDG tree, a private session bus from a fixed configuration, an owned headless Sway with two outputs
(1.5 and 1), `DISPLAY` unset, `WAYLAND_DEBUG=client`, kernel peer-credential ownership checks, pidfd cleanup, no
deletion. Phases: `contract` (layer/anchor/zone/keyboard/empty region/no tiled window), `bus` (second daemon exits 0
and takes nothing), `queue`, `actions` (click without losing the sink's focus), `history` (Escape, same-output and
other-output outside clicks, keyboard hand-back, no action replay), `persistence` (unwritable state directory, error
card, Retry click), `dnd` (and a recording `canberra-gtk-play`), `themes`, `scales` (2, 1, 1.5 and an output added),
`pinned`.

## Known limits (honest list)

* Nothing has been built or run. Compile errors, SCTK API mismatches and fixture mistakes are likely and expected.
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
