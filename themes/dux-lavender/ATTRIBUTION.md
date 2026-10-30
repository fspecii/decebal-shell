# Dux replica source attribution

Authoritative local references inspected read-only,2026-10-30:

- [dux-shell87312075dd414a6f60691464bcd6c6a9bb242a7e](https://github.com/duxlabs-dev/dux-shell/tree/87312075dd414a6f60691464bcd6c6a9bb242a7e).
  StockThemes Purple dark/light tokens, Theme/SettingsSpec geometry and DBar/DDash/
  ControlCenter source are the replica contracts. Earlier46a12d9 inspection remains
  historical; its Purple tokens match the current source.
- [dux-qml-common43e05926fcdacc53675623169bcef686faa95301](https://github.com/duxlabs-dev/dux-qml-common/tree/43e05926fcdacc53675623169bcef686faa95301),
  pinned by the shell clone: shape/type/icon source and included font assets.
- [dux-pluginsa55bb40da2f70640600a520ea05eac9c3540f1af](https://github.com/duxlabs-dev/dux-plugins/tree/a55bb40da2f70640600a520ea05eac9c3540f1af):
  first-party Pomodoro/notepad/actions references, no arbitrary script/plugin runtime copied.
- Registryf3079becbf35f4f48d46879fa46cb867f15771f9 was inspected previously;
  no root licence. Third-party entries do not inherit shell licence and are not copied.

Shell/common/plugins software: **MIT**,Copyright(c)2025 Avenge Media LLC. Full shell
notice in LICENSE.md. Dux is credited as upstream, not endorsement or an official
Dux distribution. Native implementation is independently coordinated; package assets
are not permission to run upstream installers or replace accepted Settings.

## Included typography/icon assets

Google Sans Flex: SIL OFL1.1, fullOFL.txt. Static500/700 copied unchanged from
pinned common. Regular400 is an explicitly documented STATIC instance of upstream
variable regular at all source default axes. Embedded source copyright/name metadata
preserved; not a substitute typeface. The unsupported variable regular is not bundled.

Material Symbols Rounded variable font: exact upstream bytes, Apache2.0, full
MaterialSymbolsRounded-LICENSE.txt. Native owner registers package fonts only
inside the process and renders actual PUA glyphs; no global font install/fc-cache.
Source icon axes: FILL0,GRAD-25dark/0light,opsz24,wght400unfilled/500filled.
Detailed source paths, build tool/version, source/output hashes and licenses are in
[FONT-BUILD.md](FONT-BUILD.md). SoftwareMIT does not relicense the fonts.

## Artwork and reference images

Original README references actually inspected:

- Desktop:https://github.com/user-attachments/assets/203a9678-c3b7-4720-bb97-853a511ac5c8
- Dashboard:https://github.com/user-attachments/assets/a937cf35-a43b-4558-8c39-5694ff5fcac4

Those image pixels are reference only, not bundled/live UI. The illustrated mountain
wallpaper and album artwork have no separately verified redistribution rights here;
softwareMIT alone is insufficient. No generated wallpaper is substituted for the
upstream design. Wallhaven l3edjp/39zjq6 remain independent reviewed recommendations,
not theme defaults; image authors/licences remain unknown in the catalogue. Existing
Follow theme preference and custom-widget confirmation are preserved.
