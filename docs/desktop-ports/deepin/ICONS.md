# Deepin icons and cursors

## What Deepin selects

* The toolkit's XSettings defaults (dde-services `69fcd01`, `org.deepin.XSettings.json`) are `icon-theme-name "bloom"` and
  `gtk-cursor-theme-name "bloom"`, cursor size 24.
* The default *global theme* nirvana (`dde-appearance` 7190e59 `Global_Theme = nirvana.light`, deepin-desktop-theme a0c79e3) sets
  `IconTheme=nirvana` and `CursorTheme=bloom`. nirvana stores each icon as a DCI container; its stub `index.theme` declares one directory
  and `Inherits=bloom,Papirus`, so **Bloom is the freedesktop theme under both**.

## How the port provides them

* **Bloom icons and cursors**: pinned in `assets/desktop-ports/deepin/asset-packs.toml` as `deepin-bloom` / `deepin-bloom-dark` (GPL-3.0-or-later,
  `deepin-icon-theme` 5d11ba4). Nothing is copied into this repository; the shell's installer (`decthemectl assets install`, the Icons pane)
  fetches it, verifies the git tree and keeps the licence files. Both modes recommend `deepin-bloom`: in the native captures `bloom-dark`
  drew the dock's launcher glyph dim on the dark dock. The recommendation lives in `[[recommend]]`; the theme manifest has no `[icons]`
  (applying such a section overwrote a user's icon theme, measured), so a user's icon and cursor choices are never touched.
* **nirvana**: converted by a maintainer tool, not committed and not run automatically: [DCI-CONVERSION.md](DCI-CONVERSION.md) (recipe, limits,
  the one refused upstream file, and a request for a typed native conversion in the installer). Live dock capture with the converted theme
  chosen as the user's icon theme, and a sample of its icons (CC-BY-4.0, UnionTech):
  [evidence/nirvana-dock-100-dark-light.png](evidence/nirvana-dock-100-dark-light.png), [evidence/nirvana-icons-sample.png](evidence/nirvana-icons-sample.png).

## Measured through the real installer

`assets install deepin-bloom --source-dir <clone> --root <private root>`: tree `9baca758...` verified, 6,359 files and 7,501 links installed;
22 links that dangle in upstream itself (`uos-activator*` pointing at the absent `deepin-license-activator.svg`) are dropped with a warning;
one of 43 `index.theme` directories is absent upstream; `Inherits=Papirus` is not installed so missing names fall to hicolor and the user's
fallback theme (Adwaita by default). `assets verify`: ok. `assets preview bloom`: [evidence/installer-bloom-preview.png](evidence/installer-bloom-preview.png)
(real icons plus the arrow, hand, text, busy, move, resize, no-drop, crosshair and help cursors).

Coverage against twenty common application names: Bloom lacks `chromium`, `kitty`, `telegram`, `discord`, `spotify` and `obs`; they fall to the
fallback theme like any theme that lacks a name.

## Cursors

Upstream's own prebuilt Xcursor files (24, 32, 48, 64, 72, 96, 128, 256 px; `bloom/cursors` has 60 files and 116 alias links, `bloom-dark/cursors`
likewise): light is a black arrow with a white outline, dark is blue-outlined. A decoded 48 px sheet from an earlier packaging experiment:
[evidence/cursors-bloom-48.png](evidence/cursors-bloom-48.png). The installer's `preview` exercises them natively; no pointer was moved on a display.
