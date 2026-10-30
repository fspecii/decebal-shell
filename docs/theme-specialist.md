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

## Preview performance/fitting follow-up — 2026-10-30

Parent `8c62a7e` inspected read-only: helper autodiscovery retained, merged DotText /
Ascii understood and left untouched. GUI preview resolution and gallery integrity
work now use bounded workers, process-local metadata/digest memoization, shared
private fontconfig cache and direct uniform contain drawing. No Settings sizing or
widget renderer edits. See `theme-previews.md` for measurements and edge-marker QA.
Rejected: per-job cold font caches, synchronous per-card asset hashing, cover/rounded
crop for evidence. Viewport priority remains deferred; FIFO queue is bounded to32.

## Curated photography + Aster — 2026-10-30

User rejects procedural/AI wallpaper defaults. `data/wallhaven-curated.json` now
maps all 20 existing visible shipped package IDs + Aster: 21 IDs / 42 distinct SFW choices.
Actual API detail/tag metadata and large thumbnails inspected; AI-art tag 133451 /
AI generator tags rejected, plus provisional digital-art picks replaced. Uploaders
are never promoted to authors; tagged photographer names remain unverified
candidates and all 42 per-image licenses unknown. No linked art/thumb/derivative
committed; private contact sheets/full-resolution Aster photo QA in
`/tmp/decebal-wallhaven-curation`. See `wallhaven-curated.md` for exact IDs/limits.

Aster is an original slate/porcelain coastal design: existing Breeze captions,
28 px top panel + 44 px Mac launch dock (no rail/taskbar duplication), modest native
surface alpha, two existing safe builtin time/media cards. No image inheritance,
remote manifest field, font asset or generated wallpaper fallback. Existing
Wallpaper choices and custom-layout confirmation are tested. Actual native modes
and absent-source widgets in `theme-ports/aster`; photo derivatives remain private.
Native caption helper needs two bounded cleared paint frames for settled glyphs
because rdwm budgets first-frame loading; preserved the existing renderer/hit tests.

Parent f68d460, saved-preview index 837799f and widget creator/Icons integrations
inspected read-only. No engine/Settings/widget source replaced. Parent owns runtime
catalogue wiring/data installation, old generated-default removal and compositor
shadow profile. Final contrast/crop QA for other 40 choices and ultrawide remains.
Do not assume undocumented `ai_art_filter=0` is enforced: use detail tags + explicit
visual review; parent provider has no AI filter preference yet. No competing agent.

## Generated-default retirement — 2026-10-30

All 21 visible package manifests now have no wallpaper section. Removed 26 known
generated images, 22 package shots, five SVG-source docs and the art-only rice
generator across 16 packages; 17th manifest is Sweeteh, retaining its two attributed
upstream photos unchanged as optional assets. No layout/palette/widget/font fields
changed. Parent Tidal dock-disable and macOS magnification baselines are preserved
on integration; no whole-file copy from this older checkout. Hidden legacy aliases,
examples, unrelated test solids and native dot widgets stay untouched.

All 42 original Wallhaven files reviewed from full resolution (bounded ~199 MiB).
Mountain light nr7zq0 failed transparent homelab/date contrast; replaced with actual
2017 snowfield photo 2kqkmy after native QA. Glyph dark dpr913 also replaced by inspected zpzqeg to avoid bright rooftop edges
under fine grey labels. Two replacements total; no more searches in this batch.
Current 42 images / 21 IDs each have
full-file hashes and crop/provenance caveats. No verified author/license invention.
Private full files/contact sheets/photo UI evidence: /tmp/decebal-wallhaven-curation.
No photos/derivatives committed. 34 package-only native mode previews refreshed;
plain labelled UI canvases are evidence, never generated wallpapers.

See generated-wallpaper-removal.md + JSON audit; tests verify both-mode independent
wallpaper image/mode/background + custom-widget preservation for every design and
retained photo hashes. A core golden **test-only** expectation now permits art-free
packages. No runtime renderer/helper, parent picker/cache/Settings/accessibility,
rdwm, live config/install/process or push changes. Parent owns managed-install stale
asset cleanup and explicit runtime photo selection. Physical multi-display crop,
watermarks and marginal fine Glyph text remain caveats.

