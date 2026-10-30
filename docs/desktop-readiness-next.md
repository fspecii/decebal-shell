# Next desktop-readiness work

Source review, 2026-10-30. These are proposed implementation assignments, not
completed features. See [current evidence matrix](desktop-readiness-audit.md).
No tests or live changes were performed for this report.

## 1. Third-party application indicators

**Backend checkpoint:** `src/sni.rs` now provides a dormant worker foundation.
`bash tests/e2e/sni-backend.sh` passed real Host tests against two private items,
owned/external watchers, signals, pixels, menus, actions and reownership. It is
not started by the bar or dock yet. Visible layouts/input, real-app verification,
custom icon paths, overlays, external-host unregistration and initial-property
retries remain. The original UI acceptance criteria below still apply.

**User trigger:** start a chat/sync/VPN application that registers an indicator,
then close its normal window. The current built-in status “tray” provides no
application-specific reopen/menu affordance.

**Evidence:** `src/menubar/mod.rs::Tray` is a fixed enum of shell controls;
`src/taskbar/mod.rs` still lays out that fixed list. The SNI backend is not wired
into either surface; no XEmbed manager exists. Existing D-Bus patterns live in
`src/dbus.rs` and `src/notify/mod.rs`.

**Implementation:** add a session-bus StatusNotifier watcher/host with bounded
icon/pixmap decoding and ownership/lifecycle handling, then expose its items in
bar/taskbar layouts. Keep bus/property requests outside painting. Implement
activate, context menu and scroll via protocol calls; safe menus must use item
IDs/typed actions rather than shell strings. Define explicit XEmbed scope; do not
call SNI support universal legacy tray support. Preserve rail-only dock disabling.

**Acceptance:** a private D-Bus fixture registers/unregisters two items, changes
icons/tooltips/status, dies and returns, and owns unique names. Real rendered
icons and clicks reach the correct fixture. Malformed/oversize pixmaps are
bounded, repeated restarts leave no duplicate items, and missing session bus
leaves built-in controls usable. Test both horizontal taskbar and vertical rail.
Then verify at least one real application; a mock protocol pass alone is not full
compatibility evidence.

## 2. Timer login recovery — implemented in this batch

**Integration checkpoint:** implemented headless `dectimer autostart`, portable XDG entry and optional non-restarting service; 11 unit and 6 real CLI checks passed. Release rollout and entry installation are tracked in the project checkpoint. The criteria below retain the original requirement and notification tradeoff.

**User trigger:** start a countdown, log out/reboot, log in again and wait. The
saved wall-clock deadline persists but no completion worker is started until
Timer opens.

**Evidence:** `src/bin/dectimer.rs` spawns its worker when normal UI startup finds
Running/Complete state. `src/timer.rs::{Store::worker,Store::spawn_worker}` enforce
single-worker ownership and at-most-once completion claims. Packaging supplies a
normal desktop entry only; no Timer login invocation found in `contrib`, Makefile
or rdwm session supervisor. This limitation is explicitly documented in
`docs/notes-timer.md`.

**Implementation:** add a headless idempotent recovery command and a packaged
login hook/service that exits immediately for idle/paused/claimed state. Reuse the
existing state lock, delivery claim and error recording. Do not run a permanent
service when no countdown is active. Avoid duplicate worker ownership between UI,
login and an existing active helper. Keep the no-UI CLI distinct from `--new` note
or normal timer launch.

**Acceptance:** isolated XDG state tests simulate a future active deadline, an
already elapsed unclaimed deadline, paused state, corrupt state and an existing
worker. Recovery starts no window, produces at most one completion across repeated
login calls and concurrent UI opens, records missing notifier failures, exits when
finished, and consumes no resident worker for idle/paused state. Native private
session test kills only its own worker to model logout and proves completion on
next recovery without launching Timer. Explicitly retain the known crash-after-
claim notification-loss tradeoff or change that contract deliberately.

## 3. Native rdwm Wayland output controls with timed rollback

**User trigger:** open Displays in a Wayland session to arrange a monitor or
change resolution/scale. Current UI returns “displays are arranged by the
compositor”; it cannot perform the basic desktop task.

**Evidence:** `src/settings/system/display.rs::load_env` returns unavailable when
Wayland is true. X11 already has output models, plans and a timed Keep/revert flow.
rdwm configuration supports `[[output]]` through `crates/rdwm-core/src/wlcfg.rs`;
Wayland output state exists in `crates/rdwm-wayland/src/state.rs`. XDG output
advertisement is not a writable output-management protocol.

**Implementation:** first define a typed backend capability/query/apply interface
with mode, position, scale, transform and enable constraints. Use rdwm's actual
Wayland configuration/IPC path, not xrandr against Xwayland and not an assumed
wlr-output-management implementation. Reuse bounded plan confirmation and
rollback. Keep unsupported compositors explicit, preserve unknown output/config
fields, and do not allow an unconfirmed last-output disable.

**Acceptance:** private nested compositor tests query actual output properties,
apply/revert a supported change, cancel or time out to the exact prior state,
reject invalid scales/modes, and preserve other output fields. Cover hot-unplug
while confirmation is open and denied/failed apply. A mixed-DPI physical two-output
session remains a separate required manual/hardware gate; nested pixel evidence
cannot substitute for it.

## Other explicit requirements remain open

- Explorer/Finder reproduction is still only Nautilus geometry styling. A real
  familiar navigation/layout experience needs an explicit file-manager design
  and functional file-operation acceptance tests; more CSS is not equivalent.
- Native screen-reader support, complete keyboard-only flow and complex-widget
  200% layout checks remain. External app accessibility is a separate toolkit duty.
- Wayland widget surfaces, CSD/Wayland shadows, activation/capture/portal paths and
  Qt app pixel verification remain distinct work.
- Real NVIDIA frame timing and mixed-DPI/carrier memory checks are pending after
  a new WM session. Never restart the user's live WM as a test shortcut.
- Installed system session-wrapper migration needs administrator authentication;
  use the already prepared reviewable bundle rather than pretending user-binary
  replacement updated `/usr/bin`.
- Notification-history persistence errors are currently swallowed in
  `src/notify/model.rs::History::save`; review private permissions and expose a
  recoverable persistence failure instead of silently claiming durable history.
