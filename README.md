# macshell

A tiny macOS-style desktop shell in Rust: **dock**, **menu bar with Control Center** and a
**Wallhaven wallpaper browser**. One codebase, two display backends:

| Backend | Where it runs | How |
|---|---|---|
| Wayland | Hyprland, sway, niri, river, labwc, ... | `wlr-layer-shell` + `wl_shm` |
| X11 | dwm, i3, openbox, bspwm, ... | ARGB override-redirect window (use picom for translucency/blur) |

The backend is picked automatically (`MACSHELL_BACKEND=wayland|x11` forces one).

## What is in it

| Program | What | Idle memory (release) |
|---|---|---|
| `macdock` | floating dock: hover magnification, running dots, click to launch | ~6 MB RSS |
| `macbar` | menu bar with dropdown menus, tray, clock and the Control Center (Wi-Fi, Bluetooth, Focus, Night Shift, dark mode, display and sound sliders, Now Playing) | ~7 MB RSS |
| `machaven` | Wallhaven wallpaper browser (search, tabs, endless grid, set / download) | ~33 MB RSS with a page of thumbnails |

For comparison the Quickshell/QML shell this replaces used about 550 MB. All three draw on the CPU
into a shared buffer, redraw only when something changes, and sit at 0 % CPU when idle. Binaries are
2-4 MB.

## Install

```sh
make && sudo make install          # or: makepkg -si
systemctl --user enable --now macshell-dock.service macshell-bar.service
```

On X11 run a compositor (e.g. `picom`) for the translucent, rounded look.

## Configuration

`macdock` reads the same files as the original dock, from `$MACSHELL_CONFIG` or `~/.config/omarchy/`:

* `dock-pinned-macos.json` - `{"pinned": [...], "order": [...]}` (desktop-file ids; `launchpad` and `trash` are built in)
* `dock-settings.json` - `appSpacing`, `magnification`, `transparency`, `roundness`
* `macshell.json` - `{"workspaces": "auto"}`: workspace/tag dots in `macbar` (`true`, `false` or `"auto"` = only under rdwm). Clicking a dot switches workspace (EWMH `_NET_CURRENT_DESKTOP` on X11, ext-workspace-v1 on Wayland)

The dock re-reads `dock-pinned-macos.json` and `dock-settings.json` within about a second of an external edit (a `stat` twice a second, no restart); its own changes are written atomically.

Dock interaction: click focuses a running app (restoring a minimised window) or launches it with a bounce; middle click starts a new instance; drag a pinned icon sideways to reorder; right click opens the context menu (windows, New Window, Keep in / Remove from Dock, Show in Finder, Hide, Quit; Open / Empty Trash for the Trash). The menu closes on Escape, an outside click or activation, and supports hover and Up / Down / Enter. Running apps that are not pinned appear before the Trash. `MACSHELL_OPEN` (default `xdg-open`) and `MACSHELL_TRASH_EMPTY` (default `gio trash --empty`) override the helper commands.

Environment: `MACSHELL_LAUNCHPAD` - command the Launchpad tile runs.

Icons come from the WhiteSur icon theme if installed (then Adwaita/hicolor); text uses Inter if installed.

## Window rules for the wallpaper browser

`machaven` is a normal window (app id / class `machaven`, 1280x820). Tiling compositors tile it unless you
add a rule; your original QML panel was a floating, centred window.

```
# sway (tested)
for_window [app_id="machaven"] floating enable, resize set 1280 820, move position center

# niri (tested)
window-rule {
    match app-id="^machaven$"
    open-floating true
    default-column-width { fixed 1280; }
    default-window-height { fixed 820; }
}

# Hyprland, Lua config (tested): put in your hyprland.lua
hl.window_rule({ match = { class = "^machaven$" }, float = true, center = true, size = { 1280, 820 } })

# Hyprland, classic config syntax (not tested)
windowrule = float, class:^(machaven)$
windowrule = size 1280 820, class:^(machaven)$
windowrule = center, class:^(machaven)$
```

dwm: add `{ "machaven", NULL, NULL, 0, 1, -1 }` to the `rules[]` table in `config.h` (floating = 1).

## Develop

```sh
cargo test                       # unit tests
tests/e2e/dock_menu.sh           # dock menu / reorder / live reload against rdwm on a private Xvfb (:93)
macdock --png dock.png           # render the dock to a PNG without any display
```

Tested on: Hyprland (live), nested sway and niri (Wayland), and dwm + picom (X11).

Known limits: on X11 the panels publish `_NET_WM_STRUT_PARTIAL` and the DOCK window type, which rdwm
honours; dwm ignores struts, so turn off its own bar (`showbar = 0`). Scale factor 1 only. The Focus tile needs `dunstctl` or `makoctl`. The bar is dark-only.

## License

MIT

## Status items (macbar)

Right side of the bar, all with graceful degradation (an item is hidden when its tool or hardware is missing):

| item | click | wheel / middle click |
|---|---|---|
| window layout (rdwm only) | popup with every rdwm layout (icon, checkmark), master count/width, gaps, float toggle | wheel cycles layouts |
| battery (laptops) | status popup | |
| Bluetooth (`bluetoothctl`) | power toggle, paired devices, connect/disconnect | |
| sound (`wpctl` or `pactl`) | volume slider, mute, display brightness | wheel +-5%, middle click mutes |
| Wi-Fi (`nmcli`) | toggle, scan list, connect | |
| clock | month calendar | |
| control centre | quick-settings panel | |

External commands run on worker threads; volume changes made elsewhere are followed through `pactl subscribe`
(no polling), rdwm layout changes through an rdwm IPC subscription. `tests/e2e_status.sh` drives it all on a private Xvfb
with a real rdwm and fake tools from `tests/fakebin`.