## Hidden compatibility follow-up — 2026-10-30

401ebc9 accepted for parent integration. Extended its art policy to eight hidden
aliases still shipped by make install: caelestia-mocha, everforest, kanagawa,
material-you-dynamic, nothing-light, rose-pine, terminal-amber, tokyo-night. Removed
eight generated wallpapers, eight package shots and 14 doc shots; no remaining
art-only generator found. All 34 shipped manifests now omit wallpaper sections.
Exact colour tables, structure and Terminal Amber inheritance are preserved by
canonical manifest/widget hashes; legitimate Sweeteh photos remain unchanged.

Material You's old wallpaper-owned match/scheme defaults are removed, not moved
into an unsupported field. Fixed compatibility palettes remain exact; optional
Match wallpaper is separate. No catalogue searches/new IDs, runtime/src/helper
changes, native controls, installs/live deletion/restarts/push or new agents.
Parent owns legacy canonical-output snapshot expectations; do not change core
golden tests here. Follow-up audit: hidden-wallpaper-removal.md + existing JSON.


## Dux Lavender — 2026-10-30

New Dux adaptation inspected actual upstream README desktop/dashboard pictures,
StockThemes Purple pairs and current DDash QML; shell46a12d9/pluginsa55bb40 MIT,
registryf3079be has no root licence. Full pins/notices/source hashes in package
ATTRIBUTION.md,LICENSE.md and docs/theme-ports/dux-lavender/sources.json. Stock
Purple light is#fef7ff (not early proposed#fffbfe). One floating26px native bar,
Materia captions/buttons, dock disabled, installed Adwaita icons and ordinary font
fallbacks. No copied screenshot artwork/logo/QML/plugin scripts/generated default.

Synced own checkout to accepted parent08c07a2 via merge3e79026 with exact parent
TREE, preserving all earlier branch history. Reused separate native-thread widget
commits de48c1c/37c115f as isolated fb3cc1d/842c8a7 dependencies; parent integrate
originals once, then package commit only. No competing widget implementation or
Settings edit. IDs resource-overview320x248,focus-hub320x292; enlarged-text native
sizes324/363high. Top-right resource plus bottom-right focus avoids enlarged overlap.
Existing clock/media upper-left have fixed geometry; no200%reflowclaim for those.

Actual automatic dark/light package previews and20native widget renders/layouts,
Materia dark/light1x2x sheets; all labelled fixtures, no compositor shadow claim.
Private reviewed-photograph previews at/tmp/decebal-dux-photo-evidence-final.
Catalogue now22IDs/44choices/42photos; re-reviewed moonl3edjp and sunset39zjq6
intentionally reused. No photo redistribution or theme-apply wallpaper change.
Helper now stages exact SHA-checked private independent backgrounds because new
parent previews correctly ignore package wallpaper metadata for curated IDs.

Rejected bare LICENSE/JSON inside package due existing archive data-only rules;
use LICENSE.md and docs source hash JSON. Source/store already discovers installed
packages, so no additional builtin registration. Unknown photographer/licence
remains unknown; no rights inferred from upstream screenshots/softwareMIT.

Dux source palette/layout and docs are sole specialist ownership; native Settings
and shared widgets remain separate thread ownership. Full artworkaudit retains
pre-existing NextKDE accepted-parenthash drift (f2788246 versus oldeedb3); no change
here to oldauditrecord. See theme-ports/dux-lavender.md for exact focused checks,
renderer limits and follow-ups. No live apply/install/restart/push/rdwm edit.


Dux short-output follow-up:fca4442 parent acceptance. Move focus to bottom-left,
resources top-right. Native combined4card bounds now proven1280x720 and1920x1080,
100/200%bothmodes; no skipped720/200case. IMPORTANT200%clockfixedheightclips
clock/date andmedia truncates: geometrypasses !=readabilitypasses. Parent shared
clock/media reflow follow-up; no inventedresponsivepackagefeature. Existing42SHA
reviewedphotographs supportDuxdarkl3edjp/light39zjq6 andFollowtheme; no hardcoded
wallpaperdefault/runtimechange/newsearch. Evidence undercombined-*plusJSONbounds.
