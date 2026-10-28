# Recommended icon themes (Arch Linux)

decebal reads any freedesktop icon theme; see `docs/icons.md`. Nothing here is installed by decebal. Names are
those of the official repositories or the AUR (check with `pacman -Ss` / your AUR helper before installing; AUR
package names change).

| theme | package | notes |
| --- | --- | --- |
| Papirus (+ Papirus-Dark, Papirus-Light) | `papirus-icon-theme` (extra) | Broad app coverage, SVG; the safest all-rounder. |
| Adwaita | `adwaita-icon-theme` (extra) | The default fallback theme of decebal. |
| Breeze | `breeze-icons` (extra) | KDE look, many `@2x` folders. |
| Yaru | `yaru-icon-theme` (extra) | Ubuntu look, several colour variants. |
| WhiteSur (macOS Big Sur look) | `whitesur-icon-theme-git` (AUR) | Pairs with the macOS theme. |
| Tela, Tela circle | `tela-icon-theme-git` (AUR) | Flat, rounded; many colour variants. |
| Fluent | `fluent-icon-theme-git` (AUR) | Windows 11 look; use with the Windows theme / taskbar style. |
| Candy | `candy-icons-git` (AUR) | Gradient icons, good with dark themes. |
| Numix | `numix-icon-theme-git` (AUR) | Classic flat icons. |
| Qogir | `qogir-icon-theme-git` (AUR) | Flat, Material-like. |

Per-user install without a package: unpack or clone the theme into `~/.local/share/icons/<Name>/` (it must contain
an `index.theme`); decebal notices within a second and the Icons pane lists it. Cursor-only themes (Bibata, ...)
and symbolic-only ones (Adwaita's newer releases for some contexts) are listed greyed out: they have no application icons.

Quick try:

```
decthemectl icons list
decthemectl icons set Papirus-Dark
decthemectl icons status
decthemectl icons set auto          # follow gsettings / GTK again
```
