# Retire generated theme wallpaper defaults — 2026-10-30

All **21 visible shipped nonlegacy packages**, including Aster, now omit the entire
`[wallpaper]` section. Applying a design or changing its mode sets no wallpaper
image, scaling mode, background colour, slideshow or download. Photography is an
explicit choice in **Wallpaper → For this theme**, using the separate
[`wallhaven-curated.json`](../data/wallhaven-curated.json) map. There is no new
remote manifest field or generated fallback.

## Source changes and attribution

The 16 affected generated-art packages are Brutalist, Frutiger Aero, Glyph Rice,
GNOME Adwaita-like, Gruvbox Dark, Kiln Court, Moss Atlas, Mountain Neon Dot, NextKDE,
Nothing Dot, Orbit Glass, Paper Signal, Signal Deck, Synthwave, Terminal Minimal
and Tidal Loom. Removed **26 wallpaper files, 22 packaged screenshots embedding
that art, five ARTWORK.md vector-source documents and one art-only generator**
(`tools/rice-batch-art.py`), about 15.2 MiB total. Their screenshot manifest entries
were also removed. Gallery previews use actual native metadata/renderers instead.

[`generated-wallpaper-removal.json`](generated-wallpaper-removal.json) records each
removed file/hash and baseline commit. Canonical manifest hashes prove that only
wallpaper/screenshot/description metadata changed; palette, layout, fonts,
caption geometry and widget specifications are untouched. Existing theme widget
TOMLs have byte-preservation hashes. No native text/dot renderer was removed.

Sweeteh's two **unchanged pinned upstream Unsplash photographs** remain optional
package assets, with original hashes, named filename provenance, source links and
license separation in `themes/sweeteh/ATTRIBUTION.md`. Its apply-time wallpaper
section is removed too. These are not the unknown-license Wallhaven selections.
Its photo-based historical package evidence remains attributed. No new photograph,
provider thumbnail or photo derivative is committed.

Hidden legacy palette-only compatibility packages are outside this bounded change.
Their existing assets/metadata, example packages and tiny parser/test image fixtures
remain intact. Historical commits preserve retired artwork provenance; they are
not current reproduction instructions. Existing generated images already selected
in live configuration, cached previews or installed stale files are **not** removed
by this source patch. Parent owns integration/deployment and any managed-install
cleanup; a current wallpaper changes only through explicit Wallpaper selection.

## Full-resolution recommendation audit

All 42 original choices decoded at their recorded dimensions; bounded download
limits were 42 initial files, three concurrent, 32 MiB/file, 30 s/file. About 199 MiB
was cached under `/tmp/decebal-wallhaven-curation/full`. Seven local contact sheets
show actual full-file 16:9 crops and native-pixel detail strips. The catalogue now
records full-file SHA256, byte size, verified dimensions and individual crop/
readability/provenance cautions for **all 42 current recommendations**.

Two replacements were necessary. First, In a native Mountain light render, alpine `nr7zq0`
put a dark cloud directly under the unchanged transparent homelab panel; the date
also crossed rock detail. A bounded additional search returned five snow/minimal
results. Three actual detail responses and two full candidates were fetched.
Selected the SFW **`2kqkmy`** photographic snowfield/vehicle tracks (2017 upload).
Its 16:9 crop retains the vehicle and both clear text zones. Native comparison:

- Rejected: `/tmp/decebal-wallhaven-curation/mountain-review/mountain-neon-dot-light.png`
- Replacement: `/tmp/decebal-wallhaven-curation/mountain-snow-review/mountain-neon-dot-light.png`
- Glyph pair: `/tmp/decebal-wallhaven-curation/glyph-review/glyph-rice-{dark,light}.png`
- Aster pair: `/tmp/decebal-wallhaven-curation/aster-native/aster-{dark,light}.png`

The light companion intentionally favours legibility over a second mountain
panorama; dark mode retains the alpine reference. No widget layout or backing was
changed to manufacture the result. Candidate `gw5mol` was not selected because its
edge signature needs more careful fit handling. Glyph dark `dpr913` was also replaced by inspected `zpzqeg` from the already
cached monochrome shortlist: bright rooftop edges competed with fine grey labels.
One actual API detail and one full photograph were fetched, then the same native
layout was rendered at `/tmp/decebal-wallhaven-curation/glyph-quiet-review/`.
No widget definition changed. Total extra full candidates were bounded to three.

All current images remain **42
unique choices across 21 IDs**. The rejected record and hash remain in the catalogue.

Unknown original authorship, license and capture method remain unknown. Visual
photographic appearance, a pre-AI upload date and no AI tag are not certification.
Preserve watermarks; Sweeteh's wide cloud recommendation now recommends fit for
that reason. Wide Mac landscape ends crop at 16:9; Glyph's fine grey host metadata
still needs display-specific checks; bare terminal clocks and user-positioned widgets
still need display-specific checks. Ultrawide/portrait/scaled live composition is
not verified. No request loop attempts to fill these gaps with invented IDs.

## Validation and native evidence

- `python3 tests/wallpaper-package-contract.py`: three offline audit tests cover
  preserved structure/widget bytes, removed assets, 21 independent-wallpaper
  packages and byte-identical attributed Sweeteh photos.
- `cargo test --locked --offline --test curated_theme --test glyph_rice --test theme_ports`:
  package validation/reproducibility, all 21 designs × dark/light plans preserving
  independent image/mode/colour and custom widgets, source state/cancel safety,
  native sample/empty widgets and safe action semantics.
- `cargo test --locked --offline --lib theme::engine::golden`: five package/style
  goldens. The test no longer requires an artist asset or package screenshot; it
  checks independent wallpaper preservation for packages without metadata, while
  retaining old legacy-asset checks. This is a **test-only** core-file adjustment.
- Targeted Clippy, formatting, whitespace and shell syntax checks. No X11/session
  smoke run, live provider poll, binary install or process restart.

Refreshed **34 available-native dark/light previews** for the 17 changed packages in
`docs/theme-ports/automatic`; their footers label fixed data, absent packaged
wallpaper and lack of live compositor glass. Existing affected `docs/themes/`
images now use these genuine native previews. Older widget-only compositions were
rerendered on labelled plain UI canvases; neither evidence script creates
wallpaper files or repopulates package screenshots. These solids are QA canvases,
not wallpaper artistry or new fallback assets.

Parent source through accepted `0ba18b0` was inspected read-only. Runtime cache,
accessibility, source-theme picker, Settings and rendering helpers are untouched.
The parent already disabled Tidal's duplicate dock; its accepted baseline hash is
allowed by the preservation audit during integration. This patch leaves those dock
keys untouched, as well as the parent's macOS magnification/font fixes. Apply only
this commit's diff; do not replace parent files with whole older worktree copies.
