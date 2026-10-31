# Launcher toggle lifecycle

Windows Start invokes `launch::spawn_argv(declaunch)`. The shared launch monitor
reports any unsuccessful exit within eight seconds. Previously `single::acquire`
sent SIGTERM to close the existing launcher, so rapid intentional toggles were
reported as startup crashes. Spotlight had the same singleton behavior.

Normal no-argument `declaunch` and `decspot` now acquire through
`single::acquire_clean_toggle` before compatibility/theme workers start. It
installs a dedicated SIGUSR1 disposition before publishing the lock. A second
invocation sends that signal; its async-signal-safe `_exit(0)` releases the
stateless launcher's OS windows, grabs and lock with a successful status.
SIGTERM and genuine crashes retain their ordinary status and remain reportable.
Argument modes, including Spotlight's clipboard helper, retain ordinary signal
handling. The close disposition resets at exec and the reserved signal is
unblocked, so applications launched from the grid inherit no special behavior.

This helper is for ephemeral launchers, not editors with unsaved drafts. It
is not a general change to `single::acquire` or the launch monitor.

An already-running old launcher does not implement the new SIGUSR1 protocol.
Close any such old holder before testing the newly installed binary; migration
of a still-open old holder can produce one final signal exit. Subsequent opens
use the new protocol. Other pre-rename applications keep their existing lifecycle.

## Native regression

Build the dev binaries and monitored launch helper, then run:

```
cargo build --offline -j2 --bin declaunch --bin decspot --example launchapp
LAUNCHER_TOGGLE_EVIDENCE=/var/tmp/launcher-toggle-final python3 tests/e2e/launcher-toggle.py
```

The fixture owns its private HOME/XDG paths and Xvfb. It exercises eight closes
immediately after lock publication and three visibly mapped closes per launcher,
including an inherited blocked-USR1 startup. It checks zero false early-exit
reports, successful close status, a real grid-launched child's signal mask and
exec disposition, and continued reporting of a genuine SIGTERM failure.

`LAUNCHER_BIN_DIR` selects an older binary directory and
`EXPECT_TOGGLE_FAILURE=1` requires the old regression to occur. Evidence records
exact binary hashes. Initial baseline produced sixteen false monitored errors
and mapped exit status -15; fixed dev produced zero false errors and status 0.
No live desktop processes are changed by the fixture.


## Concurrent request ordering

A separate `<name>.request.lock` now serializes launcher invocations; it is
released before entering the UI. A closing request retains it until the old
singleton lock is released (bounded at two seconds). This prevents ordinary
SIGUSR1 close signals from coalescing and losing toggle parity under load.
Other singleton applications and genuine SIGTERM reporting are unchanged.

`tests/e2e/launcher-concurrent-toggle.py` uses only owned processes and Xvfb.
An intentionally paused holder makes the old bug deterministic: two overlapping
close requests return before the holder exits, so three toggles leave no window.
With serialization, three leave one holder and a fourth closes it. Concurrent
seven/eight-request bursts also preserve parity. Existing mapped-window and
launch-monitor tests retain their independent failure-reporting coverage.
