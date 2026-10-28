# Changelog

## Unreleased

- System Settings: one **Appearance** pane (tabs Theme, Customise, Wallpaper, All apps) replaces the separate Appearance, Theme, Icons, Wallpaper and Desktop & Dock panes; Configuration is now **Config & Presets**. Old names (`decsettings theme|icons|wallpaper|desktop`) open the new place. Nothing stored or on the command line changed. Design: docs/settings-design.md.
- decsettings scrolls smoothly: the page is no longer rebuilt (27 ms on the Theme pane) for every wheel notch or mouse move, off-screen cards are not painted, and the scroll offset is a whole pixel.
- Renamed from macshell to Decebal; programs are now `dec*`, old names and settings keep working (docs/RENAME.md).
- Theme engine with data-only theme packages, custom widgets, per-theme fonts and presets.
- Match-wallpaper colours, system-wide theming (GTK, Qt, terminals, Omarchy), icon themes, Wallhaven picker, config editor.
- Caelestia layouts (left, right, top, bottom, split), workspace-switch overlay, 15 window decoration styles (rdwm).
- decnotify idle memory cut from 11 MB to about 6 MB.
