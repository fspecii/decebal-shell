# Native theme preview cache recovery

The gallery previously accepted an existing PNG without decoding it, and cached a failed card-image decode indefinitely. A damaged cache image could therefore remain a blank card even while Settings stayed alive. Preview generation now validates existing PNG data on its background worker and regenerates invalid files. Validation is reused by content digest; metadata/digests stay off the UI thread. The gallery caches successful decodes only. Replacing bytes at the same content-addressed path explicitly notifies preview consumers, since path equality alone does not imply unchanged image data.

## Evidence (2026-10-31)

* Rust source `2b68879`: 13 focused preview/card tests, formatting, strict all-target Clippy and dev Settings + renderer builds passed. `/var/tmp/decebal-preview-repair-source-gate-v3/result.json`.
* Previous optimized release `a82dc3c` failed the strengthened corruption-repair test as expected: Settings remained open, but the damaged image stayed invalid through the 60-second recovery timeout. `/var/tmp/decebal-preview-repair-negative-control/result.json`. This is a failing baseline, not a passing production gate.
* Immutable dev artifacts from `2b68879`, using fixture `42407cd`: first Appearance click painted a preview; a private damaged PNG was repaired and painted after relaunch in 4.72 seconds with no pointer/key/resize/expose command after the initial resize. Browsing, generic window reuse, damaged-cache and warm relaunch checks passed. No leftover/rescued fixture processes or I/O-pressure abort. `/var/tmp/decebal-preview-repair-final-native-control-v2/result.json`, `/var/tmp/decebal-preview-repair-idle-evidence-v2/result.json`, `/var/tmp/decebal-preview-repair-final-native-evidence-v2/result.json`.
* Initial test failures remain recorded: the first unit assertion compared an enlarged thumbnail with its tiny input, and the first idle fixture referenced an undefined cache variable. Both were test errors, corrected before the results above.

These checks cover one deliberately damaged visible preview and a private gallery browsing session. They do not prove all packages, all thumbnails simultaneously, physical displays or Wayland. The production `a82dc3c` bundle remains held; this repair is not installed and requires optimized-artifact acceptance.

Use `SETTINGS_IDLE_REPAIR=1` with `tests/e2e/settings-gallery-first-open.py` to exercise autonomous recovery. `SETTINGS_CORRUPT_CACHE=1` with `tests/e2e/settings-preview-stress.py` now requires a decoded replacement image, rather than merely an app that remains alive. Both fixtures use owned high-display Xvfb servers and check ownership before input.
