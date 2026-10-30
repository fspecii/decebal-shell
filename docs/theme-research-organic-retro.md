# Moss Atlas and Signal Deck: research and design

> Wallpaper policy update (2026-10-30): the generated wallpaper/SVG/package-shot
> experiments described below are historical and have been removed from visible
> packages. Use **Wallpaper → For this theme** and `data/wallhaven-curated.json`.
> Theme apply does not own wallpaper. Layout, palette and native widget references
> remain useful; old art paths/commands below are not current reproduction steps.


Research checked 29 October 2026. Votes below are indexed Reddit snapshots returned during research, not a claim about current live totals or an all-time ranking. These highly voted posts provide concrete community references; the resulting themes are original designs.

| Reference | Posted | Indexed votes | Useful idea |
| --- | --- | ---: | --- |
| [[bspwm] misty forest](https://www.reddit.com/r/unixporn/comments/i7565i/bspwm_misty_forest/) | 10 August 2020 | +1,194 | A calm coordinated forest environment; post details identify Polybar, Roboto/SF Mono, forest-night and an edited forest wallpaper. |
| [[bspwm] Forester](https://www.reddit.com/r/unixporn/comments/j4ze1p/) | 4 October 2020 | +1,329 | Forest-night colors, deliberate borders and coordinated application chrome. |
| [[BSPWM] Greeny Themey](https://www.reddit.com/r/unixporn/comments/iwfkqd/) | 20 September 2020 | +1,117 | Consistent restraint across a green desktop, Polybar and Iosevka. |
| [[XFCE] I really like retro themes](https://www.reddit.com/r/unixporn/comments/1q9cfau/xfce_i_really_like_retro_themes/) | 10 January 2026 | +805 | Indigo Magic and SE98 show that confident geometry and workstation character can carry a desktop. |
| [[Gnome] My desktop from 2004](https://www.reddit.com/r/unixporn/comments/eapuuo/) | 14 December 2019 | +1,559 | Period workstation instrumentation and a coherent technical personality. |

Reddit post text and indexed metadata were inspected. Direct preview image downloads for Misty Forest and the XFCE reference returned HTTP 403; visual image access was limited, so the ideas above derive from post descriptions and accessible metadata rather than a pixel-by-pixel visual analysis. No Reddit imagery or third-party theme files were included. The forest author's linked [configuration repository](https://github.com/ghifarit53/misty-forest-config) is a provenance reference, not a dependency.

## Moss Atlas

The interpretation is an organic workspace rather than another green color preset. A right-hand pill rail balances a quiet left widget column, rounded Adwaita window geometry and generous 14-pixel tiling gaps. A dock is omitted to leave the landscape open. Noto Sans provides calm readable text; semantic display and mono roles have explicit fallbacks.

Original layered terrain, a restrained sun and fine contour curves form an asymmetric composition. The upper left is kept clear for a shared clock and resource gauges. Media sits at bottom left. This geometry remains recognizable with a blue, purple or monochrome palette; the colors are not the design's identity.

## Signal Deck

The interpretation is a contemporary survey workstation, avoiding fake terminal text and decorative HUD labels. A split top/bottom instrument bar, restrained 4-pixel surface corners, compact Breeze titlebars and IBM Plex typography establish a precise silhouette. It has no dock. Compared with a terminal-style theme, normal readable application surfaces and actual shared graphical widgets carry the information.

Original orbital rings, tick marks, a subtle grid and two amber arcs occupy the left two thirds; a clear right column holds clock and system graph widgets with media at bottom right. The art contains no UI, labels or simulated metrics. Palette changes preserve its technical geometry and typography.

## Palette and widget contract

Both packages define every dark and light semantic color role. Theme manifests contain no custom widget definitions and no hard-coded rdwm focus/normal/urgent border colors; shared widgets resolve normal theme tokens. Each references only existing built-ins. Initial widget positions are seeds for the global widget system, not an instruction to reset a user's global choices when switching themes.

Wallpapers are independent art assets, as in existing theme packages. Dark is the default, with a supplied light alternative. The current engine does not automatically select a light wallpaper when changing color mode. An arbitrary palette recolors shell surfaces and widgets; it does not recolor wallpaper pixels. Font family fallback avoids a mandatory font installation. Bar thickness remains centrally controlled by the shell's supported pill layout.

## Verification

- Both packages passed `decthemectl theme validate themes/moss-atlas` and `decthemectl theme validate themes/signal-deck` using the existing development validator. No errors or warnings.
- Parsed manifests with Python `tomllib` and checked that every referenced built-in widget exists.
- Checked WCAG relative-luminance contrast in both default modes for text/background, text/popup, bar_text/bar, accent_text/accent and text_secondary/surface. Every checked pair exceeds 4.5:1 (minimum 5.58:1). This is a token-level check; arbitrary user palettes require their own contrast validation.
- Rendered original vector masters with `rsvg-convert` into four 3840 × 2160 PNG assets and visually inspected all four. Editable SVG masters and reproduction instructions are preserved in each package's root `ARTWORK.md`, because package policy accepts runtime raster images but rejects standalone SVG files.
- Desktop integration screenshots and live multi-theme/palette/widget interaction checks belong to the integrating agent. Neither package was applied to the running desktop during creation.
