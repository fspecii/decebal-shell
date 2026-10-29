# Next feature selection — 2026-10-29

Base reviewed: `feat/desktop-production-hardening` at `4bdc3e7`.
Source of recorded needs: the Obsidian project note `10 Projects/macshell.md`,
especially “Next actions and acceptance criteria” and the historical “Open items”.

No eligible, concrete, unimplemented new feature was found in that note or the
repository documentation. Recorded feature requests for Spotlight, calendar,
Wallpaper settings, Stay Awake and reusable wallpaper palettes are implemented.
The current unchecked items concern maintenance and validation. This proposal
therefore does not claim a new product requirement or implement speculative UI.

## 1. Make absent btop a successful reload no-op

This is the smallest actionable current backlog item, although it is a fix rather
than a new feature. The note explicitly requests removing the nonfatal warning
when no btop process exists.

`src/theme/system/omarchy.rs` currently gates both reload paths on the presence of
the btop executable, then invokes `omarchy-restart-btop`. Executable availability
does not establish that an instance is running.

Scope: distinguish an absent process from a failed reload in both apply paths.
Keep genuine helper failures visible and preserve existing signal opt-in rules.
Do not edit upstream scripts or live configuration.

Acceptance: private fake-command tests cover installed-but-not-running btop,
running btop, missing executable, genuine helper failure and process exit between
discovery and reload. No helper warning for absence; genuine errors remain
reported. Existing Omarchy bridge tests pass. Coordinate ownership with the
application-theming worker before editing this shared module.

## 2. Close the recorded screenshot capture verification gap

The historical Open items say screenshot capture was never completed end to end.
Reconfirm that this is still outstanding before implementation. Capture entry
points already exist; this is not authorization to add a screenshot application.

Scope: an isolated regression for the Control Center Screenshot action and the
Spotlight Screenshot command. Inspect their provider selection: the bar currently
falls back to grim/slurp, while Spotlight has X11-capable alternatives
(`src/menubar/mod.rs`, `src/spotlight.rs`). If the private X11 test demonstrates a
failure, propose a small shared provider-selection correction.

Acceptance: fake providers establish command selection, missing-tool handling and
argument preservation without capturing the user's desktop. A private Xvfb test
with an already installed capture tool produces a valid image of known content.
No downloads, installed tools, live screen capture or new settings pane. Any
interactive Wayland validation remains explicitly pending without a private
compositor and its existing dependencies.

## 3. Verify Qt appearance in isolation

The current note explicitly leaves real Qt application appearance unverified.
This is acceptance work for an existing integration, not a new theme feature.

Scope: use an already installed Qt application, private HOME/XDG directories,
private D-Bus and private display. If dependencies are absent, report the missing
prerequisite rather than installing software or claiming runtime coverage.

Acceptance: record the application, Qt version, selected platform-theme adapter,
and visible dark/light results. Check palette propagation and that private files
can be restored. Do not infer whole-desktop readiness from this probe. Coordinate
with the application-theming worker before changing adapters.

## Boundaries and recommendation

Prioritize item 1 as a separate maintenance task; use item 2 if a small user-facing
correction is desired and the gap is confirmed. A new feature needs a concrete
request before implementation. Physical multi-monitor, motion/glass and longer
desktop regression items remain recorded acceptance work requiring suitable test
conditions.

This proposal excludes live wallpapers, theme packages, widget catalog additions,
WM tiling, lockscreen and deployment. It requires no changes to `src/livewall`,
rdwm, production PAM behavior, installed binaries or running desktop processes.
