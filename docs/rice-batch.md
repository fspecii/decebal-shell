# Kiln Court and Tidal Loom

Two original Decebal desktop configurations, first committed in `3ce4b7b`. Their
clay/textile semantic palettes, geometry, captions and optional builtin widgets
remain intact. Generated wallpaper, image-backed package screenshots and the
art-only `tools/rice-batch-art.py` have been retired from the current tree.
Historical commits remain available; they are not current package reproduction
instructions. No wallpaper generator is used by these packages.

Choose photography explicitly in **Wallpaper → For this theme** using
`data/wallhaven-curated.json`. Theme apply and mode changes own no image, scaling
mode, background colour or download. Current wallpapers are preserved. The
parent's accepted Tidal dock-disable change remains independent of this patch;
no layout/dock key is changed by artwork retirement.

| Design | Preserved structure | Optional native widget seed |
| --- | --- | --- |
| Kiln Court | Warm clay tones, bottom pill panel, Adwaita captions, 12px gaps | Clock and CPU/RAM at upper left |
| Tidal Loom | Cool textile tones, left rail, Breeze captions, 8px inner gaps | Clock and history card at upper right |

Keep / Replace / Cancel is unchanged for custom layouts. No commands, providers,
font installation or network access is introduced. Source remains MIT; linked
photographs have separate unknown-license records and are not redistributed.

## Current evidence

`python tests/rice-batch-render.py` calls the existing native widget renderer,
placing its fixed sample-data output on a labelled plain UI test canvas in
`docs/rice-batch/`. It does not create wallpaper files or package screenshots.
Full available-native component renders from actual package metadata are in
`docs/theme-ports/automatic/{kiln-court,tidal-loom}-{dark,light}.png`; the footer
labels fixed data and missing wallpaper. No live compositor shadows or glass
effects are simulated. These are evidence PNGs, not wallpaper defaults.

The original contrast/golden/package checks remain useful. The current retirement
checks validate preserved manifest structure/widget bytes, safe packages and
unchanged independent wallpaper preferences. See `generated-wallpaper-removal.md`
for exact removal counts, retained photos, full-resolution curation and limits.
