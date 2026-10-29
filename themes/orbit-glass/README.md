# Orbit Glass

Orbit is a desktop command deck: a glass navigation rail on the right, minimal window chrome with controls on the right, and a measured 16/20-pixel inner/outer window rhythm. Adwaita Sans with monospace window titles provides a precise typographic voice; installed Inter/Adwaita Mono fallbacks keep it usable across machines. The right rail is the only shell navigation surface; the bottom dock is disabled at the package level to avoid running an unused dock process.

The original eclipse-field artwork leaves the right work area quiet. Palette selection remains independent of desktop layout. No widget is tied to Orbit; global widget choices and positions remain intact when switching themes. The manifest deliberately supplies no widget placements.

The theme selects matching dark/light artwork on a full apply. Later mode changes follow the packaged variants while preserving a custom wallpaper selected by the user. `ARTWORK.md` contains complete original SVG source for both 4K PNG assets; `LICENSE.txt` applies to the package. Screenshots show actual running applications on a private desktop, not UI painted into the wallpaper.

Glass requires a compositor and the shell's shared material implementation. Alpha and background blur alone are not described as liquid refraction. See `docs/theme-research-glass-paper.md` for the reference research, design rationale, precise validation and current material limitations.
