# Signal Deck

A restrained orbital control room: split instrument bars, precise square geometry, technical typography and original survey artwork. Every surface follows the selected palette.

The theme defines geometry, typography and layout. All shell surfaces and shared widgets use semantic theme tokens; selecting another palette keeps this design intact. The included dark and light colors are defaults, not requirements. Font family lists fall back to installed Noto or generic families; no proprietary fonts are bundled.

The bundled wallpaper is original vector artwork, rendered at 3840 × 2160. Its editable SVG masters and reproducible rendering instructions are in ARTWORK.md. The default is `signal-deck-dark.png`; `signal-deck-light.png` is an optional light alternative. The engine currently does not switch wallpaper files automatically when changing color mode. Wallpaper art is independent from semantic shell colors.

Three shared built-in widgets provide clock, monitoring and media controls, with breathing room for windows. Placements are initial defaults; global widget preferences remain the user's choice across theme changes. No widget implementation belongs to this theme. The shell controls actual bar thickness; this package uses only supported bar placement and geometry.

Inspired by the composition and restraint documented in `docs/theme-research-organic-retro.md`, without copying third-party artwork. MIT licensed, including original artwork.
