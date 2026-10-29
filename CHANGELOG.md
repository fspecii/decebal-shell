# Changelog

## Unreleased

- The bar's weather widget is clickable in every theme: macOS, Windows 11 taskbar and Caelestia pill bars open a weather popup (conditions, feels-like, humidity, wind, location, today's high/low, 2-day forecast, Refresh row, offline / no-data states). It reads the same wttr.in answer as before, nothing new is fetched. The pill bars and the taskbar gained the weather icon, which they never showed.
- Activity Monitor (`decmon`): a process manager and performance monitor, opened by the bar's agents icon (popup) or as a window; End task / Force quit with guards (docs/procmon.md).
- The bar's coffee-cup icon is clickable: it turns Stay Awake off (hover shows how long it was on, right click offers 30 min / 1 h / 2 h timers).
- declock: a themed X11 screen locker (blurred backdrop, clock, avatar, password field with dots, wrong-password shake and back-off) authenticating through PAM; see docs/lockscreen.md.
- System Settings: one **Appearance** pane (tabs Theme, Customise, Wallpaper, All apps) replaces the separate Appearance, Theme, Icons, Wallpaper and Desktop & Dock panes; Configuration is now **Config & Presets**. Old names (`decsettings theme|icons|wallpaper|desktop`) open the new place. Nothing stored or on the command line changed. Design: docs/settings-design.md.
- decsettings scrolls smoothly: the page is no longer rebuilt (27 ms on the Theme pane) for every wheel notch or mouse move, off-screen cards are not painted, and the scroll offset is a whole pixel.
- Debug logs: one file per program under `~/.local/state/decebal/logs/`, `DECEBAL_LOG=debug`, `decthemectl log` (docs/debugging.md).
- Colour palettes separate from themes (Catppuccin, Rose Pine, Gruvbox, Tokyo Night, Everforest, Kanagawa, Nord, Dracula, Solarized, Monochrome, your own), `decthemectl palette`; the palette gallery is in Appearance > Customise > Colours (it replaces the old Colour scheme tiles).
- New themes: Nothing Dot, Nothing Light, Brutalist, Terminal Minimal, Terminal Amber (docs/themes.md).
- Renamed from macshell to Decebal; programs are now `dec*`, old names and settings keep working (docs/RENAME.md).
- Theme engine with data-only theme packages, custom widgets, per-theme fonts and presets.
- Match-wallpaper colours, system-wide theming (GTK, Qt, terminals, Omarchy), icon themes, Wallhaven picker, config editor.
- Caelestia layouts (left, right, top, bottom, split), workspace-switch overlay, 15 window decoration styles (rdwm).
- Control Center in every theme: a quick-settings button in the Windows taskbar tray, `decutil control-center toggle` (works in any theme, with no bar), the `control-center` hot-corner action, and a theme / `decebal.json` `control_center` section (`enabled`, `position`, `style`, `tiles`) with a pane in Settings (Desktop & Dock).
- decnotify idle memory cut from 11 MB to about 6 MB.
