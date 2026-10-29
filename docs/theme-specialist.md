# Decebal Theme Specialist

This thread owns all theme creation and ports. No theme subagents. Work only in
this checkout; other workers' checkouts are read-only source material. General
widget/features work stays in its own threads. No installs, live configuration,
restarts, rdwm edits, pushes or merges. Preserve PAM and window buttons, and the
custom-widget Keep / Replace / Cancel policy. Do not touch `src/livewall` or
`src/settings/app.rs` while the parent is rebuilding them.

## Backlog and checkpoints

- Completed: `3ce4b7b24566b372ed0bfda2510e184f40119de3`, original Kiln Court /
  Tidal Loom dark/light batch. Keep it intact. See `rice-batch.md` for evidence.
- Priority 1, completed in this theme commit: Mountain Neon Dot, from user reference ending
  `5cb42945-93ba-4f85-b386-ba20dd4a15b9.png`: teal snow mountain, magenta/cyan
  separated top islands, transparent stacked dot clock/date at left, homelab at
  right, no dock. Reusing six widget definitions and both imagegen wallpapers
  from `feat-mountain-neon-dot` (worker stopped by user). No upstream identity is
  known; do not guess one.
- Priority 2, next: improve existing Glyph Rice from reference ending
  `e808c311-08d1-4b1c-bcab-88b002ce1d9e.png`: giant dot time and greeting left,
  weather/calendar/day cards above MPRIS and system cards right, black dot texture,
  slim left rail. Reuse the stopped `feat-monochrome-dot-dashboard` worker's
  six-card layout and numeric dot-renderer prototype. Existing package GPL
  attribution remains applicable; screenshot alone does not establish identity.

- Priority 3, queued: [Aarav2709/Sweeteh](https://github.com/Aarav2709/Sweeteh).
  Inspect pinned upstream README/code/assets/license first, then port cohesive
  colors/fonts/layout/widgets and legal wallpaper/icon choices. Track application
  and terminal coverage and engine gaps. Never run its installer.

Both stopped source checkouts are under
`/home/ambsd/.t3/worktrees/decebal-theme-studio/`; never edit or commit there.
Actual reference images were inspected at the supplied attachment paths.

## Design and renderer knowledge

- Ordinary widget text uses the UI font, so putting Doto in `font.display` does
  not make a dot clock. Reject a font-only approximation. Reuse and finish the
  worker's small safe `dot-text` primitive: original seven-row alphabet, round
  dots, deterministic width, no external font or asset dependency.
- Native `decwidgets render` uses **sample** data unless `--live` is supplied.
  Label compositions clearly. Never bake a sample render into artwork or imply
  that sample metrics are actual device/service status. Test empty data too.
- Homelab has no shared endpoint-health provider. Keep service rows explicitly
  unconfigured; reject hard-coded uptime, success dots and latency values.
- Theme `wallpaper.dark/light` already exists and follows mode when the engine
  owns the selected artwork. Prefer it to `images` for paired artwork in ports.
- Shared vertical bar geometry is fixed (40 px pill, 52 px strip). A manifest
  cannot set an arbitrary narrower rail or configure three separated top
  islands. Mountain uses safe interactive widget islands with bar/dock off;
  these do not supply a full tray. Glyph keeps the native functional rail.
- Layouts are pixel seeds, not responsive grids. Measure bounds at the intended
  resolution and document smaller/scaled-screen limits. Containers do not
  automatically ellipsize long text: set max_chars and reserve width.
- Preserve window controls explicitly; a border-only decoration with
  `buttons = "none"` is rejected even when a reference hides chrome.

## QA contract

For each port: validate/pack with the real package engine; render all native
widgets in dark/light, plus absent-data states; inspect compositions and
individual cards; check contrast, anchors and non-overlap; test palette/mode and
custom-layout choice preservation. Retain attribution, source asset hashes,
reproduction commands, rejected approaches and practical fidelity limits. Commit
one theme at a time, and update this note with exact commits and evidence.

## Mountain checkpoint and rejected approaches

Mountain native evidence: `docs/theme-ports/mountain-neon-dot/`, six widgets in
both modes and sample/empty states, four compositions. Port contracts 2/2, golden
package/apply 5/5, dot-renderer pixels/width 1/1, CLI validation clean. Reproduction
and original imagegen SHA256s are in the package README. The three-island layout
is explicitly an approximation without tray/struts; do not claim full bar parity.

Rejected: ordinary-font clock, live host metrics baked into gallery art,
hard-coded homelab health and ASCII distro identity copied from the screenshot.
Light QA required a backing behind system text and moving homelab off the peak.
The stopped dot prototype also lacked a layout-dump match arm; completed it.
A prototype pixel-gap assertion sampled the antialiased edge, not an empty gap;
corrected the assertion while retaining the 0.43-cell round-dot radius.
