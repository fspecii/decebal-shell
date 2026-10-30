# Next desktop-readiness work

Checkpoint 2026-10-30 after production source/layout `1d0d4b9`, QA `2f8fd5d`,
Dux evidence `c372f45` and the
reviewed production rollout. Implemented baselines are separated from remaining
work below; see [current evidence matrix](desktop-readiness-audit.md). Latest full
library gate: 1,604 passed, zero failed, ten ignored (prior gates: 1,591 and
1,597). Tests and deployment evidence are recorded in that matrix.

Remaining product gaps also include actual bar-item editing lists, full Files
layouts and screen-reader accessibility; theme/profile labels alone do not satisfy them.

## 1. Third-party application indicators

**Integration checkpoint:** SNI now renders in horizontal bars, vertical rails and
Windows taskbars, with cached images, typed input, nested/dynamic menus and paged
overflow. Private Host/UI tests and actual OBS checks pass; see
[scope and limits](app-indicators.md). Custom icon paths, overlays, external-host
lifecycle, bounded retries and worker-side icon/theme generation refresh are
implemented. Named icons/tint change without app signals, new GetAll or paint I/O.
Remaining work includes XEmbed,
Wayland activation/global coordinates and wider real-app compatibility.

**User trigger:** start a chat/sync/VPN application that registers an indicator,
then close its normal window. The app should remain reachable from its indicator.

**Evidence:** `src/sni.rs` supplies worker snapshots and typed actions;
`menubar/indicators.rs`, `menubar/vbar.rs` and `dock_app/windows.rs` render and
route input. Actual private native tests exercise all three surfaces and OBS.

**Accepted baseline:** private real D-Bus fixtures and native UI cover two hosts,
items/pixels, typed actions, disabled/check/nested/lazy menus, death/reownership,
malformed bounds, missing bus/reconnect, host theme A→B/tint and stable keys. Actual
OBS works in the private native session; crowded rail paging is exercised.

**Next acceptance:** expand real-app/Wayland activation and coordinate coverage;
define XEmbed scope explicitly. Keep all requests/decoding off paint, typed IDs
instead of shell execution, bounded retries and rail-only dock disabling.

## 2. Timer login recovery — implemented in this batch

**Integration checkpoint:** implemented headless `dectimer autostart`, portable XDG entry and optional non-restarting service; 11 unit and 6 real CLI checks passed. Release rollout and entry installation are tracked in the project checkpoint. The criteria below retain the original requirement and notification tradeoff.

**User trigger:** start a countdown, log out/reboot, log in again and wait. The
saved wall-clock deadline persists but no completion worker is started until
Timer opens.

**Evidence:** `dectimer autostart` and `contrib/decebal-timer.desktop` now exist;
packaging and the project checkpoint record the login entry. Store locking,
at-most-once claims and atomic-binary-upgrade recovery are implemented. This
supersedes the earlier “no login invocation” finding. See `docs/notes-timer.md`.

**Remaining integration:** verify clean-login/upgrade sessions on real hardware;
retain the explicit crash-after-claim notification-loss tradeoff. No resident
worker is needed for idle/paused/claimed state, and recovery must never open UI.

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
- Notification history source/disk binaries now expose durable private storage
  failures and Retry, with native recovery evidence. The old live daemon was
  intentionally retained to avoid losing unknown session-only state; its safe
  handoff remains pending. Do not describe that old process as private/durable
  merely because new files were installed.
- Follow-theme Files defaults are implemented, including palette-independent
  selection and dark native evidence. Full Explorer/Finder layouts remain open.
- Curated photo following plus post-photo Dynamic refresh is implemented and
  production-tested (old producer compatibility included). Continue provider,
  provenance and race coverage rather than treating a cached success as network
  availability or a universal non-AI guarantee.
- Dux Settings styling, resource/focus cards and the Specialist's Dux Lavender
  package are integrated, release-tested and installed without applying a theme
  or replacing the user's widget layout. New widget host2869626 is active;
  open Settings keeps its old image/drafts until the user reopens it. Four-card
  native painter/input checks pass at1280×720/200%; this is not a full Dux runtime.
- Notification colours now follow cached live palettes in source/on disk; exact
  release theme/history checks pass. Legacy live daemon670409 remains retained.
  Full enlarged-text notification reflow and live icon-pack cache invalidation
  remain open, alongside safe durable-history runtime handoff.
- NextKDE magnification metadata/runtime repair is complete. rdwm `9788af9`
  workarea-only float fit is accepted (504 core/two ignored; release panel7,
  fullscreen11, snap10), installed in both user WM bindirs for next login.
  Ordinary free floats adapt only to a new reserved area; explicit resize/drag
  clears the limit and fullscreen is excluded. Running WM1714917 remains old,
  so general overlap protection is not active yet. No WM restart was used.
- Post-photo colour refresh rereads the latest selection with bounded retries;
  this is not a cross-process compare-and-swap. Stronger synchronization against
  arbitrary external edits after the final reread remains a separate limit.
