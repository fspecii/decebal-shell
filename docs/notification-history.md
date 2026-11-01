# Notification history recovery

Open history with `decnotify history` (toggle) or `decnotify history show`.
Notification delivery and transient banners continue when history storage fails.
The panel reports the failure and keeps pending changes in memory for the current
session. A failed Clear shows **Clear is not saved**: the durable file has not
been cleared. Fix the storage problem, then click **Retry**.

```sh
decnotify history status          # human-readable persistence state
decnotify history status --json   # error, pending, clear_pending; no notification content
decnotify history retry           # retry loading/merging/saving after repair
decnotify clear                   # failure exits nonzero when Clear cannot be saved
```

`decnotify status` and `decnotify retry` are short aliases. Status exits 1 while
storage is unavailable. Retry preserves corrupt, symlinked, unreadable or unsafe
history rather than replacing it; repair that condition first. Unsaved changes
are lost if the daemon/session closes before a successful retry.

History is bounded to 100 records and 1 MiB, written atomically inside the private
state directory. Original notification content is excluded from storage errors.
`DECNOTIFY_DEBUG_HISTORY` is an opt-in native-test hook containing synthetic
history summaries and UI rectangles; leave it unset in normal sessions.

## Theme coherence

Native banners and history use the resolved theme's popup, accent and text
colours, including independently selected palettes and stored wallpaper colours.
The existing theme watcher wakes the notification event loop when these change;
open windows repaint without replacing notifications, resetting expiry times,
changing DND or saving history. Rendering reads the cached theme and never builds
a theme, decodes a wallpaper or runs a command. Surfaces are opaque so text
contrast does not depend on the image underneath; small text is adjusted toward
a readable colour when needed. The high-contrast accessibility override applies.
Notifications are static, including when reduced motion is enabled.

This change covers colours. Existing fixed row metrics do not fully reflow for
200% accessibility text, and cached application icons do not reload on an icon
pack change. These remain separate layout/icon integration work.

## Private native regressions (X11)

`tests/e2e/notify-history.sh` (persistence failures, Retry, merge, restart, with
a private `rdwm`) and `tests/e2e/notify-theme.sh` (theme and icon repaint) run the
real `decnotify` on resources owned by `tests/e2e/notify_private.py`. Nothing is
built, installed or defaulted: the caller supplies already built, unchanging
binaries (a changed binary fails the run).

```
MS_BIN_DIR=/abs/bin RDWM_BIN_DIR=/abs/rdwm-bin bash tests/e2e/notify-history.sh
MS_BIN_DIR=/abs/bin bash tests/e2e/notify-theme.sh
```

`MS_BIN_DIR` (decnotify, and decthemectl for the theme run) and `RDWM_BIN_DIR`
(rdwm) are required absolute paths. Optional: `NOTIFY_HISTORY_EVIDENCE_DIR` /
`NOTIFY_THEME_EVIDENCE_DIR` name a **new** directory (an existing one is refused)
for screenshots, child logs, debug JSON and `cleanup.json`; `KEEP_TMP=1` keeps
the private tree of a theme pass. `E2E_DISPLAY` is optional and must be inside the
private display namespace `:700`..`:65535`; the old default displays (`:348`,
`:367`) are no longer accepted. Without it a free display is allocated. An
occupied explicit display is never adopted or removed (history exits 77, theme
fails).

Isolation: the environment is built from scratch (private HOME/XDG tree, dead
system bus, `DECEBAL_SYSTEM_AUTOMATIC=0`, no session, backend, Wayland or
`RDWM_*` variables). The Xvfb is started by `tests/e2e/private_x11.py` and its
listening sockets are re-proven to belong to that exact process before every
`xdotool`/`xprop`/`import` call. The session bus uses a fixed configuration
without service activation and its socket is checked with kernel peer
credentials. Every child is owned through a pidfd.

Cleanup: SIGTERM/SIGINT/SIGHUP stop the test immediately and run cleanup (exit
`128+N`). All children get SIGTERM together with a bounded total grace; any that
need SIGKILL, leave process-group members behind, or cannot be reaped fail the
run. Failed, interrupted or force-cleaned runs keep the private tree and the
fresh evidence; a clean pass removes the tree. Prerequisites that are missing
(tools, binaries, non-root uid for the permission-denied scenarios) exit 77 for
history and fail for theme. Fixture unit checks (no processes started):
`python3 tests/e2e/notify-private-test.py`.
