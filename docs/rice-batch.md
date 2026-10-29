# Kiln Court and Tidal Loom

Two original desktop designs added on top of `4bdc3e7`. This batch deliberately
avoids the existing forest landscapes, orbital instruments, editorial paper,
terminal grids and glass motifs. Both are data-only format-1 packages; no widget,
lockscreen, wallpaper-player or window-manager implementation changed.

| Design | Structure | Optional shared widget layout |
| --- | --- | --- |
| Kiln Court | Clay vaults, bottom pill panel, no dock, 10 px shell corners, Adwaita captions, 12 px gaps | Clock and CPU/RAM rings at upper left |
| Tidal Loom | Flowing textile bands, left pill rail, compact Windows-style taskbar, 16 px corners, Breeze captions, 8 px inner gaps | Clock and system-history card at upper right |

Each includes complete dark/light semantic colors, two original 3840 × 2160 PNG
wallpapers and two gallery images. Builtin registration is needed for discovery
without copying packages into user configuration. Existing installation recipes
already copy the whole themes tree, so no packaging special cases are needed.
No font installation is required; both use existing font fallback chains.

Apply after integration with `decthemectl theme apply kiln-court` or
`decthemectl theme apply tidal-loom`. Both now use the engine's existing
`wallpaper.dark/light` metadata: engine-owned artwork follows mode changes,
while a custom wallpaper remains selected. Automatic native previews use those
actual metadata paths; no theme ID selects an image in renderer code.

Widgets are optional through Settings → Widgets. Theme layouts follow the
existing ownership policy: untouched layouts switch with themes; replacing a
customized layout requires Keep / Replace / Cancel. Mode and palette edits retain
placement. No custom widget definitions, providers or commands are added. The
native headless renderer uses sample data, so the rendered clock, weather and
resource values are illustrative.

## Attribution and reproduction

All artwork and compositions are original work for this repository, copyright
2026 Decebal contributors, under its MIT license. No external images, theme code,
logos or fonts are redistributed. These are architectural and textile studies,
not ports of another rice. Editable deterministic artwork source is
[`tools/rice-batch-art.py`](../tools/rice-batch-art.py), using Pillow only.

```sh
python tools/rice-batch-art.py
cargo build --locked --offline --bin decwidgets --bin decthemectl
python tests/rice-batch-render.py
```

The renderer starts no display server or host, uses private HOME/XDG/config paths,
clears display access and disables automatic system theming. It calls the actual
`decwidgets render` with each manifest's builtin references and theme selection,
then positions the resulting PNG pixels over the artwork at the declared anchors.
Assertions check references, placement bounds and non-overlap at 1920 × 1080.
Pillow does not simulate bar, dock or window chrome. The package gallery images
are labelled native-widget compositions, **not desktop screenshots**.

Evidence (all four visually inspected):

- [Kiln Court dark](rice-batch/kiln-court-dark.png) and [light](rice-batch/kiln-court-light.png)
- [Tidal Loom dark](rice-batch/tidal-loom-dark.png) and [light](rice-batch/tidal-loom-light.png)

The eight individual native widget PNGs are retained alongside those compositions.
Both packages pass CLI validation without warnings. The existing golden package
suite includes both designs and checks manifest equivalence, validation and apply.
Existing all-theme contrast and palette tests include both automatically. An
additional token check across all ordinary text surfaces, secondary/faint text,
bar text and accent text gives minimum default contrast ratios of 5.53 / 5.22
(Kiln dark/light), and 5.98 / 4.70 (Tidal dark/light).

## Follow-up ideas

- Capture the full bar/dock/caption silhouette in a private native desktop during
  integration; this batch's visual evidence covers native widgets and artwork.
- Verify placement and taskbar balance on physical multi-monitor and ultrawide
  layouts, including scaled outputs and smaller laptop screens.
- Consider engine-level mode-aware wallpaper alternatives for all packages rather
  than adding theme-specific code.
- Explore a restrained monochrome ceramic companion and a warmer woven variant
  as separate future studies, without expanding this two-theme batch.

No live configuration was edited, binaries installed, desktop processes restarted,
rdwm altered, or real PAM authentication invoked. Production lock behavior is
unchanged. No merge or push is part of this delivery.

## Validation record

- `cargo test --locked --offline --all-features -- --skip pam_starts_for_the_current_user`:
  1,454 passed (1,430 library + 24 integration), zero failed, four ignored,
  one deliberately filtered current-user PAM-start probe. Private appearance
  config and `DECEBAL_SYSTEM_AUTOMATIC=0` were used.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`:
  passed. `cargo fmt --check` and staged diff whitespace checks passed.
- Default-feature `decwidgets` and `decthemectl` built successfully. The render
  reproduction completed eight native widget renders, four compositions and both
  CLI package validations without errors or warnings.
- After final manifest whitespace cleanup, the existing golden package/apply test
  was rerun to confirm the compiled manifests match the final files.

The all-features build includes the existing test-auth feature for tests only;
no binary from that build was installed or deployed. No production PAM source or
build feature defaults were changed.
