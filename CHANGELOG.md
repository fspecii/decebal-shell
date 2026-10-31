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

- The launcher (`declaunch`) publishes an AT-SPI2 tree in every style (Launchpad, Dux, Windows, GNOME, KDE, Xfce, BlackBerry, UKUI): the search field as editable text, app lists and category / page tabs with stable ids bound to desktop ids, and open / pin / unpin / navigate actions that run the launcher's own paths. Style-owned Kickoff controls and physical Orca acceptance are not covered yet; see docs/accessibility.md.
- Desktop widgets (`decwidgets`), the dashboard (`decdash`) and the on-screen display (`dechud`) now run natively on Wayland as well as X11 and detect the session themselves (`DECEBAL_BACKEND=x11|wayland` forces one). Widgets are one wlr-layer-shell surface per card on the bottom layer, with clicks, scrolling, live data and theme changes as on X11; the dashboard is a card anchored under the bar that closes on Escape or a click outside; the HUD is a click-through card above the bottom edge. A compositor without layer-shell falls back to X11 (XWayland) when there is one.
- The Disk ring and the `disk.*` widget data read the filesystem that holds your data when `/` is a small read-only image (OSTree, bootc, composefs). On such systems they always showed 100 %.
- Keyboard & Input has a native Wayland section, "Touchpad, mouse & keyboard (rdwm)": natural scrolling, tap to click, left-handed buttons, pointer speed and key repeat, saved to rdwm's `[input]` and applied to every libinput device on reload. The `xinput`/`xset` rows only reached X11 apps, so on Wayland their "not installed" rows and the red "No X server" error are hidden. Needs an rdwm whose schema lists `[input]`; older ones show nothing extra.
- Wayland windows paint an already configured first frame before blocking for further events. A private quiescent-compositor regression proves the old stall and fixed first buffer commit; accessibility acceptance now waits for its owned compositor output and records protocol diagnostics.
- Settings wraps long labels and URLs within narrow cards without reducing text size. Keyboard focus follows controls when content loads, while explicit scrolling stays under user control; oversized rows no longer oscillate between redraws.
- Taskbar Start sends the clicked button's position and panel edge to the launcher, including side panels, fractional scale and floating gaps.
- Icon asset packs can convert supported Deepin DSG/DCI containers natively into PNG themes with pinned source provenance and visible per-icon refusals. Conflicting source files are refused before conversion; palette/multilayer containers remain unsupported. This adds converter infrastructure, not an automatic Nirvana download (docs/desktop-asset-packs.md).
- Installed theme packages (including their parents, logos and fonts) are found wherever `make install` or a package put them: `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`), after your own `~/.config/decebal/themes`. `PREFIX=/usr/local` and `PREFIX=$HOME/.local` installs used to be invisible; `DECEBAL_SYSTEM_THEMES` still names the one folder to use. Theme previews see the same folders as the shell (docs/theme-engine.md).
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
