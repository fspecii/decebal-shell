# Private Wayland widget recovery gate

Status: **development-binary native output/configuration recovery passed**; retained-output server-close/retry,
rendered pixels and hardware remain unproved. Five watcher helper tests pass. Root evidence is recorded below.

Run `tests/e2e/wayland_widgets_recovery.py` only through the serialized desktop resource guard. It requires
Sway and swaymsg, a Linux inotify interface, and an immutable `decwidgets` binary. It copies the binary into
a new evidence directory, starts one owned headless/pixman Sway output with Xwayland disabled, and uses private
HOME/XDG paths, dead session/system D-Bus addresses and absent audio/rdwm endpoints. Every IPC/status operation
checks kernel peer credentials of both private compositor sockets. Cleanup uses owned child pidfds; forced
cleanup or token-owned descendant rescue fails the gate. It never signals a named process or uses physical input.

Example arguments for the root-owned guarded runner:

```sh
python3 tests/e2e/wayland_widgets_recovery.py \
  --bin /var/tmp/decebal-widget-recovery-artifacts-e6981ae/decwidgets \
  --out /var/tmp/new-wayland-widget-recovery-evidence
```

The gate places the real built-in `tty-clock` and requires CLI JSON status `ready`, with one wanted/shown card.
It disables and re-enables the sole private output twice, requiring Degraded with one unplaced card and then
Ready with one shown card, while the same widget-host process remains alive. It then checks a missing definition
(Degraded/broken), zero enabled cards (Ready/zero) and restoration of the valid card. Thread counts are recorded
for context; this is not an MPRIS leak or overall CPU test.

## Deterministic readiness invalidation evidence

The fixture watches only the private host's readiness filename using inotify. Each output change must delete the
old record before the next atomic publication. This proves the previous tally was invalidated during rebuilding
without relying on a polling race to catch a short Starting state. An atomic overwrite alone does not count as
invalidation. Queue overflow and incomplete event records fail closed. Five helper tests cover real filesystem
delete/republish versus overwrite, filename filtering, ordering and malformed/overflow evidence.

The pre-invalidation negative binary `/var/tmp/decebal-wayland-repair-artifacts-2432669-wip/decwidgets` is expected
to fail the deletion-before-publication assertion; root must actually run that negative control before claiming
the native gate detects this regression. No negative run has been performed by the fixture author.

## What remains unproved

Output disappearance is a real compositor lifecycle event. It is **not** a deterministic reproduction of a
layer-shell `closed` event while an output remains available: Sway has no IPC command that closes an arbitrary
layer-shell card, and output-layout rebuilding can bypass the exponential retry wait. Thus this gate verifies the
rebuild invalidation half, actual output loss/recovery and configuration/status outcomes; it does not prove the
retained-output close/retry window. That path still requires an instrumented compositor or a carefully isolated
protocol fixture. Rendered pixels, mixed fractional scales, physical hotplug, GLES/DRM and other compositors are
outside this gate. A passing result must not be presented as full widget-host or desktop acceptance.

Results contain binary/harness/helper SHA256 values, observed status samples, per-transition inotify events,
same host PID/thread counts and cleanup state. Failed attempts retain evidence. No source build, installation,
live session change or native execution was performed while implementing this fixture.

## Root validation checkpoint

The first native attempt found a real lock-owner lookup failure: widgets were presenting frames while the CLI
reported Down. On this filesystem `stat` reported device `00:25` while `/proc/locks` reported `00:23` for the same
locked inode. The repair retains the ordinary lock-table lookup and verifies the recorded process's actual
file-descriptor identity and exclusive kernel flock as a fallback. PID text alone never establishes readiness.
Two new tests cover real lock/unlock identity and rejection of shared/waiting/foreign/POSIX locks. All 20 host tests,
strict all-target Clippy and the development widget-host build passed:
`/var/tmp/decebal-widget-lock-identity-source-gate/result.json`.

The fixed immutable development binary passed the private Sway fixture with two output disable/enable cycles,
readiness deletion before republishing on every transition, unchanged host PID, and correct missing-definition,
zero-enabled and restored configurations. Evidence: `/var/tmp/decebal-widget-recovery-native-positive-evidence-v3/result.json`
and `/var/tmp/decebal-widget-recovery-native-positive-gate-v3/result.json`. Cleanup was clean and the resource
guard did not abort. The binary/harness/helper hashes are in the result. Earlier failed evidence is preserved.

The earlier pre-invalidation binary also predates the filesystem lock repair, so a negative run of that binary
would stop at Down before testing invalidation. No native negative control isolating only invalidation is claimed;
the watcher helper's overwrite-only negative test is narrower evidence. Normal cleanup passed; the new signal
handler routes SIGTERM/INT through the same finally cleanup, but interrupted cleanup has not yet been injected.
