# Tidal Loom

Original Decebal design and procedural artwork, copyright 2026 Decebal contributors,
licensed under the repository MIT license. No downloaded imagery, logos, third-party
theme files or bundled fonts. Fonts are named only and fall back to Inter/Noto Sans.

The desktop uses the left vertical rail only. The bottom dock is explicitly disabled,
so it retains no dock process.

Apply with `decthemectl theme apply tidal-loom`. The package supplies dark and light
semantic colors. Both artworks are bundled with `wallpaper.dark/light`; mode changes follow
engine-owned artwork while preserving a custom wallpaper. Independent palettes recolor shell and
widgets, not wallpaper pixels.

The two desktop widgets are optional layout seeds using existing safe builtin
widgets. Untouched theme layouts follow theme switches; customized layouts use the
shell's Keep / Replace / Cancel choice. Disable them in Settings → Widgets for a
clear desktop. No custom commands, scripts or network providers are introduced.

`screenshots/main.png` and `screenshots/light.png` are labelled compositions of
actual native headless widget pixels with sample data over the original wallpaper,
not full desktop captures. Reproduce from the repository with
`python tools/rice-batch-art.py`, build `decwidgets` and `decthemectl`, then run
`python tests/rice-batch-render.py`. The scripts live outside the runtime package.
