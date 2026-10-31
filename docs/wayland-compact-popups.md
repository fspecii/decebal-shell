# Wayland compact popups (HUD, dashboard, process monitor card)

Status: **development source and headless-Sway protocol/input checks passed; hardware and release acceptance remain pending.** The combined repair snapshot
passed formatting and 199 focused tests (3 ignored), including 27 popup backend tests, under the serialized
resource guard. Evidence: `/var/tmp/decebal-wayland-repair-source-gate/result.json` and its per-phase logs.
Strict all-target Clippy and four development executable builds subsequently passed. A private headless-Sway
protocol fixture passed for the immutable HUD/dashboard binaries, with no forced cleanup or rescued descendants:
`/var/tmp/decebal-wayland-popup-native-evidence-v3/qualification.json`. This is not rendered-pixel, input hand-back,
hardware or release-build acceptance; remaining checks are under [Proof still owed](#proof-still-owed).

Applies to apps whose `WindowApp::wants_compact_popup()` is true (an overlay that is a passive indicator, hangs from a
bar, or sits above the bottom edge): `dechud`, `decdash`, `decmon --popup`. Full-screen overlays (Spotlight, the
switcher, Exposé) and normal xdg windows keep their earlier paths unchanged. Code: `src/backend/wayland_window.rs`.

## Surfaces

| Surface | Kind | Size | Input | Keyboard |
| --- | --- | --- | --- | --- |
| host | `zwlr_layer_surface_v1`, layer overlay, anchored to all edges, exclusive zone -1 | the output (logical, from configure) | whole output (interactive) / empty (passive) | exclusive (interactive) / none (passive) |
| card | desynchronised `wl_subsurface` of the host, placed with `set_position` | the card (logical) | whole card (interactive) / empty (passive) | — |
| catcher | layer overlay on every *other* output, namespace `decebal-popup-outside` | that output | whole output | none |

The host and catchers are transparent: one 1 x 1 buffer stretched by `wp_viewport`, or without a viewporter the
smallest buffer an integer buffer scale stretches exactly (`w/g` x `h/g` at scale `g = gcd(w, h)`). They are
committed once per size; no full-output buffer is drawn, cleared or copied per frame.

## Frames

* At most 3 card-sized buffers. The next frame goes into a released one, else a new one while fewer than 3 exist,
  else it waits 8 ms for a release (backpressure; never a fourth allocation).
* A frame is drawn only once a free buffer exists, then diffed against the last presented frame. Only the changed
  bounding box is copied (each buffer also catches up on frames it missed) and damaged with `damage_buffer`; an
  unchanged frame is not committed at all.
* Each card commit requests `wl_surface.frame`; the next frame waits for the callback (or 1 s if a compositor drops
  it). Exit (`Action::Close`, the HUD timeout) never waits for a frame.
* A card that grows or shrinks drops its buffers; a pool larger than 6 frames of the new size is replaced.

## Scale

* `WindowApp::popup_scale()` (new, default `PopupScale::Logical`): a logical app is drawn at 1x and the compositor
  stretches it (right size, soft on HiDPI). `PopupScale::Applied` apps (the HUD and dashboard) get `set_scale(k)` with the output's
  real scale and are presented sharp: `wp_fractional_scale_v1` + `wp_viewport` for a fractional `k` (buffer =
  `round(logical * k)`, buffer scale 1), else `set_buffer_scale(n)` for the integer scale (buffer = `logical * n`).
* The scale comes from the host surface only (preferred fractional scale, else `preferred_buffer_scale` / the output
  scale via sctk). `backend::ui_scale()` and `Xft.dpi` are no longer used for Wayland cards, so an XWayland
  `Xft.dpi` cannot double the compositor's scale. `DECEBAL_SCALE` does not apply to Wayland cards.
* `set_screen` is the host's configure size, i.e. the output's logical size (times `k` for `Applied` apps), never a
  physical mode. Top-level windows now get the logical size of the output they entered (xdg-output, else the
  current mode turned by the transform and divided by the integer scale).
* The card's height is clamped to the room the placement leaves: `output - top - 8` under (or above) a bar,
  `output - bottom margin - 8` above the bottom edge, and the width beside a side bar likewise.

## Output and keyboard

* `WindowApp::popup_monitor()` now matters on Wayland: an output name (`decdash --output DP-2`, new), else the output
  whose logical rectangle (xdg-output) holds `point`, else no output is requested and the compositor places the
  host (its focused output); the backend then learns the real output from `wl_surface.enter`. Before, every output
  announced overwrote the app's screen with its physical mode, so the last enumerated output won.
* An interactive card holds the keyboard (exclusive) while open. Every way out is honest:
  * a click beside the card, on the bar, or on any other output closes it (the click is consumed, as with the X11
    pointer grab), and with it the exclusive keyboard;
  * `wl_keyboard.leave` after a real `enter` closes it (`popup_closes_on_focus_loss`);
  * `input_grabbed(Some(mask))` is reported only after the compositor sent `enter` (with the modifiers mapped to the X
    mask); without an `enter` within 750 ms of the first frame it reports `input_grabbed(None)` and logs a warning.
    Escape then cannot work, outside clicks still close the card. It is never assumed.
* A card whose app returns `popup_closes_on_focus_loss() == false` (none of the current compact apps) gets no
  catchers and keeps the keyboard until it closes itself, by design.
* Passive cards (the HUD) take no input and never the keyboard.

## Backend choice

`window_app::run_window` uses `backend::session::decide` (the lazy form of `choose_mode`; owned elsewhere). The
compositor is probed at most once (`wayland_window::probe()`, one registry roundtrip) and only when the policy reaches
Wayland, so `DECEBAL_BACKEND=x11` and X11 sessions with a leaked `WAYLAND_DISPLAY` never connect to a
compositor. A compact card needs `zwlr_layer_shell_v1` + `wl_subcompositor` (and no longer xdg-shell); anything else
needs `xdg_wm_base`. `wp_viewporter` and `wp_fractional_scale_manager_v1` are optional.

## Known gaps (protocol or compositor limits)

* No compositor-side pointer grab exists for a layer surface without an input serial from this client (a hotkey or
  another process launched it), so `xdg_popup` grabs are not usable; outside clicks are caught by transparent
  surfaces instead. Stacking of the catchers relative to other overlay-layer clients (lock screens, other overlays)
  is compositor policy.
* Exclusive keyboard focus is what the protocol offers for an Escape-closable card; a compositor that refuses it (for
  example while another exclusive client is up) is reported as `input_grabbed(None)`.
* The dashboard now opts into `PopupScale::Applied`, converts its boundary coordinates to logical units, and paints
  through `gfx::with_render_scale`. Focused tests and development builds passed; rendered-pixel native scaling
  acceptance remains outstanding. Small clock cells still have
  device-pixel minimum text sizes in `clockface.rs`; fractional-scale and 2x native captures remain required.
* `point` hints need xdg-output logical geometry; without it only output names match.
* Full-screen overlays still use one output-sized buffer per frame and an exclusive keyboard without catchers
  (unchanged on purpose; outside this change).
* `decbar` / the vertical bar do not pass `--output` to `decdash` yet (not owned here); without it the compositor's
  focused output is used, which is where the click came from in the usual single-seat case.

## Proof still owed

The root owner ran `tests/e2e/wayland_compact_popup.py` on private headless Sway after isolating D-Bus/XDG,
copying the tested binaries, supplying fake audio data, disabling Xwayland and checking server ownership with
kernel socket peer credentials. Three ownership helper tests pass. The protocol run passed, with logs and exact
binary/harness hashes in `/var/tmp/decebal-wayland-popup-native-evidence-v3/qualification.json`. The two earlier
attempts failed at fixture ownership verification before either app ran; neither qualifies the product.

### Extended fixture and virtual-pointer repair

The first `--input` run at `/var/tmp/decebal-wayland-popup-input-native-evidence` **passed Escape**: actual keyboard
enter, blocked key not leaked, normal popup exit, and marker `returned0` reaching the same native recorder without
refocusing. It then timed out on the same-output outside click. The private seat advertised only keyboard
capability, so the dashboard never bound `wl_pointer`; IPC cursor commands alone did not create a device. This is
an incomplete/failed input matrix, not accepted pointer behaviour.

The source now creates a private virtual pointer before starting the recorder and popups. The repaired fixture
passed all three native input cases on headless Sway: Escape, same-output outside click and other-output catcher
click, with blocked-key isolation and keyboard return to the same native recorder without refocusing. Evidence:
`/var/tmp/decebal-wayland-popup-input-native-evidence-v2/result.json`; controller
`/var/tmp/decebal-wayland-popup-input-native-gate-v2/result.json`. Exact binary/harness hashes are in that result.
No forced cleanup, rescued processes or resource abort occurred. The original failed attempt remains recorded.
Run future attempts only inside the serialized resource guard, for example:

```sh
python3 tests/e2e/wayland_compact_popup.py --bin-dir /var/tmp/decebal-wayland-repair-artifacts-2432669-wip --out /var/tmp/new-popup-input-evidence --keep --input
```

`--input` requires installed `foot`, `wtype` and `swaymsg`. It creates a native terminal in the private compositor
whose only child is a raw-PTY Python key recorder, pinned to HEADLESS-1. A long-lived virtual keyboard keeps the
seat's keyboard capability present between short `wtype` sends. The fixture proves that an initial marker reaches
the recorder, opens the dashboard on HEADLESS-2, waits for a real `wl_keyboard.enter`, and checks that a typed key
cannot leak to the recorder. Three cases then close the popup: Escape, a same-output click outside the card, and a
click on the other output's catcher. Every case requires normal exit and a new marker reaching the same recorder
without a focus command after the close. Every input action first checks both private sockets' kernel peer PID/UID.
The pointer holder is a small in-process Wayland wire client using only the registry, `wl_seat` and version-1
`zwlr_virtual_pointer_manager_v1.create_virtual_pointer` requests from the locally vendored
`wlr-virtual-pointer-unstable-v1.xml`. It verifies the actual connection's kernel peer PID/UID, waits for the seat's
pointer capability bit, and holds the connection until cleanup. Closing that connection removes its virtual device;
there is no uinput, physical device, installed daemon or separate helper process. `virtual-pointer.json` records the
advertised capability, and pointer cases require the popup's `wl_seat.get_pointer` request before injection.
The pointer actions still use Sway's deprecated seat cursor IPC commands, scoped to the owned compositor; this is
not a physical touchpad or mouse test. Missing virtual-pointer support or unsupported IPC commands fail instead of
silently skipping pointer cases. Native delivery passed on private headless Sway; physical devices and other compositor policies remain unproved.

The source also strengthens protocol proofs: `wl_output.name` events must identify HEADLESS-2 for the dashboard
host and HEADLESS-1 for its catcher. Card buffer commits need a frame request on the same subsurface, reuse must
follow `wl_buffer.release`, and live same-size allocations must stay at three or fewer. Requests before an earlier
callback finishes fail unless the documented one-second fallback elapsed. Nine helper tests pass (three ownership
checks, three protocol tests with deliberate broken traces, and three wire/string/virtual-pointer handshake tests).
The handshake tests use a local Unix socket peer in a thread, not a compositor or desktop, and check advertised
pointer capability plus failure when the manager protocol is missing. Applying the stronger parsers to the *earlier saved*
protocol logs found HUD 1 / dashboard 7 card commits and completed callbacks, at most 2 live same-size buffers,
and exact expected output names. That is a retrospective parser check, **not execution of the new input fixture**.

The result records binary/harness hashes, protocol metrics, input-case records and cleanup state. Partial input-case
records are retained in `input-results.json` on failure. Rendered pixels, input timing/frame rate, hardware, other
compositors, lock-screen stacking and full popup pointer-grab semantics remain unproved.

1. `cargo test --lib backend::wayland_window` (pure geometry/state tests in `src/backend/wayland_window/tests.rs`).
2. Two outputs at scale 1.5 and 1: `dechud` uses a 360 x 222 card buffer behind a 240 x 148 viewport on the 1.5
   output; no `wl_shm_pool.create_buffer` is output-sized; frames request callbacks.
3. **Passed on headless Sway:** `decdash --output HEADLESS-2` maps there with one catcher on HEADLESS-1;
   Escape and both outside-click cases exit normally and return typing to the previously focused native client.
4. A busy card (dashboard performance tab) never has more than 3 card buffers alive and commits small damage.
