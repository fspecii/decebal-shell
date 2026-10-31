# Attribution

The package manifest and these notes are original Decebal content under the MIT licence of this repository (`LICENSE.txt`).

The values in `theme.toml` (colours, pixel metrics, opacities, radii) were read from, and are credited to:

* **libadwaita** 1.10.0, `c9ab118612eee92324143783c4641c98eaded3f0`, © The GNOME Project, LGPL-2.1-or-later.
* **GNOME Shell** 51.1, `33da06854e229c256afd5872fedd83c28cbfa176`, © The GNOME Project, GPL-2.0-or-later with the
  `gnome-shell-sass` stylesheets under LGPL-2.1-or-later.

No stylesheet, source file, icon, font or wallpaper from either project is included. "GNOME" and "Adwaita" are used only to name the design
being reproduced; this theme is not affiliated with or endorsed by the GNOME Project.

## Bundled font

`fonts/MaterialSymbolsRounded.ttf` is the exact upstream Material Symbols Rounded variable font (Google, Apache-2.0, full text in
`MaterialSymbolsRounded-LICENSE.txt`), byte-identical to the copy in `themes/dux-lavender`. It is bundled only because the native bar
(`bar.style = "dux"`, the only bar that honours item lists today) paints its item glyphs from it; it is registered inside the shell
process only. These are Material glyphs, not Adwaita symbolic icons: that mismatch is a documented framework blocker in
`docs/gnome-theme-ports.md`.
