# Native X11 glass materials

NextKDE and Orbit Glass use screen-registered backdrop sampling, rounded edge refraction, restrained directional highlights, subtle grain and a softened background. Foreground text and icons are painted independently and remain sharp. This is an original Rust implementation; no upstream NextKDE/KWin shader code is included.

The X11 backend reads the root wallpaper pixmap and redirected windows below the surface through XComposite. It never reads the composited root, which would feed the panel's own image back into itself. Only visible material bounds are sampled: dock headroom, unused popup canvases and pointer bridges stay transparent. Unsupported servers or unavailable background pixmaps fall back to tint/compositor blur rather than presenting an invented backdrop.

Theme metric keys:

| Key | Meaning |
|---|---|
| `glass_refraction` | Rounded bezel displacement, 0–32 pixels; zero disables native material. |
| `glass_edge` | Bezel width, 2–40 pixels. |
| `glass_highlight` | Directional rim intensity, 0–1. |
| `glass_grain` | Small deterministic material grain, 0–0.03. |
| `glass_blur` | Background softening radius, 0–24 pixels. |

Existing `bar_alpha`, `dock_alpha` and widget semantic colour alpha provide the tint. Widget definitions use the same material without theme-ID checks. The native path sets `_DECEBAL_GLASS_NATIVE=1` and disables external blur on that surface to avoid double-filtering. The fallback Picom rules must retain their unconditional default-off rule.

Backdrops refresh at a bounded 250 ms cadence; foreground input and animations retain their normal repaint cadence. Background-only refresh reuses the foreground drawing, avoiding font parsing on every material update. Crops, root-tree traversal and caches are bounded. Captured pixels remain in memory and are never logged or saved by the renderer. This is not compositor-frame-synchronous KWin refraction; rapidly changing content behind a panel can update less frequently than the foreground.

`tests/e2e/glass.sh` uses generated backgrounds and actual X11 windows. Its patterned-bezel comparison distinguishes native distortion from ordinary straight-alpha tint, while other assertions cover root-background changes, unused-canvas sharpness, fallback flags and compositor restart. Pure tests verify an unwarped centre, crisp opaque foreground, screen registration when the panel moves, and disabled-material identity.
