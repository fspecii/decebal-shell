# Desktop widgets host: session choice, readiness and Wayland lifecycle

Source repairs are committed and pushed as `e6981ae`. Focused tests, strict linting and development builds pass; native widget lifecycle and optimized-release acceptance remain pending. The qualified live desktop is unchanged.

## Which server (shared with every surface)

`crate::backend::session` holds the one policy for panels (`backend::run`), the widgets host and popups:

| `DECEBAL_BACKEND` | `XDG_SESSION_TYPE` | `DISPLAY` | `WAYLAND_DISPLAY` | result |
|---|---|---|---|---|
| `x11` | any | set | any | X11 (no compositor probe) |
| `x11` | any | unset | any | none (error, never a silent switch) |
| `wayland` | any | any | set + protocol | Wayland; otherwise none |
| unset | `wayland` | any | set + protocol | Wayland; without the protocol XWayland when `DISPLAY` is set |
| unset | `x11`, `tty`, unset, other | set | any | X11, even with a leaked/nested `WAYLAND_DISPLAY` |
| unset | any | unset | set + protocol | Wayland |

API: `Mode { X11, Wayland }`, `prefer_wayland(forced, session, display, wayland) -> bool`,
`choose_mode(forced, session, display, wayland, wayland_supported) -> Option<Mode>`, plus `decide(.., probe: FnOnce)`,
`Env::current()`, `detect(probe)` and `panel_mode()` (cached per process). The probe runs at most once and only when the
answer depends on it.

Behaviour change: a session with `DISPLAY` set and no `XDG_SESSION_TYPE=wayland` (for example a compositor started
from a tty that does not export the session type) now puts panels and widgets on XWayland/X11.

## One host per server

* X11 keeps the `decwidgets` lock and the `_DECEBAL_WIDGET_HOST_S<screen>` selection.
* A Wayland host uses lock `decwidgets-wl-<WAYLAND_DISPLAY>`, so a nested compositor or second session gets its own host.

## Readiness record

`$XDG_RUNTIME_DIR/<lock>.state` holds the host's pid, that pid's start time, the server (socket path, or `x11:<DISPLAY>`),
the socket's device and inode, the configuration stamp it built and a tally (`wanted`, `shown`, `broken`, `unplaced`).
A reader trusts it only while `/proc/locks` shows that pid holding the lock, the start time still matches and the
server is the same. A record left behind by SIGKILL, a reused pid or another compositor is therefore never "ready".

States: `Down`, `Starting` (not settled for the current configuration), `Ready` (all enabled cards shown, or none
enabled), and `Degraded` (definition errors, no output, closed by the compositor, or more than 64 cards).

* `host::start()` spawns the host without waiting and without probing. decbar startup and theme apply use it.
* `host::ensure_running()` probes once, waits up to 3 s and fails on `Degraded`. Settings and `decwidgets add` use it.

## Wayland lifecycle

* Cards are rebuilt only when the output layout (name and logical size) changes, not on every `wl_output` update.
* A card the compositor closes is dropped. The cards are rebuilt after 250 ms, 500 ms, 1 s, 2 s and 4 s, then the host
  gives up until the outputs or configuration change.
* Live collectors (and so history) are kept for cards that are configured but have no output at the moment. They stop only
  when no card is enabled.
* Scrolling: one action per wheel event; touchpad scrolling accumulates 50 logical px per action and resets on stop,
  direction change and pointer enter/leave.
* HiDPI: `wp_fractional_scale_v1` through `wp_viewport` when offered, otherwise the integer `wl_surface` buffer scale.
  Every card is drawn at that scale (the old 1x fallback for clocks, rings, graphs and images is gone; see below).

## Scaled painting (widgets and dashboard charts)

The scaled painters passed the focused source gate below; real compositor pixel captures and mixed-scale hardware acceptance remain pending.

* `dash/charts.rs`: `ring`, `sparkline` (stroke and gradient area), `arrow`, `image_rounded`, `image_circle` draw through
  `gfx::transform()` instead of `Transform::identity()`. Paths, stroke widths, gradient points, image patterns and clip
  paths are all logical and transformed together. At render scale 1 the transform is the identity, so X11 and every
  1x caller draw exactly the same pixels as before.
* `widgets/paint.rs`: analog clock strokes (`clock_line`) and images (`draw_image`, cover/contain with a rounded clip) use
  `gfx::transform()`; icons are rasterised at `size x render_scale` (the same cache key at 1x); ASCII full blocks snap
  their edges to whole surface pixels (identical at 1x, no seam at 125 %). `ImageCache` still keeps one decoded source per
  path, whatever the scale.
* `widgets/host_wayland.rs`: `crisp()` is gone; `BufferScale::pick(int_scale, scale120, viewport)` (no `crisp` argument);
  `paint_card(item, scale, w, h, painter)` paints a laid-out card into a buffer of `scale.buffer_size(w, h)`.

## Dashboard (`decdash`) on a scaled Wayland output

`Dash::popup_scale()` is `PopupScale::Applied`. Dash keeps laying out, hit-testing and painting in logical pixels; only
`dash/mod.rs` converts:

