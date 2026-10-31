# Plasma Layan for Decebal

A port of [Layan KDE](https://github.com/vinceliuice/Layan-kde) by Vince Liuice, a popular flat indigo Plasma global theme
(Layan dark and Layan-light colour schemes, translucent Plasma theme, Aurorae round window buttons, Kvantum widget style).

**Status: NOT READY.** Colours, translucency, caption look, Kickoff's logo, the applet set and the top panel are ported through the shared
panel contract (`[bar] applets`, `[branding]`, `min_shell_version = "0.3.0"`, shared commit `71e343413fdafcc6fde638d701b63c183512f1c8`).
Layan's upstream layout has a KDE global menu; Decebal has no exported application menus yet, so `app_menu` (which would draw fixed placeholder titles and a second logo) is not in the
default applet set; the focused window's real title stands in. See [`docs/plasma-theme-ports.md`](../../docs/plasma-theme-ports.md).

## Upstream

`vinceliuice/Layan-kde` at `a0b6a4956022276aee33309b2e07d1d0ef3db30c` (2025-11-27), `GPL-3.0` (`LICENSE`, "GNU GPL v3").
Inspected: `color-schemes/Layan.colors`, `LayanLight.colors`, `plasma/look-and-feel/*/contents/{defaults,layouts}`,
`plasma/desktoptheme/common/{translucent,solid}`, `aurorae/themes/Layan`, `Kvantum/Layan`, and the upstream
`fullscreenpreview.jpg`. Nothing is executed; `install.sh` was not run.

## What is ported

- All colour tokens recomputed from the two colour schemes (see `tests/plasma_theme_ports.rs`). Border and `text_faint` are
  derived as documented in the manifest; LayanLight's translucent inactive foreground is flattened on its Window colour.
- Translucency from the Plasma theme SVGs: panel fill 0.6, dialog fill 0.8.
- Layout script: one top panel of 1.78 grid units (about 32 px). Upstream lists Kickoff, global menu, panel spacer, tray,
  split digital clock and KRunner search. The flat top bar carries `launcher, window_title, tray, network, volume, clipboard, notifications, clock, search`; `app_menu` is off until a
  genuine DBus menu exporter exists.
- Aurorae buttons are round, on the right, in minimise-maximise-close order; rdwm's `adwaita` round style is the closest
  existing renderer. Corner radius 8 px is read from the upstream preview, not from source.
- The bottom dock appears in upstream's preview image but not in its layout script, so it is a documented companion.

Tiling policy is the user's **global** behaviour (layouts, master fraction, master count, smart gaps): this package deliberately sets none of
it, so applying or switching the theme never changes custom layout proportions. Only visual gaps (8 inner / 4 outer, KWin's tile
padding of 4) and the 1 px border are the theme's. KWin floats windows by default; Decebal's global tile-first behaviour is an
intentional difference.

Plasma's analog and digital desktop clocks are saved in the package (`widgets/`) but off; Layan's layout places no desktop widgets.

## Known differences

There are no exported application menus (see above); `desktop_label` is a neutral "Desktop". Tela (dark) / Tela-circle (light)
icons are recommended by upstream but are not installed or bundled; Breeze icons back them up. Layan-white-cursors and the
Kvantum widget style cannot be expressed. Upstream's light and dark variants are separate global themes with different icon
themes; one package cannot switch icon themes per mode.

## Licence

`GPL-3.0-only`, copied in `LICENSE.txt`; upstream `AUTHORS.txt` is retained. Layan is not affiliated with Decebal; names are
used for attribution. Upstream wallpapers are not bundled.
