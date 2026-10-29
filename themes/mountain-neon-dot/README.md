# Mountain Neon Dot

A Decebal interpretation of the user-supplied mountain desktop reference
`5cb42945-93ba-4f85-b386-ba20dd4a15b9.png`, inspected 2026-10-29. No upstream
project/author identity was supplied or inferred. The reference is used for
composition only and is not redistributed. No logo or screenshot pixels copied.

## Mapping and honest data

Dark teal snow mountain, magenta/cyan island underlines, transparent stacked dot
hours/minutes and a small dot date at left, homelab at upper right, native system
readings below on a restrained translucent backing for daylight readability. No dock. Six theme widgets use the safe DSL, theme semantic
colors and existing data sources. Original `dot-text` uses a seven-row alphabet,
so it works without installing a dot font. Desktop applications retain Breeze
window buttons. Both mode-specific wallpapers are selected by the package
engine when it owns the artwork; custom wallpapers are preserved by mode edits.

The three top islands are **widgets**, not a real bar or system tray. The left
opens the launcher; the right opens the dashboard. Center time/date/weather and
right volume/battery values come from shared providers. Missing values show —,
not fake Wi-Fi strength, muted status, charge or temperature. Hot corners still
open overview/control center. Islands do not reserve screen space; tiled or
maximized apps may cover them. A future shared island-bar feature belongs in a
separate widget/shell task, not hard-coded in this package.

Homelab has no health provider in the current DSL. The six named services are
reference-shaped **unconfigured templates**, not detected installations. Every
row says NOT CONFIGURED and the header says MONITORING NOT CONFIGURED. There
are no network requests, commands, invented successes, latencies or availability
percentages. Configure monitoring only after a shared opt-in provider exists.
The system panel uses actual shared username/hostname, uptime, CPU, memory,
disk and network-rate sources with explicit unavailable fallbacks.

## Artwork provenance

Both original wallpaper PNGs are imagegen outputs recovered byte-for-byte from
the stopped Mountain worker, as requested by the user. They match preserved
imagegen output files in generation directory
`01a18988-d709-7d83-8aa4-512433de6834`. The exact generation prompt was not retained
with the worker's package; do not claim a real photograph, location or artist.
No third-party mountain photo was downloaded. The light asset is a separately
generated daylight companion, not a recolored copy of the reference screenshot.

- `wallpapers/dark.png`, 1672 × 941, SHA256
  `5c236eea8414340c1c59f60f551dd833bf6b5303ce6990e9f0bc1e516e74e579`
- `wallpapers/light.png`, 1672 × 941, SHA256
  `88918fd9e43df330f8e8e58c6b20df6d242dab723d266e8bb41091e68d1d1113`

Manifest/widgets are MIT, copyright 2026 Decebal contributors. Generated art is
supplied with this package for reuse under the same terms; no external image or
font licenses are claimed. No fonts are bundled or installed. The generated
mountain differs from the reference; a screenshot-faithful photographic match
would require the original photograph and its reuse permission.

## Validation and limits

See `docs/theme-ports/mountain-neon-dot/` in the repository for actual native
sample/empty widget renders and four labelled compositions. These are headless
widget pixels over wallpaper, **not full desktop captures**. Worker screenshots
that baked in live host metrics were replaced. Reproduce with private config:

```
DECEBAL_CONFIG=/tmp/private-theme-config DECEBAL_SYSTEM_AUTOMATIC=0 \
 DECEBAL_THEME_EVIDENCE="$PWD/docs/theme-ports" cargo test --test theme_ports
python tests/theme-ports-compose.py mountain-neon-dot
```

The test validates safe packages, reproducible packing, dark/light artwork,
custom-layout Ask/Keep/Replace plans without writing, and visible unavailable
states. Native text bounds and non-overlap are checked at 1920 × 1080. Pixel
layout seeds are not responsive: smaller/scaled screens need placement editing.
Transparent light text over dark imagery and dark text over daylight is checked
visually, but arbitrary wallpapers/palettes can reduce readability. Package
widgets remain optional via Settings → Widgets and existing layout choices.

Validation checkpoint: two port contract/native-render tests passed, five golden
package/application tests passed, and the dot-renderer pixel/width test passed.
All 24 native widget PNGs (six × two modes × sample/empty) were rendered; four
compositions passed position/non-overlap checks and were visually reviewed.
CLI package validation reported no errors or warnings. The light system card
received a translucent backing after QA found text over bright rock unreadable;
the homelab moved right into clear sky. No live provider was polled for evidence.
