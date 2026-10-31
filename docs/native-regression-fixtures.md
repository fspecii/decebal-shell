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
