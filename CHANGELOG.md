# Changelog

## Unreleased

- Renamed from macshell to Decebal; programs are now `dec*`, old names and settings keep working (docs/RENAME.md).
- Theme engine with data-only theme packages, custom widgets, per-theme fonts and presets.
- Match-wallpaper colours, system-wide theming (GTK, Qt, terminals, Omarchy), icon themes, Wallhaven picker, config editor.
- Caelestia layouts (left, right, top, bottom, split), workspace-switch overlay, 15 window decoration styles (rdwm).
- Control Center in every theme: a quick-settings button in the Windows taskbar tray, `decutil control-center toggle` (works in any theme, with no bar), the `control-center` hot-corner action, and a theme / `decebal.json` `control_center` section (`enabled`, `position`, `style`, `tiles`) with a pane in Settings (Desktop & Dock).
- decnotify idle memory cut from 11 MB to about 6 MB.
