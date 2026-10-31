# openKylin (Classic 启典)

An adaptation of the real UKUI desktop design from the openKylin project (see [ATTRIBUTION.md](ATTRIBUTION.md)), built on the
shared Decebal panel framework (`min_shell_version = "0.3.0"`).

* **Global theme:** `Classic` (square corners everywhere, flat solid `#3790FA` accent). Light and dark palettes are derived from UKUI's resolved token tables; the mapping is
  documented in `docs/desktop-ports/openkylin/palette.md`.
* **Panel:** classic UKUI taskbar on the bottom edge (it can sit on any edge). Every applet in the shared registry can still be turned on or off, and the edge changed, in
  Settings → Panels; the package only supplies defaults and never overwrites your choices.
* **Window decoration:** the measured native `openkylin` / `openkylin-dark` rdwm styles (38 px header, 30 px buttons, radius 12) when your running window manager lists them,
  otherwise an honest fallback with a persistent warning (see the port documentation).
* **Branding:** the start button is `icon:kylin-startmenu` from your icon theme; without a UKUI icon pack it shows the
  neutral grid.
* **Suggested separately (never forced):** icons `ukui-icon-theme-classical`, cursor `dark-sense`, photographs from the Wallpaper → For this theme list.
* **Fonts:** `Noto Sans CJK SC` then `Noto Sans` (your system fonts; none is bundled).