* `content_size`/`initial_size`: logical size x scale, rounded (the backend's `to_logical` gives back the 1x card).
* `set_screen`, `resize`: device to logical (stored logical in `dux.screen`, `dux.size`).
* `popup_anchor`: logical anchor x scale (the backend divides it again).
* pointer positions divided by the scale; scroll deltas stay as the backend sends them (logical).
* `draw`: `gfx::with_render_scale(scale, ..)`; `draw.rs`/`dux.rs` read the logical size with `gfx::logical_size(pm)`;
  symbols and Material icons are rasterised at the surface size.
* The scale is applied only after `set_overlay(true)`, which only the Wayland compact path calls. The X11 backend calls
  `set_scale(Xft.dpi / 96)` and never `set_overlay`, so on X11 Dash stays exactly as before (that Xft value was ignored).
* Dux clock face: it is rasterised on a worker and composited without the render transform, so Dash asks for it at
  `(clock size) x scale` and paints it at device coordinates. `clockface.rs` (not changed) still clamps the date line with
  `14 x font_scale`, `12 x font_scale` and `16 x font_scale` device pixels, so at a scale above 1 those minimums are smaller
  in logical terms (only for very small clock cells), and its "Clock font unavailable" fallback line is drawn at 12 device px.

## Settings: readiness on the worker

`settings/widgetspane.rs::save` writes `widgets` with `edit_json_then` (unknown keys kept as before). When an enabled
desktop card exists, the after-save callback queues `host::ensure_running` as a `run_task` ("Desktop widgets") on the
same ordered settings worker, so it runs only after the file is written and never on the UI thread. While it waits the
notice is "Widget choices saved; starting the desktop widgets…". Ready gives the caller's saved message; Degraded, a
host that did not start, or a legacy host give "Widget choices saved, but the desktop could not display them: …" as an
error. A failed write never asks the host. Hidden widgets or no enabled desktop card never start one. Behaviour change:
the readiness wait now always sees the file it was saved with (it used to race the threaded writer).

## Hosts from before per-server locks (upgrade overlap)

An older build held the generic `decwidgets` lock on both servers and, on Wayland only, wrote `$XDG_RUNTIME_DIR/decwidgets.ready`
(its pid) after its first frame.

* X11: unchanged. The current X11 host uses the same lock, so an older X11 host simply keeps it (`classify_x11` reports it
  as Ready without a record, as before). Installed X11 hosts are not touched.
* Wayland: `host::legacy_host(&key)` (pure part `legacy_for(key, &LegacyFacts)`) returns the pid of an older host when the
  pid in `decwidgets.ready` is the live holder of the `decwidgets` flock (`/proc/locks`), that holder is not a current X11
  host (trusted `decwidgets.state` with the same pid and start time and an `x11:` server), and its `/proc/<pid>/environ`
  `WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR` resolve to this compositor's socket. A holder on another socket (nested compositor,
  second session) is ignored. An unreadable environment counts as this compositor.
* Then: `run` does not take the keyed lock (`autostart` logs and returns Ok, `daemon` fails with the message); `start`
  does not spawn (logs); `ensure_running` returns the legacy message; `Probe::state` reports `HostState::Legacy(pid)`
  (new variant, `running()` true) when the keyed lock is free; `decwidgets status` prints it and `--json` has
  `"state":"legacy","legacy_pid":N`.
* Nothing is signalled or killed and no migration is claimed. Handoff still required: the user logs out and back in, or
  ends that process; the next start (bar startup, theme apply, Settings, `decwidgets add`) then starts the keyed host.
* Limits: the guard only covers the older Wayland host (the one that writes `decwidgets.ready`). An older X11 host drawing
  through XWayland of the same compositor is not detected, as with any X11 host next to a Wayland one (one host per
  server is the design). If a keyed host and an older one both already run (started before this guard), the keyed host's
  state is reported and the overlap is not flagged. A current X11 host that has not written its first record yet, holding
  the generic lock with a pid that a stale `decwidgets.ready` also names, would be taken for an older host until it settles.

## Root source validation checkpoint

The combined repair snapshot passed `cargo fmt --check` and 199 focused tests, with 3 ignored and no failures
(`/var/tmp/decebal-wayland-repair-source-gate/result.json`). This includes backend selection, popup geometry/state,
widget painting/lifecycle, dashboard scaling, asynchronous widget creation rollback and readiness-result ordering.
The latter two were repaired after independent source review. Strict linting, executable builds and native
compositor tests remain outstanding; these results do not qualify a live deployment.

### Recovery status follow-up

Fresh source review found a stale Ready announcement while compositor-closed widget surfaces were absent or
being rebuilt. `Announcer::invalidate` now removes that announcement before surface removal/rebuild and clears its
cache, so the same configuration is republished only after replacement frames settle. Failure to invalidate stops
the host rather than preserving a trusted stale record. The focused host suite passed 18 tests, strict all-target
Clippy and a fresh development widget-host build (`/var/tmp/decebal-widget-recovery-source-gate/result.json`).
This repair is included in `e6981ae`; a real compositor closure/recovery fixture is still required.
