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

Private native regression: `MS_BIN_DIR=target/debug bash tests/e2e/notify-theme.sh`.
Set `NOTIFY_THEME_EVIDENCE_DIR` to retain screenshots and `E2E_DISPLAY` to choose
an unused private X display. No build, installation or live session is performed.
