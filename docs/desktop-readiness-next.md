# Next desktop-readiness work

Current priorities after the accepted tracker/sound/popup batch and pending native
Dux integration:

- Finish independent review and exact optimized native acceptance for the Dux
  bar, README-profile dashboard, Control Center and Applications launcher. Root
  source through `b6b49ef` passes 1,662 library tests and 80 actual-font painter
  checks; source integration is not a live deployment. Package folder/archive
  validation passes. A full panel matrix had one startup-probe timeout near its
  last mainbar case; isolated mainbar and dedicated Control Center/launcher gates
  pass. The bounded diagnostic full rerun passed all36captures; retain the earlier
  intermittent observation and startup diagnostics rather than inventing a cause.
- Continue the actual source-backed replica section by section: the native owner
  now has the original variable-clock typography task; the sole theme specialist
  owns its future licensed asset and selector update. Expressive artwork/motion,
  richer sheets and remaining launcher providers are still missing. Preserve the
  accepted Settings structure; do not label the complete replica finished.
- CPU worker defects are repaired and deployed: MPRIS EOF spin and window-group
  Tracker lifetime. Exact optimized private media/glass and window-group lifecycle
  checks pass; the observed old ~400% daemon was replaced. This does not establish
  universal power/FPS/hardware performance. See [current evidence](desktop-readiness-audit.md).
- Hardware-test configurable three/four-finger gestures on a supported touchpad.
  The new WM is already running after guarded X11 reload; next login is no longer
  the activation blocker. XI2.4 best-effort delivery and private injected tests do
  not prove physical device support.
- Finish delivery of the source-integrated newcomer improvements: Getting Started
  consumes cached actual effective shortcuts, with unknown/custom/disconnected
  states; notification sounds now have accurate search/help/DND wording. Native
  Welcome fixtures cover 100/200% normal/short and disconnected states, with the
  documented one-button root correction still needing final production checks.

Remaining product gaps include true Explorer/Finder file-manager workflows,
AT-SPI/screen-reader operation, portals/capture and full Wayland widgets/shadows,
physical output/audio/network/input acceptance, and a complete new-user/upgrade
release matrix. Theme/profile labels alone do not satisfy those requirements.

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

## 3. Wayland Displays — implemented and privately validated

The native adapter and staged Settings workflow now implement typed rdwm
capability/query/apply/confirm/revert. Keep persists through the compositor;
its timer safely reverts if Settings closes or crashes. Instance/generation
and reply-order guards protect drafts and confirmation state. Unsupported
compositors stay explicit; forcing X11 rendering never selects xrandr against
actual Wayland. The raw output table no longer bypasses the transaction.

Exact release native nested tests exercised actual pixels/input, per-head caps,
Apply/Keep/Revert, Settings SIGKILL timeout, stale reset, persisted compositor
restart and X11/forced-X11 regression. Settings was installed to both user bindirs
without output/configuration changes or any app/compositor restart. The user's
current X11 session is unchanged; matching rdwm protocol v1 is required for the
new controls in Wayland. See [scope/contract](wayland-displays.md).

**Remaining acceptance:** physical DRM modes, disable/enable lifecycle, hot-unplug
and mixed-DPI multi-output hardware checks. Nested mode/power are host-owned and
correctly disabled; nested pixel evidence cannot establish physical driver safety.
Other compositors require explicit adapters rather than protocol assumptions.

Claude's structural Settings UI redesign is integrated, release-tested and installed;
its Dux theme fidelity follow-up is separate from functional output management.

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
  the then-open Settings window was preserved and later closed by the user. Four-card
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
