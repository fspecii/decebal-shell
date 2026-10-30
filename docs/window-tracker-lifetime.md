# Window tracker lifetime

`wm::Tracker` clones are public owners. Window collectors now use a separate
state-sharing handle which does **not** own their lifetime. Dropping the last
public clone closes a wake socket: idle X11 and Wayland collectors wake and exit,
releasing their connection, callback and shared controller. Keeping a clone alive
preserves title/window updates and activation/workspace controls.

This closes the remaining `window.*` widget-data leak after the MPRIS repair:
changing the widget data groups or disabling widgets used to drop the public
tracker while its collector retained itself forever. Normal idle collection now
polls the protocol socket together with the lifetime socket, with no periodic
wake timer. Wayland socket backpressure uses the existing bounded100ms writable
wait and checks lifetime again afterward; ordinary idle collection stays blocked.
Initial protocol connection/registry setup and an already in-flight X11 reply
retain their existing synchronous behavior. This does not add automatic tracker
reconnection after the display server disconnects.

X11 property draining also handles `_NET_WM_NAME`, legacy `WM_NAME`, and
`WM_CLASS` updates. Empty/missing UTF-8 titles fall back to `WM_NAME` STRING.

## Regressions

`cargo test --offline --lib wm::` covers the owner/collector split with two public
clones. `cargo test --offline --test wm_tracker_lifetime -- --nocapture` starts its
own Xvfb and uses private HOME, all XDG directories and unreachable session/system
buses. Eight create/update/activate/drop cycles return threads and FDs to baseline;
the surviving clone receives both UTF-8 and legacy title changes and sends real
EWMH activation requests. If Sway is installed, it also starts a private headless
pixman compositor and checks four Wayland idle clone/drop cycles. That proves
Wayland collector teardown, not hardware behavior or toplevel-control fidelity.
No live compositor, pointer, app, config, or installed binary is used.

The media-free widget integration reproducer is:

```
WIDGETS_BIN=/absolute/decwidgets EVIDENCE=/tmp/fresh-dir \
  python3 tests/e2e/widget-window-tracker-lifetime.py
```

`EXPECT_LEAK=1` records an unfixed binary. Four enable/disable cycles on baseline
SHA `0c336bca55d0108a9b8951e49328476dca1bb8ac9f77bba504d3b4398f2902ab`
grow2→6threads and8→12FDs. The repaired debug host returned2→2threads and8→8FDs;
no MPRIS card or media worker is involved. Evidence: `/tmp/window-tracker-before`
and `/tmp/window-tracker-after` with exact binary hashes. Final direct native test
log: `/tmp/decebal-tracker-native-direct.log`; explicit lease unit log:
`/tmp/decebal-tracker-lease.log`. Exact final native test executable:
`/tmp/wm-tracker-native-final` (compiled directly against the current default-feature
debug rlib to avoid repeating unrelated all-bin debug links).
