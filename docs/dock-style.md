# Dock style: macOS dock or Windows taskbar

`macdock` has two looks. `style = "mac"` (default) is the floating, magnifying dock. `style = "windows"` turns it into a
Windows 10/11 style taskbar: monitor wide, docked at the bottom (or top) edge, flat surface, no magnification, no bounce.
The change is live: edit the file, use the dock's right-click menu or the Desktop & Dock pane of `macsettings`; the surface
is resized and moved, its strut republished (so the window manager's work area follows) and the old look disappears
without a restart.

## Keys of `dock-settings.json`

All keys are optional; the file keeps its other keys when the dock or the settings app writes it. Keys are camelCase; the
snake_case spelling (`windows_align`) is accepted too. A value that is not one of the words below falls back to the default
(it never voids the rest of the file), and a file that does not parse keeps the current look.

| key | values (default first) | meaning |
|---|---|---|
| `style` | `"mac"`, `"windows"` | the look |
| `windowsAlign` | `"center"`, `"left"` | `center`: Windows 11 (rounded, 48 px, buttons in the middle of the bar). `left`: Windows 10 / classic (square, 44 px, line under running apps, pale search box) |
| `windowsLabels` | `"never"`, `"always"` | `never`: one icon button per app (windows combined). `always`: "never combine": one button with the window title per window |
| `windowsSearch` | `"box"`, `"button"`, `"none"` | search entry next to Start; opens macspot (`MACSHELL_SPOTLIGHT` overrides the command) |
| `windowsPosition` | `"bottom"`, `"top"` | edge of the taskbar (left and right are not drawn yet and give the bottom) |
| `windowsTheme` | `"auto"`, `"dark"`, `"light"` | `auto` follows the system light/dark setting |
| `transparency` | 0..1 | existing key; the taskbar uses a quarter of it (0.4 gives a 90% opaque bar) |

Heights scale with `Xft.dpi / 96` (or `MACSHELL_SCALE`): 48 px (center) / 44 px (left) at 96 dpi.

`macshell.json`: `{"bar": {"enabled": true | false | "auto"}}`, see "macbar hiding" below.

`macthemectl style windows|macos|caelestia` writes `style` (and `windowsAlign`) here together with the theme and `bar.enabled`; see
[theming.md](theming.md#shell-styles-macos-windows-caelestia).

## What the taskbar does

* **Start**: a four-pane logo drawn in code; opens Launchpad (`maclaunch`, `MACSHELL_LAUNCHPAD` overrides). **Search**: opens macspot.
* **Task buttons**: pinned apps (the same `dock-pinned-macos.json`, Launchpad and Trash are not shown) and the running apps that are not pinned. Running: a small pill under the icon, the focused app's pill is longer and accent coloured (classic look: a line under the whole button). Hover highlight.
  Click: not running -> start it; running -> focus (restoring it if minimised); the focused app's button -> minimise it (several windows of one app: cycle through them). Middle click: new instance. Right click: menu (windows of the app, app name = new window, *Pin to taskbar* / *Unpin from taskbar*, *Open file location*, *Minimize*, *Close window* / *Close all windows*). Drag a pinned button to reorder; saved on release.
* **Too many windows**: buttons shrink to a minimum, then the row scrolls with the wheel (edges fade).
* **Tray**: the status items of macbar (window layout under rdwm, Bluetooth, Wi-Fi/Ethernet, sound, battery) with the same popups (wheel over sound = volume, middle click = mute), then the clock (time over date) which opens the same calendar, then the show-desktop sliver (minimises every window of the workspace; the next click restores them).
* **Dock menu**: right click on an empty part of the taskbar (or of the macOS dock panel): *Dock Style: macOS / Windows* (the current one is greyed), alignment, window titles, settings. In the macOS style every icon menu also ends with *Dock Style: Windows*.

The tray reuses macbar's code instead of copying it: `menubar::embed` (`MacBar::status_engine`, `status_*`) lets another surface host the data
workers, the popup builders and the calendar; the taskbar only draws the tray icons and places the popup next to them.

## macbar hiding (`bar.enabled`)

`macshell.json`: `{"bar": {"enabled": false}}` hides the menu bar, `"auto"` hides it exactly while `style` is `"windows"`, `true` (default) always shows it.
macbar polls the two files every 2 s. When the bar is switched off its surface is closed (the strut disappears, windows use the space) but
**the process stays alive**, sleeping and re-checking, and maps its surface again when the setting changes. This matters for the session:
`rdwm-macshell-session` supervises `macbar macdock` with `pgrep -x macbar || rdwmctl action spawn macbar` every 2 s, so a bar that exited would be
respawned forever; a sleeping one is simply left alone. No change to the session script is needed. For a Windows-only session the user
sets `{"bar": {"enabled": "auto"}}` once. (`macbar --png/--app` ignore the setting.)

## Adapting to the shared theme

All taskbar colours are one struct, `taskbar::draw::Colors` (`Colors::dark()`, `Colors::light()`, picked by `Colors::pick(light)`). Replace the
constructors with ones built from the theme module to follow it; sizes are `taskbar::Metrics` (one place, scaled by DPI).

## Code map

* `src/config.rs`: `DockStyle`, `WinAlign`, ..., `DockSettings`, `BarSettings`, `write_setting`.
* `src/taskbar/mod.rs`: pure model: `Metrics`, `build_buttons`, `layout`, hit-testing, `click_action`, tested without a display.
* `src/taskbar/draw.rs`: painting (`Painter`, `Colors`, `Frame`).
* `src/dock_app/windows.rs`: input, settings changes, popups, menu glue of `DockApp` in the Windows style.
* `src/app.rs`: `App::surface_spec` (the app can re-specify edge/margin/reserved space at runtime) and `App::quit_requested`; both backends honour them (X11 republishes `_NET_WM_STRUT_PARTIAL`, Wayland re-sets anchor/margin/exclusive zone).
* `tests/e2e/taskbar.sh`: Xvfb + rdwm end-to-end test (needs `Xvfb xdotool import xprop`, builds the fake windows from `examples/fakewin`).
