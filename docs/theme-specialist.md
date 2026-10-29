# Decebal Theme Specialist — 2026-10-30

This is the sole thread for theme creation/ports, references, palettes/layouts,
widgets, assets, attribution, previews and visual QA. No theme agents. General
widget/features work stays separate. Work only in this checkout; stopped workers'
checkouts and `/home/ambsd/Projects/rdwm` are read-only. No live config, installs,
restarts, rdwm edits, pushes or merges. Preserve PAM, caption controls and widget
Keep / Replace / Cancel. Parent owns integration/deployment.

## Completed checkpoints

| Work | Exact commit | Evidence / decisions |
| --- | --- | --- |
| Kiln Court + Tidal Loom | `3ce4b7b24566b372ed0bfda2510e184f40119de3` | Original clay architecture / woven bands; `rice-batch.md`. Ancestor preserved. Paired mode artwork corrected in automatic-preview follow-up. |
| Mountain Neon Dot | `3d93bf3728ef912b21d9cb5cfd25aaa384f0ce92` | Six safe widgets, generated art recovered byte-for-byte, honest unconfigured homelab; package README + `theme-ports/mountain-neon-dot/`. |
| Glyph Rice 1.1 | `21e01845901c5d15bc0125787141890ee0e30d9c` | Giant dot time, six real-data cards, original GPL palette/art, functional native rail/captions; package README + `theme-ports/glyph-rice/`. |
| Sweeteh | `c0551988daacfc74598277fd837f80e1db44261d` | Upstream `b7c0bdaa6aca54c7b45e341341b4b34408bb9c5a`; eight raised widgets, real fallback Material roles, GPL/OFL/Unsplash attribution; package README + ATTRIBUTION.md. |
| Automatic previews | `c80ab4a8b9badacdb4ec0ca6ab99abf2587a6593` | Actual metadata, wallpaper, widget/bar/dock painters, optional native captions, isolated worker/cache; `theme-previews.md`, `theme-ports/automatic/`. |
| Existing decoration designs | This decoration-picker commit | 14 supported designs exposed, native defaults/controls; `decorations.md`, `decorations/` and generated `src/settings/decoration-catalogue.json`. |

Parent source through `e33a0f1` is retained via local cherry-picks `a92416c`,
`9e9dd00`, `15c9844`. Livewall source matches that parent checkpoint. Further
Settings changes build on it; never overwrite the parent's gallery/video work.

## Reference continuity

Actual user screenshots were inspected, not inferred from text:

- Mountain: attachment suffix `5cb42945-93ba-4f85-b386-ba20dd4a15b9.png`.
  Teal snow mountain, rose/cyan top islands, stacked transparent dot time/date,
  homelab right, no dock. No upstream identity supplied; do not guess one.
- Glyph: suffix `e808c311-08d1-4b1c-bcab-88b002ce1d9e.png`.
  Giant dot time/greeting left; weather/calendar/day, MPRIS/system right;
  black dot texture and slim rail. Existing upstream GPL attribution retained.
- Both partial workers stopped by user: `feat-mountain-neon-dot` and
  `feat-monochrome-dot-dashboard` under the sibling T3 worktrees. Their files
  were reused read-only. No edits/commits in those checkouts.
- Sweeteh: actual pinned README/code/license/assets and video frames inspected.
  Do not copy the demo's flower image from frames or the video without provenance.
  Checked-in photos, source URLs/hashes and font licensing are recorded separately.

## Renderer knowledge / rejected approaches

- Ordinary widget text uses UI fonts; Doto in `font.display` alone never made a
  dot clock. Shared original `dot-text` now draws bounded seven-row round dots.
  Letters uppercase; unsupported glyphs use `?`. Avoid it for user names/prose.
- No homelab provider exists: retain NOT CONFIGURED, never fake uptime/latency.
  No media: hide transport. No art: honest placeholder. Missing weather/history:
  explicit unavailable. Do not bake live/sample telemetry into wallpaper.
- Actual host geometry and `wallpaper.dark/light` exist. Prefer paired metadata;
  preserve custom wallpapers on mode edits. Do not special-case theme IDs.
- Native rail: 40 px pill / 52 px strip. Widget islands lack tray/struts and may
  be covered by apps. Source pixel seeds need editing on smaller/scaled screens.
- Native previews are generated lazily after discovery/create/import/edit, using
  fixed labelled data and content keys for manifests/assets/widgets/palette,
  renderer version/helper identity and installed font identities. Tests mutate
  each input. Palette tiles are swatches; no generic desktop drawings remain.
- Headless compositor glass/motion, week-grid offsets, custom folder recoloring,
  automatic bundled-font registration and media-art decode-failure predicates
  remain gaps. Do not claim them. See detailed package/preview docs.
- Rejected: border-only/disabled caption buttons as visual styling, fake generic
  widgets/windows in gallery previews, static play glyphs and ordinary-font clocks.
  Native QA moved mountain text off bright rock; AA edge pixels are not empty gaps.

## Next incremental decoration work

Existing Adwaita, Breeze, Arc and Materia renderers are exposed; do not duplicate.
`borderonly` is diagnostic only, outside the normal button picker. Nord/Gruvbox/
Dracula and light variants are existing style treatments, not new full themes.

1. Elementary/Pantheon is genuinely absent. **Parent must coordinate a proper
   tool-managed rdwm checkout** before renderer edits. Concrete scope/reference,
   registration, three functional controls, native hit/action/scale tests and
   attribution are in `decorations.md`. No shell worktree or edits to live rdwm.
2. Audit Orchis structure against Adwaita/Materia before adding a renderer.
3. Audit Catppuccin/Nordic variants; use palettes when only colors differ.

One missing structural style per tested commit. Keep all completed ports intact.
Validation checkpoint: full Decebal suite 1,467 passed, zero failed, five ignored
(including the explicit native picker evidence test), PAM-start probe filtered;
that ignored picker test separately passed in dark and light. Read-only rdwm
core/deco suite: 526 passed, zero failed, two ignored. Native catalogue: 60
mode/scale/state sheets with real hit-area assertions. All-target/all-feature
Clippy with warnings denied, formatting and diff checks are required before handoff.
