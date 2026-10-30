# Aster native QA

Actual package renders: [`aster-dark.png`](aster-dark.png),
[`aster-light.png`](aster-light.png), 1920×1080 desktop + labelled 120 px footer.
These use the exact shipped manifest, real native DecBar/Mac dock/widget painters,
and the existing Breeze caption renderer. There is no bundled wallpaper; the
background is the actual theme background colour and the footer reports the
unavailable asset. These images are UI evidence, not generated wallpaper files.

The two native desktop widgets have sample/empty renders under `widgets/` (eight
PNGs). Empty weather contributes no invented temperature; empty MPRIS shows
“Nothing playing” and no transport controls. Sample values are fixed and labelled
in the whole-desktop footer. Two dock pins and the Finder menu are existing native
renderer samples, not package-owned applications or a macOS integration claim.
The widgets have 24 px vertical separation and share a 32 px right edge inset;
both leave the central photograph/window work area clear.

For full-resolution photo/crop QA only, both linked Wallhaven originals were cached
under `/tmp/decebal-wallhaven-curation/full/`. The same native worker rendered a
**temporary** copy of Aster with those two actual files: no engine/package source
or live configuration was changed. Evidence is deliberately outside git:

- `/tmp/decebal-wallhaven-curation/aster-native/aster-dark.png`: `9mqvz1`, 3840×2160
  blue coast. The wave is below the two upper-right cards, the open sky stays quiet,
  and the small dock sits below the subject without covering its crest.
- `/tmp/decebal-wallhaven-curation/aster-native/aster-light.png`: `wex83x`, 6144×4096
  sunset lake. A 16:9 center-fill crop retains mountain/reflection/horizon; pale
  backed cards are readable against the darker blue sky and warm water.
- `private-photo-inputs.json` beside them records exact cached file SHA256 values.
  No image edits, added shadows, fake panels or decorative placeholder widgets.

These photo derivatives and originals have unknown redistribution licenses and
are **not** shipped. Review their original Wallhaven/source pages via the catalogue;
uploader and author are not interchangeable. Parent runtime selection remains
pending. This batch does not replace any other theme's old wallpaper defaults.

## Reproduce without a display or live apply

```sh
cargo build --locked --offline --bin decthemectl
python3 tools/build-decoration-preview.py /home/ambsd/Projects/rdwm
export DECEBAL_DECO_RENDERER="$PWD/target/decoration-preview-helper/target/debug/decebal-deco-preview"
python3 tools/render-curated-theme-evidence.py aster --out docs/theme-ports/aster
DECEBAL_ASTER_EVIDENCE="$PWD/docs/theme-ports/aster/widgets" \
  cargo test --locked --offline --test curated_theme -- --nocapture
# After explicitly caching the two catalogue originals, private photo QA:
python3 tools/render-curated-theme-evidence.py aster \
  --dark-photo /tmp/decebal-wallhaven-curation/full/wallhaven-9mqvz1.png \
  --light-photo /tmp/decebal-wallhaven-curation/full/wallhaven-wex83x.jpg \
  --out /tmp/decebal-wallhaven-curation/aster-native
```

The optional caption helper compiles only inside ignored `target/`, with rdwm
sources read-only. Its native hit checks assert all three real button centres
resolve to Close / Minimize / Zoom and lie within the caption. Focused/unfocused
samples use the actual configured colours. This batch adds a bounded second
native paint frame to the evidence helper: rdwm deliberately defers glyph loading
after a per-frame budget, so first-frame-only capture showed a box for the new
lowercase `f` in “Unfocused”. A second cleared native frame captures settled text;
no duplicate text renderer or rdwm source change is introduced.

## Validation and limits

`tests/curated_theme.rs` checks 21-ID complete catalogue coverage, 42 unique SFW
choices, no flagged AI/digital-art tags, real-host URLs and explicit unknown
license/verified authorship. It verifies reproducible package packing; wallpaper
image/mode/background and Wallhaven preferences survive both mode apply plans;
buttons recover from a prior false flag; user Maximise action stays intact; and
custom-widget Ask / Keep / Replace / cancelled-review paths stay safe. Both widget
sample/empty states render through native code in both modes.

Package validation has no errors. Two known engine warnings are retained honestly:
`decoration.buttons = "right"` enables buttons but side is owned by Breeze (right);
`dock.position = "bottom"` is stored, with the native Mac dock's existing fixed
bottom layout (general left/right layout support is not claimed). No left/right
dock placement or custom caption-button order is used.

Headless evidence has no live compositor shadows, backdrop blur, glass refraction,
hover magnification, interactive windows or native application restyling. Physical
screen/DPI, ultrawide/portrait cropping and live MPRIS/weather availability remain
integration checks. This is a new package and advisory catalogue, not a claim of
new Plasma effects, compositor code or verified redistribution rights.
