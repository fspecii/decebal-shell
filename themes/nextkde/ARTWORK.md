# Original NextKDE reference-inspired wallpaper source

Copyright (c) 2026 ambsd. MIT; see LICENSE.md. These are original gradient compositions, not copied upstream assets. The restrained slate-to-aqua field follows the reference desktop screenshot, keeping the wallpaper quiet behind transparent cards. Dark and light differ gently rather than introducing unrelated imagery. Render with `rsvg-convert -w 3840 -h 2160 source.svg -o glass-dark.png`. SVG source is embedded because theme packages accept raster image files only.

## Dark

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 3840 2160"><defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop stop-color="#2d405f"/><stop offset="1" stop-color="#4da4bd"/></linearGradient><radialGradient id="air"><stop stop-color="#b1e4ec" stop-opacity=".065"/><stop offset="1" stop-color="#b1e4ec" stop-opacity="0"/></radialGradient></defs><path fill="url(#sky)" d="M0 0h3840v2160H0z"/><ellipse cx="2800" cy="1800" rx="2000" ry="1800" fill="url(#air)"/></svg>
```

## Light

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 3840 2160"><defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop stop-color="#465e7b"/><stop offset="1" stop-color="#73bfd0"/></linearGradient><radialGradient id="air"><stop stop-color="#b1e4ec" stop-opacity=".065"/><stop offset="1" stop-color="#b1e4ec" stop-opacity="0"/></radialGradient></defs><path fill="url(#sky)" d="M0 0h3840v2160H0z"/><ellipse cx="2800" cy="1800" rx="2000" ry="1800" fill="url(#air)"/></svg>
```
