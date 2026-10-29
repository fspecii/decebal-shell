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
- Priority 2, completed in the Glyph refinement commit: improve existing Glyph Rice from reference ending
  `e808c311-08d1-4b1c-bcab-88b002ce1d9e.png`: giant dot time and greeting left,
  weather/calendar/day cards above MPRIS and system cards right, black dot texture,
  slim left rail. Reuse the stopped `feat-monochrome-dot-dashboard` worker's
  six-card layout and numeric dot-renderer prototype. Existing package GPL
  attribution remains applicable; screenshot alone does not establish identity.

- Priority 3, completed in the Sweeteh port commit: [Aarav2709/Sweeteh](https://github.com/Aarav2709/Sweeteh).
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

## Glyph refinement checkpoint

Mountain commit: `3d93bf3728ef912b21d9cb5cfd25aaa384f0ce92` (includes shared dot
text support). Glyph reuses that primitive and the stopped worker's six cards.
Evidence: `docs/theme-ports/glyph-rice/`, dark/light and sample/empty. Old
three-card renderer now delegates to the canonical six-card render path.

Rejected: disabled caption buttons, ordinary-font clock, blank album-art holes,
static play icons, unavailable temperature rendered as a real degree reading,
and empty CPU graphs that look like a measured zero. Keep the original GPL
palette/art/license; avoid adding guessed screenshot provenance. Renderer limits:
continuous day ring/CPU line graph, no cover-decode failure predicate, fixed
40 px native rail, 1920 × 1200 seed requiring smaller-screen layout edits.

## Decoration track — queued after current ports

Audit `/home/ambsd/Projects/rdwm/crates/rdwm-deco/src/config.rs`, styles and
hit-testing read-only. Existing IDs found: macos, macos-light, classic, windows,
windows-light, adwaita, adwaita-light, breeze, arc, materia, nord, gruvbox, dracula,
minimal, borderonly. Do not duplicate them. Decebal's snapshot/picker still lists
only macos/classic; first deliver a clear picker for supported structural styles,
with functioning caption controls and representative native previews if feasible.
Keep borderonly out of the normal button-style picker. Priorities after audit:
GNOME/Adwaita, KDE/Breeze, Arc, then missing Elementary/Pantheon; Material already
has `materia`. Catppuccin/Nordic/Orchis must be assessed for structure versus
palette-only variants. Missing renderer work needs parent coordination of a
proper tool-managed rdwm checkout; never edit the live rdwm source checkout.

Preserve parent preview-gallery `e33a0f1`; no edits to `src/livewall`,
`src/settings/app.rs` or parent-owned Settings UI renderer files. Keep catalogue,
hit areas/actions, focused/unfocused, dark/light and scale QA for each delivery.

## Sweeteh checkpoint / automatic-preview requirement

Glyph commit: `21e01845901c5d15bc0125787141890ee0e30d9c`.
Sweeteh upstream pinned `b7c0bdaa6aca54c7b45e341341b4b34408bb9c5a`.
Real defaults/code/demo inspected; port uses fallback Material blue roles, 4 px
native ledges and eight optional shared-data widgets. GPL code, OFL Google Sans
and two unchanged Unsplash photos have separate provenance/hashes in package
ATTRIBUTION.md. No demo-frame flower image or uncredited video imported.
Native evidence: `docs/theme-ports/sweeteh/`; 32 widget state/mode PNGs and four
compositions. Font registration, custom folder recoloring, cursor selection,
true week grid, island/tray reservation, extra terminal slots and animation are
explicit engine gaps, not claimed integrations. No upstream installer executed.

Next required architecture: automatically generated package/gallery previews,
not hand-drawn desktop miniatures or mandatory manual screenshots. Resolve actual
manifest (inheritance included), effective mode/palette, artwork, widget files and
anchors; compose real native pixels for widgets/bar/dock/decoration where
available. Fixed labelled sample data is allowed, per-theme invented layouts are
not. Use a content-addressed cache including manifest/assets/builtin widget
sources, palette and renderer version; request regeneration on discovery after
creation/import/change. Run generation in a private headless process, never by
applying anything to the live session. Test each invalidation dimension and
retain both modes. Missing native render support must be labelled, not mocked.
Checkpoint all ports before bringing parent `e33a0f1` preview-gallery source into
this branch; retain its entire livewall/Settings change and do not edit the
parent checkout. Decoration catalogue/picker follows sequentially.
