# Desktop readiness follow-up

This follows the snapshot in desktop-readiness-audit.md; it does not declare KDE or
GNOME parity. Original scope remains: reliable windowing, theming, wallpaper sources,
widgets, familiar apps, performance, recovery and a polished newcomer experience.

Implemented in this batch:

- rdwm bounded settings export transfer, explicit disabled/empty preference resets,
  and screenshot chords through decutil. The nine macOS suite failures reproduced
  on unchanged `64a31e2` were stale fullscreen expectations; updated assertions exercise
  Super+F enter/leave before the separate float/fullscreen shortcuts. Updated suite:
  167 passes. Session suite: 60 passes. Screenshot chord suite: 5 passes; private
  fake-command tests cover source errors/timeouts and activation reset values.
- Real next-login runtime accessibility preferences. Raw theme/palette selection is
  not rewritten. Rendered light/dark contrast tests and native Settings at 200% pass.
  Reduced motion covers dock magnification/bounce. Screen readers, foreign apps and
  every possible custom editor/widget layout are not covered by this verification.
- Installed-code Getting Started page, searchable through Spotlight/Settings and
  CLI aliases. Paragraphs wrap instead of hiding instructions in single-line row
  summaries. Explicit destinations preserve the existing model and unsaved drafts.
  Native tests cover same PID/window, X11 reveal across workspaces and config mismatch
  refusal. Wayland activation tokens and a per-display singleton lock remain open.
- Dock lock contention retries for two seconds and reports failure; desired state is
  read after lock acquisition. Private real-flock tests exercise latest desired state.
- Absent btop reload is a no-op, while probe/helper failures remain observable. Private
  fake-command tests and the system-theme fixture cover these distinctions.

Deployment boundary:

The installed system `/usr/bin/rdwm-decebal-session` is still the old version because
passwordless installation was refused (administrator authentication required).
The source fix and sample keymap are pushed. This matters for next-login exports;
restarting the WM does not replace that system script. A later guarded WM
handoff activated the new user binary, while the system wrapper remains old.
The current user's three screenshot chords matched the old defaults exactly; only those three
argument vectors were updated, checked and reloaded, preserving other settings and
workspace/window membership.

Remaining work includes curated Wallhaven assets for each theme (sole specialist),
Wayland/CSD shadow integration, safe recovery from old
GTK style-pack symlinks, Qt/Wayland capture verification, accessibility layout checks
for other complex surfaces, and broader newcomer/performance gates. rdwm uses a
separate caption window rather than a reparented full frame. Its unified X11
shadow carrier now covers the whole SSD frame, independently of Picom's own
shadow feature. The current optimized WM passed 45 private composited pixel,
input, stacking, movement/upload and lifecycle assertions in
`tests/e2e/x11-shadows.sh` (log `/tmp/rdwm-shadow-current-optimized.log`). This
does not establish Wayland or CSD support.


Validation checkpoint:1513 library tests passed (6 ignored), strict all-target
Clippy/format checks passed. Production native suites: system theme74/74, dock
lifecycle9/9, guide/aliases/same-window navigation/200% high contrast, preview-cache
restart and widget creator/Icons all pass. Separate cold-worker accessibility
preview isolation test passes. Full production build includes all23 binaries.


## 2026-10-30 optimized rollout update

This supersedes the earlier 1,513-test and next-login-only deployment checkpoint,
without broadening what those tests prove. Shell source `0e2fd63` is built and
running; subsequent root commits through `1816055` add tests/documentation. The
library checkpoint is 1,683 passed, 0 failed, 16 ignored, with strict all-target
Clippy and format checks. Exact optimized native gates include Windows palette
changes without process restart, first-click theme previews, cache corruption and
warm relaunch, popup-launched application lifetime, Dux title redraw, popup
placement, font fallback and slider input. The historical preview-browsing
Settings disappearance has not been reproduced or assigned a confirmed cause.

The shell handoff completed with configuration and notification-history hashes
unchanged. The current notifier, widgets, bar and Settings hashes match the final
optimized artifacts. Dux 1.2 is installed without selecting it or replacing the
user's widget layout. Settings was refreshed after WM activation and exposes the
Wallpaper desktop-frame capability.

The optimized `efa8f1f` rdwm build is running after a completed guarded X11 handoff.
The three then-current clients retained geometry, flags, tags, view and focus.
An earlier failed attempt and rollback recentered an already overflowing floating
window; the original overflowing position was not restored. Helper `d463d0a`
refuses that case before signaling, with private unchanged-state regression
coverage. Do not describe the later successful retry as undoing that earlier move.

Evidence:

- `/tmp/decebal-review3-native/acceptance-manifest.json`
- `~/.local/state/decebal/deployments/review3-20261030-230112/shell-reload-journal.json`
- `~/.local/state/rdwm/restarts/20261030-234119-fu60m41l/journal.json`
- `/tmp/rdwm-restart-helper-test-_lquxo1i/result.json`
- `/tmp/rdwm-desktop-frame-optimized-repaired/result.json`
- `/tmp/rdwm-frame-settings-optimized-repaired/result.json`

The desktop-frame acceptance includes real widget/video-wallpaper layering and
Settings toggle integration in a private X11 session. Default-off frame support
is not evidence for Wayland compositor operation or physical mixed-DPI output.
System-wrapper migration, hardware gestures, AT-SPI, full Explorer/Finder
workflows, full Dux fidelity and new-user/upgrade release acceptance remain open.
