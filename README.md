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
| `macnotify` | notification daemon (`org.freedesktop.Notifications`): macOS-style banners top-right, actions, history panel, Do Not Disturb | ~6 MB RSS, 0 % CPU idle |
| `macsettings` | System Settings for rdwm + the panels: edits `~/.config/rdwm/config.toml` in place (comments kept), applies live, follows hand edits; see `docs/macsettings.md` | ~25 MB RSS, 0 % CPU idle |
| `macswitch` | Alt+Tab window switcher for rdwm (X11): a card with one icon per window, all tags (other tags dimmed); see below | starts and paints in ~40 ms, exits on commit |
| `machud` | on-screen display for volume / mute / brightness / mic mute (X11); click-through, gone after 1.4 s; see below | starts and paints in ~40 ms, exits when it hides |
| `macdash` | dashboard popup under the bar (X11): overview (user, clock, calendar, weather, resource rings), MPRIS media player, live performance graphs; opened from the bar's chart icon; see below | exits when closed; samples once a second only while open |
| `macthemectl` | `list`, `get`, `apply NAME` for the shared theme (one choice restyles bar, dock, Spotlight, Launchpad, settings and rdwm's titlebars); see `docs/theming.md` | CLI |

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
* `dock-settings.json` - `appSpacing`, `magnification`, `transparency`, `roundness`, and the Windows taskbar options `style` (`"mac"` default / `"windows"`), `windowsAlign`, `windowsLabels`, `windowsSearch`, `windowsPosition`, `windowsTheme` (see [docs/dock-style.md](docs/dock-style.md))
* `macshell.json` - `{"bar": {"enabled": true}}` hides `macbar` (`false`, or `"auto"` = hidden while the dock is a Windows taskbar); `{"workspaces": "auto"}`: workspace/tag dots in `macbar` (`true`, `false` or `"auto"` = only under rdwm). Clicking a dot switches workspace (EWMH `_NET_CURRENT_DESKTOP` on X11, ext-workspace-v1 on Wayland)

The dock re-reads `dock-pinned-macos.json` and `dock-settings.json` within about a second of an external edit (a `stat` twice a second, no restart); its own changes are written atomically.

Dock interaction: click focuses a running app (restoring a minimised window) or launches it with a bounce; middle click starts a new instance; drag a pinned icon sideways to reorder; right click opens the context menu (windows, New Window, Keep in / Remove from Dock, Show in Finder, Hide, Quit; Open / Empty Trash for the Trash). The menu closes on Escape, an outside click or activation, and supports hover and Up / Down / Enter. Running apps that are not pinned appear before the Trash. `MACSHELL_OPEN` (default `xdg-open`) and `MACSHELL_TRASH_EMPTY` (default `gio trash --empty`) override the helper commands.

Environment: `MACSHELL_LAUNCHPAD` - command the Launchpad tile runs.

Icons come from the WhiteSur icon theme if installed (then Adwaita/hicolor); text uses Inter if installed.

## Launching apps

Every launcher (dock, Launchpad, Spotlight, macsettings) starts apps through one code path: Exec field codes, `Path=`, `Terminal=true` (in a terminal), `TryExec=`, detached in their own session, errors logged to `~/.local/state/macshell/launch.log` and shown as a notification.
Without `WAYLAND_DISPLAY` (an X11 session) Chromium-family browsers and Electron apps get `--ozone-platform=x11` appended, since the flags files Omarchy ships (`~/.config/chrome-flags.conf`, ...) ask for Wayland. Add your own rules in `macshell.json`:

```json
{"launch": {"builtin": true, "rules": [{"match": ["myapp", "foo*"], "when": "x11", "args": ["--flag"], "env": {"K": "v", "DROP": null}}]}}
```

## Spotlight (`macspot`)

A search box centred on the primary monitor. Bind **one** key to the bare command `macspot`; running it
again while the box is open closes it (a toggle, single instance via `$XDG_RUNTIME_DIR/macspot.lock`).

```
# rdwm / dwm-style X11 session
Super+Space  ->  macspot
# Hyprland
bind = SUPER, SPACE, exec, macspot
```

On X11 it is an override-redirect popup (no window manager involvement) that grabs the keyboard and pointer,
so typing works whatever the WM focuses, and Esc, a click outside, or pressing the hotkey again closes it.
On Wayland it is a layer-shell overlay with exclusive keyboard focus.

| Type | Result |
| --- | --- |
| `fire`, `vsc` | applications, fuzzy-ranked; the ones you open often or recently rank higher. An empty box lists recent apps |
| `report` (3+ chars) | files under `$HOME` via `plocate`, else `fd` (skipped when neither is installed) |
| `2+3*4` | calculator, Enter copies the result |
| `5 km to mi`, `100 f in c` | unit conversion (length, mass, volume, data, time, speed, temperature) |
| `> cmd` | run a command in a terminal |
| `lock`, `sleep`, `restart`, `shut down`, `log out`, `screenshot` | system commands (restart, shut down and log out need a second Enter; commands whose program is missing are hidden) |
| `install foo`, `remove foo` | Arch packages |
| anything else | web search (`MACSPOT_SEARCH_URL`, `%s` is the query; default DuckDuckGo) |

Enter opens. Ctrl+Enter or Alt+Enter is the alternative: an app runs in a terminal, a file shows its folder,
a calculation copies `expr = result`, `> cmd` runs in the background. Up/Down, Tab, Ctrl+N/P choose.

State: the app index is cached in `~/.cache/macshell/apps.tsv` (rescanned in the background on every start, so
the box opens at once) and usage counts live in `~/.local/state/macshell/spotlight-history`. Copying needs no
`xclip`: on X11 a tiny helper process owns the selection; on Wayland `wl-copy` is used.

`tests/e2e_spot.sh [macspot] [display]` runs the whole thing on a private Xvfb (under `rdwm` when installed) with stub apps.

## Window switcher (`macswitch`)

Alt+Tab for the rdwm X11 session. rdwm starts it from its binding (`macswitch` = forward, `macswitch --back` = Alt+Shift+Tab):

```toml
[[bind]]
mods = ["alt"]
key = "Tab"
action = "spawn"
args = ["macswitch"]

[[bind]]
mods = ["alt", "shift"]
key = "Tab"
action = "spawn"
args = ["macswitch", "--back"]
```

A centred card shows one icon per window (the same icon lookup as the dock, a coloured letter tile when there is none) and the
selected window's full title below. Windows of every tag are listed; the ones not in view are dimmed and say which desktop they are
on. The first selection is the previous window (the last one with `--back`). While it is shown it holds the keyboard:
Tab/Right/Down next, Shift+Tab/Left/Up previous, Return, a click on a cell or **letting go of Alt** switches, Esc cancels,
`q`/`w` closes the selected window. Switching sends rdwm `view` (when the window is on another tag), `unminimize` (when it is
minimised) and `focuswindow`. A second `macswitch` while one is open only moves its selection (SIGUSR1/SIGUSR2). `--mod super|ctrl`
changes the held modifier. The list comes from `rdwmctl state` over the IPC socket, most recently used first: rdwm's own `mru`
array when `get_state` has one, else the focused window followed by the order macswitch itself last left
(`$XDG_RUNTIME_DIR/macswitch-mru`), then rdwm's list order. With a compositor the card is translucent; without one it is opaque.

`tests/e2e/switch.sh` runs it against a private Xvfb and rdwm (xdotool holds Alt, taps Tab; also checks Escape, clicks, tags and screenshots).

## On-screen display (`machud`)

The little rounded card macOS shows when you press a volume or brightness key: an icon (speaker with waves, crossed speaker when muted,
sun, microphone), a label (`Volume 45%`, `Muted`, `Brightness 70%`, `Microphone muted`) and a 16-segment bar. Call it after the key's own action:

```toml
# rdwm: wpctl/brightnessctl change the value, machud reads it back and shows it
[[bind]]
mods = []
key = "XF86AudioRaiseVolume"
action = "spawn"
args = ["sh", "-c", "wpctl set-volume -l 1.5 @DEFAULT_AUDIO_SINK@ 5%+; machud volume"]
```

`machud volume` / `machud mic` read the default sink / source with `wpctl get-volume` (`pactl` as a fallback), `machud brightness` uses
`brightnessctl -m` and falls back to `/sys/class/backlight`. A value that cannot be read shows nothing and exits 0. The card sits horizontally
centred on the primary monitor, a tenth of its height above the bottom edge, as an override-redirect window with an empty input region: it never
takes focus, keys or clicks. It disappears 1.4 s after the last call; calling again while it is up updates the value and restarts the timer
(single instance via `$XDG_RUNTIME_DIR/machud.lock`; the new value travels in `$XDG_RUNTIME_DIR/machud.msg` plus SIGUSR1). It follows the shared theme, is
translucent under a compositor and opaque without one, and costs nothing while idle because the process exits. X11 only (no Wayland layer-shell
version yet). `tests/e2e/hud.sh` runs it on a private Xvfb with shimmed `wpctl` / `brightnessctl`.

## Dashboard (`macdash`)

A port of caelestia-shell's dashboard: a card hanging under the menu bar with three tabs (click them, or Left / Right / `1` `2` `3`).

* **Overview**: avatar (`~/.face`, else an initial tile), name, `user@host`, uptime, the clock and date, a month calendar, the current weather
  (the bar's cached reading in `~/.cache/macshell/weather.json`, fetched only when stale), CPU / memory / disk rings and a now-playing card
  that opens the Media tab.
* **Media**: any MPRIS player (`org.mpris.MediaPlayer2.*`, found and watched over macshell's own D-Bus client, no `playerctl`): cover art
  (`file://` or `http(s)` URLs, the latter fetched once with `curl` into `~/.cache/macshell/dash-art/`), title / artist / album, a progress bar
  you can click or drag to seek (the position is polled once a second, only while the tab shows), previous / play-pause / next, a volume slider
  when the player has a `Volume` property (Up / Down or the wheel too), and a picker when several players run (click a chip, or Tab).
* **Performance**: CPU (total ring, per-core meters, history, package temperature from hwmon, load), memory and swap, root filesystem, network
  rates with a history graph, and an NVIDIA GPU card (`nvidia-smi --query-gpu`; hidden when the tool is missing). Rings and sparklines are tiny-skia paths.

Bind **one** key to the bare command `macdash` (or `macdash media` / `macdash performance` to open on a tab): running it again while the card is open closes it
(single instance via `$XDG_RUNTIME_DIR/macdash.lock`). Escape, a click outside, or focus moving elsewhere also close it. `--x PX` centres the card on that x offset
(macbar passes the position of its chart icon), `--top PX` moves it off the default 30 px. It is an override-redirect X11 popup that grabs the keyboard and
pointer like `macspot`, follows the shared theme and is translucent under a compositor. Nothing runs while it is closed: sampling threads and the D-Bus
connection only exist while the card is open. `tests/e2e/dash.sh` (`make e2e-dash`) runs it on a private Xvfb with a private D-Bus session and fake players
(`examples/fake_mpris.rs`); it needs `tesseract` to read the card.

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

## Notifications (`macnotify`)

X11 only (Wayland sessions keep mako/dunst). `macnotify` owns `org.freedesktop.Notifications` on the session
bus, so `notify-send`, Chrome, Telegram, ... show up as banners below the menu bar, top right, newest first,
at most five at a time (the rest wait). Hover pauses the countdown and shows a close button; click a banner
to run its `default` action and dismiss it, click an action button to invoke it, right click to dismiss,
middle click to dismiss all. Urgency `critical` never expires. It speaks D-Bus through a small
hand-written client (`src/dbus.rs`, no libdbus, no async runtime) and draws override-redirect windows
clipped with the Shape extension, so it needs no compositor and never paints black corners.

If another daemon already owns the name (mako, dunst) it exits quietly, so it is safe to start from any
X11 session script. Under a Wayland session it refuses to start (`MACNOTIFY_FORCE=1` overrides).

```
macnotify                      run the daemon (autostart: contrib/macshell-notify.desktop or .service)
macnotify history              toggle the history panel (bind it to a key); click an entry to remove it
macnotify dnd on|off|toggle    Do Not Disturb (also the Focus tile of the Control Center)
macnotify test                 send a sample notification
```

Settings, `~/.config/macshell/notifications.toml` (re-read on every notification, edited by macsettings):

```toml
dnd = false            # banners hidden while true; history still records; critical ones still show
banner_seconds = 5     # default lifetime when the app does not ask for one
muted_apps = []        # app names or desktop-entry ids that never show or record
```

History lives in `~/.local/state/macshell/notification-history.json` (last 100). Colours come from
`notify::ui::Palette` (dark, light when `desktop-style` says light): the single place to hook the theme module.
Markup in bodies is stripped (macnotify does not advertise `body-markup`).

Test without touching your session: `make e2e-notify` (private `dbus-daemon` and Xvfb).

## Develop

```sh
cargo test                       # unit tests
tests/e2e/dock_menu.sh           # dock menu / reorder / live reload against rdwm on a private Xvfb (:93)
macdock --png dock.png           # render the dock to a PNG without any display
```

Tested on: Hyprland (live), nested sway and niri (Wayland), and dwm + picom (X11).

Known limits: on X11 the panels publish `_NET_WM_STRUT_PARTIAL` and the DOCK window type, which rdwm
honours; dwm ignores struts, so turn off its own bar (`showbar = 0`). Scale factor 1 only. The Focus tile needs `macnotify`, `dunstctl` or `makoctl`. The bar is dark-only.

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
