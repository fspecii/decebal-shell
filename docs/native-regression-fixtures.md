# Native applet and launcher regression fixtures

Use system Python with python-xlib, Pillow, Xvfb and XTEST. Supply immutable binaries explicitly. Run one native job at a time through `tools/dev/desktop_job.py`; coordinate its pause/slot policy with the desktop owner. These tests do not build or install anything.

```sh
# DECEBAL_QA_BIN is an absolute immutable bundle bin/ directory.
python3 tools/dev/desktop_job.py -- python3 tests/e2e/control-center-reload-native.py \
  --bin "$DECEBAL_QA_BIN/decbar" --out /absolute/fresh-reload-evidence
LAUNCHER_BIN_DIR="$DECEBAL_QA_BIN" \
LAUNCHAPP_BIN="$DECEBAL_QA_BIN/examples/launchapp" \
LAUNCHER_RAPID_EVIDENCE=/absolute/fresh-rapid-evidence \
python3 tools/dev/desktop_job.py -- python3 tests/e2e/launcher-rapid-toggle.py
```

Both fixtures require fresh evidence directories and retain their logs/results. They use owned high-display Xvfb servers, private HOME/XDG paths and unavailable private service buses. Action tools are fixture fakes; only read-only font discovery is forwarded. Native commands verify X-server ownership. Cleanup signals use pidfds after same-UID/private-token checks; forced cleanup or leftover processes fails the run. Copied binaries prevent sibling lookups from reaching installed services.

`control-center-reload-native.py` starts one overlay per Card, Fluent, GNOME and Dux style. Each existing PID/XID transitions actions → empty → actions → unavailable media through atomic JSON replacement, without injected refresh signals. It checks target changes, inert keyboard navigation, changed/stable rendered pixels, positive controls and process/window continuity. This covers standalone X11 overlays at one display size/scale; Settings pointer persistence has separate Rust tests. It does not prove Wayland, hardware actions, all theme fidelity or physical animation timing.

`launcher-rapid-toggle.py` checks bursts of 2/3/4/5/7/8 typed toggles, clean intentional exits, no stray launcher, the shared launch-monitor route, idempotent Open, Close never opening, and genuine SIGTERM retaining its failure status. It does not physically click a Start button or measure frame timing.

Portability/safety checkpoint `746d6de`: both repository fixtures passed against the exact qualified **704c657** release binaries in 17 seconds, with no rescue, leftovers or I/O-pressure abort. The reload test recorded 16 states. The rapid test recorded six clean bursts, zero false launch-monitor reports and genuine SIGTERM -15. Evidence: `/var/tmp/decebal-portable-native-fixtures-control/result.json`, `/var/tmp/decebal-portable-cc-reload-evidence/result.json`, `/var/tmp/decebal-portable-launcher-rapid-evidence/result.json`.

The fixture-source commit also contains newer Wayland source inherited from upstream. This checkpoint tests the supplied 704c657 binaries; it is not acceptance of that newer source. Binary/helper hashes and fixture hashes are retained in results.


## UKUI and Wayland fixtures

These also require explicit immutable binaries and new evidence directories. Run them through the same single-job resource guard:

```sh
python3 tools/dev/desktop_job.py -- python3 tests/e2e/openkylin-launcher-native.py \
  --bin "$DECEBAL_QA_BIN/declaunch" --fixture-dir tests/e2e --out /absolute/fresh-ukui-evidence
python3 tools/dev/desktop_job.py -- python3 tests/e2e/wayland_compact_popup.py \
  --bin-dir "$DECEBAL_QA_BIN" --out /absolute/fresh-wayland-popup-evidence --keep --input
python3 tools/dev/desktop_job.py -- python3 tests/e2e/wayland_widgets_recovery.py \
  --bin "$DECEBAL_QA_BIN/decwidgets" --out /absolute/fresh-wayland-widget-evidence
```

The UKUI launcher gate covers 72 screen/scale/mode/edge combinations, same-PID/XID expand/restore, search launching and pointer actions. The integrated development artifact passed; see [the scoped acceptance record](desktop-ports/openkylin/replica-gaps.md). This does not establish compositor blur or upstream pixel fidelity.

The popup gate needs headless-capable Sway, swaymsg, foot and wtype. It verifies private sockets with kernel peer credentials, uses virtual input devices and a native key recorder, and checks exact output identity, per-card frame/buffer requests, Escape, both outside-click paths and keyboard hand-back. [The contract and evidence](wayland-compact-popups.md) distinguish development native results from release/hardware acceptance.

The widget gate checks same-host output loss/recovery, readiness deletion before republication, and invalid/empty/restored configurations. [Its limitations](wayland-widget-native-gate.md) explicitly exclude the retained-output layer-shell closed/retry window and an isolated native invalidation-only negative control.

For deployment, qualify the version-matched data bundle as well as the binaries. A running `DECEBAL_SYSTEM_THEMES` can remain pinned to an older release even after executable replacement. Verify that the new launch environment points to the new immutable system-data directory; preserve user theme overrides and retain the old bundle for rollback. See [release verification](../tools/release/README.md). A result for one supplied binary is never acceptance of a different build.


## Integrated source qualification checkpoint

Source `677af74` passed the complete library suite under a private D-Bus session and the serialized resource
guard: 2,402 passed, 0 failed, 23 ignored, in 768 seconds. Evidence:
`/var/tmp/decebal-integrated-library-gate/result.json`. The integrated launcher artifact also passed all 72
native combinations (`/var/tmp/decebal-ukui-integrated-launcher-matrix-control/result.json`). These results
qualify their recorded development/test artifacts; optimized binaries still require their own native gates.
Ignored tests, retained-output layer-shell closure/retry, physical input/output and rendered replica fidelity
are not silently promoted to passed. The next build must include matched runtime theme data.
