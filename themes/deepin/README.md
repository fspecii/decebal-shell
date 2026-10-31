# Deepin for Decebal

A source-backed port of the **Deepin Desktop Environment** (DDE 25) look: DTK light and dark palettes, the dock as a full-width
translucent strip (the shared taskbar, on any edge) with a centred launcher, tasks, tray and clock, and 50 px window titles. Light and dark
use DTK's own values; every number is traced in `theme.toml` comments and in
[`docs/desktop-ports/deepin`](../../docs/desktop-ports/deepin/README.md).

**Not a pixel replica.** What is and is not done, honestly:

* **Window frame**: the manifest asks for rdwm's own `deepin` (light) and `deepin-dark` styles, measured from deepin-kwin's Chameleon decoration
  (rdwm commit `952457b`), through the mode-specific window-style contract. The engine proves the style against the *running* rdwm; if that
  rdwm does not list it the fallback `adwaita-light` / `adwaita` is applied with a persistent Settings warning. The fallbacks are **not**
  Chameleon. No rdwm built from `952457b` was run for this port, so the Chameleon rendering itself is not captured here.
* **Panel**: the shared taskbar at 48 px (Deepin's dock is 56, dconfig default 48); no fashion (floating) mode, no multitasking-view overview.
* **Launcher**: no Deepin launcher style; the default launcher is used. **Widgets**: none placed; the Memory Monitor is offered.
* **Blur**: the dock's 0.4 translucency is drawn, the backdrop blur is not.
* **Icons and cursors**: not part of the package, never selected by it. Bloom (what the toolkit selects) is a pinned entry in
  `assets/desktop-ports/deepin/asset-packs.toml` for the shell's asset installer, recommended for this theme; Deepin's own `nirvana` icons are
  DCI containers with only a maintainer conversion recipe (`docs/desktop-ports/deepin/DCI-CONVERSION.md`), not a native converter.
* **Wallpaper**: none; two Wallhaven candidates with unknown licences are metadata in `assets/desktop-ports/deepin/wallpapers/`.

## Use

`decthemectl theme install themes/deepin`, then apply. Panels: bottom taskbar by default; Settings → Panels moves it to any edge and
toggles every applet. Install the matching icons and cursors, if wanted, with the asset installer
(`decthemectl assets install deepin-bloom` with `DECEBAL_ASSET_CATALOGS=assets/desktop-ports/deepin`).

Requires Decebal 0.3.0 (`[bar] applets`, `[branding]`, side taskbars) and the engine with the mode-specific window-style keys.

## Licence

The manifest and these notes are MIT (`LICENSE.txt`). Colour and metric values are facts read from LGPL/GPL sources
(dtkgui, dtkdeclarative, dde-shell, dde-services, deepin-kwin) and no source code from them is included. Artwork is not part of this package.
