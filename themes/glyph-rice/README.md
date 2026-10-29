# Glyph Rice for Decebal

A data-only port of [Nebelung-cloud/glyph-rice](https://github.com/Nebelung-cloud/glyph-rice), inspected at revision `39a2a097300bd41e2927706e85bac0f9f8d57d8b` (2026-10-29). Reference files: README, LICENSE, Theme.qml, bar/Bar.qml, home/Home.qml, components/Wall.qml; desktop, quick and deck screenshots. This is distinct from Decebal's existing Nothing Dot theme.

## Mapping

Theme.qml's dark/light `bg`, `s1`, `s2`, `s3`, `grey`, `fg` and red map directly to the background, panel, surface, border, secondary text, foreground and accent tokens. Window borders use the neutral `s3` token; red is reserved for actions/alerts. The left vertical pill bar replaces Glyph's rail; there is no dock upstream, so the dock is disabled. Border-only decorations, 20 px rounding, opaque panels and round cards follow the reference. Light mode uses upstream's exact inverted greys and unchanged red.

The original static 1920×1080 wallpapers reinterpret the default sparse grid and lower-right halftone orb, without copying screenshot pixels. `wallpaper.dark/light` selects the matching artwork. Wallpaper colour extraction is disabled so application restores this palette. The screenshot is an **isolated widget composition**, not evidence of a running Glyph/Decebal desktop.

Optional package widgets: `glyph-home` (time/date/greeting), `glyph-system` (live CPU/memory/disk/history), and `glyph-player` (MPRIS metadata and play/pause). All use semantic tokens and safe actions. Applying the theme selects these widgets. If your current layout is customised, Settings asks whether to keep it or use Glyph’s layout before applying the theme. Keeping your layout preserves its options and positions. These cards are also available through Settings → Widgets.

## Fidelity limitations

Upstream highlights focused window borders with `fg`; this port retains Decebal's shared neutral border token in both focus states because package rdwm border overrides are not variant-aware. Decebal's existing Caelestia rail has different grouping, icons, proportions and workspace presentation. Its control center, launcher, dashboard and notifications retain their own layouts. This package does not implement Glyph's animated edge OSD, wave wallpaper transitions, dot sliders, circular Cava matrix, earbuds integration, lock screen or per-app configs. The static orb does not animate or adapt its geometry to aspect ratio; fill crops on other ratios.

Font chains request Space Grotesk, Space Mono and Doto, with installed-font fallbacks; no fonts are installed or bundled. The current widget text renderer uses the UI font role for all text, so the large clock is ordinary type rather than Doto even when the display role is available. System graphs are continuous lines rather than dot matrices. The player button is a static play icon (it toggles playback); media values use explicit unavailable fallbacks. Upstream's “one live red thing” is approximated: shell focus/selection or simultaneous alerts can introduce more red. No custom shell renderer was added.

## Licence and attribution

Upstream Glyph is copyright its contributors, licensed GPL-3.0; its full licence is preserved in LICENSE.txt. The port's manifest, widget definitions and original wallpaper interpretation are distributed under GPL-3.0-only. No upstream font or third-party screenshot is redistributed. Nothing, Nothing OS and Glyph are trademarks of Nothing Technology Ltd. This fan port is not affiliated with Nothing or endorsed by upstream.

## Validation

Run `cargo test --test glyph_rice` for package safety/packing and isolated application-plan checks (both palettes/artwork, explicit custom-widget preservation, bar/dock). `cargo test --lib theme::engine::golden` includes built-in registration and package checks. `decthemectl theme validate themes/glyph-rice` validates without installation. `python tests/glyph-rice-render.py /tmp/glyph-validation` regenerates six actual widget PNGs and two labelled compositions using temporary config and no display (requires Pillow). Actual widget PNGs can also be rendered with `decwidgets render themes/glyph-rice/widgets/glyph-home.toml --png /tmp/glyph-home.png` under private DECEBAL_CONFIG/DECEBAL_THEMES directories selecting this theme. Never run theme apply against the live desktop for validation.
