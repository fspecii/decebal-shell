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
