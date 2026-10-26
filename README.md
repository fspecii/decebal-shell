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
| `machaven` | Wallhaven wallpaper browser (search, tabs, endless grid, set / download) | ~44 MB RSS with a page of thumbnails |

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

Environment: `MACSHELL_LAUNCHPAD` - command the Launchpad tile runs.

Icons come from the WhiteSur icon theme if installed (then Adwaita/hicolor); text uses Inter if installed.

## Develop

```sh
cargo test                       # unit tests
macdock --png dock.png           # render the dock to a PNG without any display
```

Tested on: Hyprland (live), nested sway and niri (Wayland), and dwm + picom (X11).

Known limits: X11 override-redirect windows cannot reserve screen space, so turn off dwm's own bar
(`showbar = 0`). Scale factor 1 only. The Focus tile needs `dunstctl` or `makoctl`. The bar is dark-only.

## License

MIT
