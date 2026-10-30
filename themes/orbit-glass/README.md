# Orbit Glass

Orbit is a desktop command deck: a glass navigation rail on the right, minimal window chrome with controls on the right, and a measured 16/20-pixel inner/outer window rhythm. Adwaita Sans with monospace window titles provides a precise typographic voice; installed Inter/Adwaita Mono fallbacks keep it usable across machines. The right rail is the only shell navigation surface; the bottom dock is disabled at the package level to avoid running an unused dock process.

Palette selection remains independent of desktop layout. No widget is tied to Orbit; global widget choices and positions remain intact when switching themes. The manifest deliberately supplies no widget placements.

The former eclipse artwork and its SVG source are removed. `LICENSE.txt` remains
applicable to package configuration; no photograph is relicensed under it.

Glass requires a compositor and the shell's shared material implementation. Alpha and background blur alone are not described as liquid refraction. See `docs/theme-research-glass-paper.md` for the reference research, design rationale, precise validation and current material limitations.

## Wallpaper choices

Open **Wallpaper → For this theme** and select the separately curated Wallhaven
photograph: dark [5y3q31](https://wallhaven.cc/w/5y3q31), light
[lygg6r](https://wallhaven.cc/w/lygg6r). The source catalogue at
`data/wallhaven-curated.json` records uploader versus unverified author, unknown
license, source links and crop/readability notes. These links are recommendations,
not package-owned remote fields or a redistribution license.

This package sets no wallpaper image, scaling mode, background colour, slideshow
or download action. Theme apply and mode changes preserve the independent
Wallpaper selection; no automatic downloads or generated artwork fallback.
Former generated artwork and packaged screenshots containing it were removed.
The palette, layout, captions and native widget definitions remain unchanged.
