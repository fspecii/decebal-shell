# Changelog

## 0.2.1 — Native Dux clock

- Dux Lavender 1.3 registers the original licensed Google Sans Flex variable
  font and renders its clock with source-derived axes and production grid sizing.
- Shared immutable font snapshots and a coalescing worker keep preparation off
  the event thread; four-font packages retain their previous rendering.
- Hot text-size changes below 100% invalidate cached date coverage correctly.
- The new package requires 0.2.1; older shells reject it rather than silently
  loading an unsupported profile. Full Dux replica parity remains incomplete.


## Unreleased

- Native Notes and Timer apps, linked Quick note/Timer widgets, launcher entries, private local persistence, conflict recovery and countdown notification after closing the window (docs/notes-timer.md).
- Independent Explorer/Finder inspired visual profiles for modern GNOME Files, selected in All apps and kept across theme changes (docs/file-manager-profiles.md).

- System Settings > Widgets can **create** a widget without writing code: pick a digital clock, analog clock, system metrics (CPU, memory, disk, network) or a note, name it, choose Small/Medium/Large and a screen position, check the live preview (the desktop's own painter with labelled sample data) and press Create. It saves a safe user widget file in your widgets folder, never overwriting an existing one, and adds it to the desktop. No commands are involved (docs/widgets-settings.md).
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
- Native Getting Started page in Settings and Spotlight, with wrapped instructions and links to the real controls.
- Explicit Settings destinations navigate the existing model without closing it or losing drafts; X11 reveals the window across workspaces.
- Next-login accessibility exports now drive effective text size/high contrast and reduced dock motion; Settings layouts accommodate 200% text.
- Dock reconciliation retries contention and surfaces a timeout; an absent btop process no longer produces a theme reload warning.
