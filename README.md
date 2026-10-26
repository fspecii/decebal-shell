# macshell

A tiny macOS-style desktop shell in Rust: **dock**, **menu bar with Control Center** and a
**Wallhaven wallpaper browser**. One codebase, two display backends:

| Backend | Where it runs | How |
|---|---|---|
| Wayland | Hyprland, sway, niri, river, labwc, ... | `wlr-layer-shell` + `wl_shm` |
| X11 | dwm, i3, openbox, bspwm, ... | ARGB override-redirect window (use picom for translucency/blur) |

The backend is picked automatically (`MACSHELL_BACKEND=wayland|x11` forces one).

## Why

The original Quickshell/QML version used about 550 MB of RAM. This one draws on the CPU into a
shared buffer and only redraws when something changes:

| | macdock (release) |
|---|---|
| binary | 1.8 MB |
| RSS | ~5.7 MB |
| idle CPU | 0 % |

## Install

```sh
make && sudo make install          # or: makepkg -si
systemctl --user enable --now macshell-dock.service
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

Tested nested in: Hyprland, sway, niri (Wayland) and dwm + picom in Xephyr (X11).

## License

MIT
