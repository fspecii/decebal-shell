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
restarting the current WM would not solve it and was not attempted. The current
user's three screenshot chords matched the old defaults exactly; only those three
argument vectors were updated, checked and reloaded, preserving other settings and
workspace/window membership.

Remaining work includes curated Wallhaven assets for each theme (sole specialist),
proper whole-window shadows/premium decoration integration, safe recovery from old
GTK style-pack symlinks, Qt/Wayland capture verification, accessibility layout checks
for other complex surfaces, and broader newcomer/performance gates. rdwm uses a
separate caption window rather than a reparented full frame; simply enabling global
Picom shadows is not evidence of a correct whole-window shadow.


Validation checkpoint:1513 library tests passed (6 ignored), strict all-target
Clippy/format checks passed. Production native suites: system theme74/74, dock
lifecycle9/9, guide/aliases/same-window navigation/200% high contrast, preview-cache
restart and widget creator/Icons all pass. Separate cold-worker accessibility
preview isolation test passes. Full production build includes all23 binaries.
