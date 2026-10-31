# Workspace HUD watcher lifetime

`dechud watch` holds `$XDG_RUNTIME_DIR/dechud-watch.lock` before connecting to
rdwm, including while rdwm is absent or reconnecting. Another invocation exits
successfully without signalling or replacing the holder. The compatibility
check also recognizes a held `machud-watch.lock`.

The watcher lock is separate from `dechud.lock`, which controls the short-lived
volume/brightness/workspace card and its existing SIGUSR1 updates. Child cards
do not inherit the watch lock across exec. The lock releases when the watcher
exits, so a supervisor can start one replacement.

Older watcher versions had no watch lock. Installing a new binary does not stop
those old processes; an upgrade must identify and stop only the session's old
watcher processes before starting one new watcher. Do not kill arbitrary HUD
cards or rely on an unqualified process-name kill.

Private regression (no live display, configuration, audio or IPC):

```sh
HUD_BIN=target/debug/dechud python3 tests/e2e/hud-watch-singleton.py
EXPECT_DUPLICATES=1 HUD_BIN=/path/to/old/dechud python3 tests/e2e/hud-watch-singleton.py
```

The fixture checks concurrent startup without a WM, one subscription, duplicate
exit status, one emission per event, card reuse/expiry, WM reconnection and lock
release/replacement. The old binary demonstrates duplicate watchers. All paths,
Xvfb processes and the fake IPC socket are private under `/var/tmp`.
