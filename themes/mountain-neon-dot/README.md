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
window buttons. Photographic wallpaper is selected separately from the theme.

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

## Retired artwork

The earlier imagegen mountain assets have been removed from the package and its
apply metadata at the user's request. No generated image or photographic fallback
is bundled. The user screenshot remains a composition reference only. Manifest
and widgets retain their original MIT attribution; no third-party photo license
is inferred. Existing native `dot-text` is interface rendering, not wallpaper art.

## Validation and limits

See `docs/theme-ports/mountain-neon-dot/` in the repository for actual native
sample/empty widget renders and four labelled compositions. These are headless
widget pixels on the theme background colour, **not full desktop captures**. Worker screenshots
that baked in live host metrics were replaced. Reproduce with private config:

```
DECEBAL_CONFIG=/tmp/private-theme-config DECEBAL_SYSTEM_AUTOMATIC=0 \
 DECEBAL_THEME_EVIDENCE="$PWD/docs/theme-ports" cargo test --test theme_ports
python tests/theme-ports-compose.py mountain-neon-dot
```

The test validates safe packages, reproducible packing, preserved independent wallpaper choices,
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

## Wallpaper choices

Open **Wallpaper → For this theme** and select the separately curated Wallhaven
photograph: dark [ey7prw](https://wallhaven.cc/w/ey7prw), light
[2kqkmy](https://wallhaven.cc/w/2kqkmy). The source catalogue at
`data/wallhaven-curated.json` records uploader versus unverified author, unknown
license, source links and crop/readability notes. These links are recommendations,
not package-owned remote fields or a redistribution license.

This package sets no wallpaper image, scaling mode, background colour, slideshow
or download action. Theme apply and mode changes preserve the independent
Wallpaper selection; no automatic downloads or generated artwork fallback.
Former generated artwork and packaged screenshots containing it were removed.
The palette, layout, captions and native widget definitions remain unchanged.

Full-resolution native QA replaced the original cloudy light recommendation with
a sparse photographic snowfield: black homelab text was lost over dark clouds.
The dark choice remains alpine; light favours legibility. No widget was moved.
