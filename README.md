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
| `macbar` | menu bar with dropdown menus, tray, clock and the Control Center (Wi-Fi, Bluetooth, Focus, Night Shift, dark mode, display and sound sliders, Now Playing); a vertical bar down the left edge (workspace pills, window, status icons, stacked clock, power) under the Caelestia style, see [docs/caelestia.md](docs/caelestia.md) | ~7 MB RSS |
| `machaven` | Wallhaven wallpaper browser (search, tabs, endless grid, set / download); the same browser lives in System Settings > Wallpaper > Wallhaven, see [docs/wallpapers.md](docs/wallpapers.md) | ~33 MB RSS with a page of thumbnails |
| `macnotify` | notification daemon (`org.freedesktop.Notifications`): macOS-style banners top-right, actions, history panel, Do Not Disturb | ~6 MB RSS, 0 % CPU idle |
| `macsettings` | System Settings for rdwm + the panels: edits `~/.config/rdwm/config.toml` in place (comments kept), applies live, follows hand edits; see `docs/macsettings.md` | ~25 MB RSS, 0 % CPU idle |
| `macswitch` | Alt+Tab window switcher for rdwm (X11): a card with one icon per window, all tags (other tags dimmed); see below | starts and paints in ~40 ms, exits on commit |
| `macexpose` | Exposé / Mission Control window overview for rdwm (X11): thumbnails of the current workspace, or `--all` grouped by workspace; see below | starts and paints in ~100 ms, exits on commit |
| `machud` | on-screen display for volume / mute / brightness / mic mute and workspace switches (X11); click-through, gone after 1.4 s; see below | starts and paints in ~40 ms, exits when it hides |
| `macclip` | clipboard history for X11: a daemon that records the CLIPBOARD (no polling) and a Spotlight-style picker that pastes with Ctrl+V; see below | daemon ~4 MB RSS, 0 % CPU idle; picker exits on commit |
| `machot` | Hot Corners for X11: the pointer resting in a screen corner runs an action (Dashboard, Spotlight, Launchpad, Lock, ...); a daemon with `get` / `set` CLI; see below | daemon ~3 MB RSS, 0 % CPU idle |
| `maclaunch` | Launchpad: a searchable grid of installed apps (type to filter, arrows, Enter, Esc) | ~20 MB RSS while open |
| `macwm` | debug tool: prints the window list whenever it changes | CLI |
| `macsession` | session menu (X11): the screen dims and a row of big buttons offers Lock, Log Out, Suspend, Hibernate, Restart, Shut Down; opened from the Apple menu ("Session…") or a key; see below | starts and paints in ~40-60 ms, exits on commit |
| `macutil` | CLI behind the Control Center's quick toggles: `record` (screen recording), `idle-inhibit`, `gamemode`, `dnd`; see below | CLI, no daemon |
| `macdash` | dashboard popup under the bar (X11): overview (user, clock, calendar, weather, resource rings), MPRIS media player, live performance graphs; opened from the bar's chart icon; see below | exits when closed; samples once a second only while open |
| `macthemectl` | `list`, `get`, `apply NAME` for the shared theme (one choice restyles bar, dock, Spotlight, Launchpad, settings and rdwm's titlebars); `style macos\|windows\|caelestia` sets theme, dock and bar together; `dynamic` takes Caelestia's colours from the wallpaper; see `docs/theming.md`; `wallpaper wallhaven random\|search WORDS` sets a wallpaper from wallhaven.cc (docs/wallpapers.md) | CLI |

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
* `macshell.json` - `{"bar": {"enabled": true}}` hides `macbar` (`false`, or `"auto"` = hidden while the dock is a Windows taskbar); `{"dock": {"enabled": true}}` hides `macdock` (`false`, or `"auto"` = hidden while the theme's bar is the vertical one, i.e. under the Caelestia style, which has no dock); `{"workspaces": "auto"}`: workspace/tag dots in `macbar` (`true`, `false` or `"auto"` = only under rdwm). Clicking a dot switches workspace (EWMH `_NET_CURRENT_DESKTOP` on X11, ext-workspace-v1 on Wayland)

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

## Window overview (`macexpose`)

A Mission Control / Exposé style overview for the rdwm X11 session. `macexpose` dims the screen (full-screen, override-redirect,
keyboard and pointer grabbed, like `macsession`) and shows the windows of the **current workspace** as a grid of thumbnails;
`macexpose --all` shows every window grouped by workspace in rows, under a strip of workspace chips 1-9 (the occupied ones are
highlighted, the current one is outlined; clicking a chip switches to that workspace). Bind it in rdwm:

```toml
[[bind]]
mods = ["super"]
key = "Up"
action = "spawn"
args = ["macexpose"]

[[bind]]
mods = ["super", "shift"]
key = "Up"
action = "spawn"
args = ["macexpose", "--all"]
```

Each window is captured once, before the overlay appears: Composite `NameWindowPixmap` when the server has it (and the window is
redirected), else a plain `GetImage` on the window (so a window hidden behind another one can come out wrong without a
compositor), box-filtered down to what the grid needs. Windows that are not mapped (minimised, on another workspace in `--all`)
get a card with the app icon (the dock's icon lookup) instead. Cells have rounded corners, the app icon and title underneath; the
hovered/selected cell is highlighted and the focused window is outlined. The layout is one uniform scale for all windows, the
largest at which everything fits, with the number of rows chosen by aspect ratio (it reads like macOS). The window list is rdwm's
`get_state` in creation order.

Keys: Return or a click switches to the window (`view` first when it is on another workspace, `unminimize` when minimised, then
`focuswindow`, the same requests as `macswitch`); arrows move the selection (up/down jump to the row above/below), Tab/Shift+Tab step,
Home/End; typing filters by title or class (a search pill appears, several words must all match, Backspace edits; `/` starts a filter
explicitly); `w`, Delete or a middle click closes the selected window (`WM_DELETE_WINDOW`; `w` is an ordinary letter once a filter is
started, Ctrl+W always closes); Escape or a click on empty space cancels and puts the focus back where it was. Running `macexpose`
again while it is open closes it. Everything is captured synchronously, so ten windows start and paint in about 100 ms
(`MACEXPOSE_TIMING=1` prints the time to the first frame). Without a compositor the dimmer is opaque; with one it is translucent.

`tests/e2e/expose.sh` runs it against a private Xvfb and rdwm with coloured test windows (screenshot-checks the thumbnails, click,
Escape, filter, `--all` with a chip and a window on another tag, close with `w`).

## On-screen display (`machud`)

`machud workspace` shows the current rdwm workspace as a big number with nine dots; `machud watch` runs for the whole session and shows it on every workspace switch (rdwm's `tag_view` event; `rdwm-macshell-session` starts it as the `wsosd` service).

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

## Clipboard history (`macclip`)

X11 only. `macclip daemon` runs for the whole session (autostart: `contrib/macshell-clip.desktop` or `.service`; under the rdwm
session add `macclip daemon` to its services). `macclip` (alias `macclip pick`) opens the picker; bind it to a key:

```
# rdwm / dwm-style X11 session
Super+V  ->  macclip
```

**Daemon.** It waits for XFixes selection-owner events on CLIPBOARD, so it costs nothing while you are not copying; PRIMARY is ignored.
It asks the new owner for `TARGETS` and the text (`UTF8_STRING`, else `STRING`) and keeps up to 100 entries, newest first, in
`$XDG_STATE_HOME/macshell/clipboard-history` (JSON, mode 0600, written atomically). Copying an entry again moves it to the top; empty or
whitespace-only text and anything over 100 KB is skipped. **Privacy:** nothing is recorded when the owner's window (or the focused window) has
a class containing `1password`, `bitwarden`, `keepassxc` or `keepass`, or when the selection offers the `x-kde-passwordManagerHint` target.
When the application that owns the clipboard exits and nobody took over, the daemon owns it and serves the newest recorded text
(`TARGETS`, `UTF8_STRING`, `STRING`, `TEXT`, `text/plain`), so copied text survives closing the app; a secret that was skipped is not resurrected.
`macclip clear` forgets the history and empties the clipboard.

**Picker.** A centred card like Spotlight: a search field and the entries (first line cut with an ellipsis, `⏎ N lines` for multi-line text, a small
age such as `3m`). It grabs the keyboard and pointer like `macspot`; it also works when the daemon is not running (it reads the file).

| Key | |
| --- | --- |
| type | filter (case-insensitive substring) |
| Up / Down / Tab (Shift+Tab back) | choose |
| Return | paste: make it the clipboard, close, give the focus back and press Ctrl+V (XTest; without the extension it only sets the clipboard) |
| Shift+Return | copy only |
| Alt+1 .. Alt+9 | paste that entry (Alt+Shift+digit copies only) |
| Delete | remove the entry from the history |
| Esc, click outside, `macclip` again | close |

Terminals paste with Ctrl+Shift+V: use Shift+Return there and paste by hand. `tests/e2e/clip.sh` (`make e2e-clip`) runs the daemon and the picker against a private
Xvfb with `xclip`, a fake password manager window and a window that receives the Ctrl+V.

## Hot corners (`machot`)

X11 only. `machot daemon` runs for the whole session and puts a 2x2 px InputOnly window in each configured corner of every monitor (RandR
monitors; rebuilt on screen changes). When the pointer has stayed in a corner for `dwell_ms` (default 150), the corner's action runs; it
cannot fire again until the pointer has left the corner and a short cooldown (400 ms) has passed. Nothing fires while a mouse button is
held, so dragging a window to a corner (or into rdwm's edge snap zones) is safe. The daemon grabs nothing, never takes the focus and sleeps
in `poll(2)` while idle (no timers; the only wake-ups are pointer crossings, restacks and config changes). The corner where two monitors
touch is not a corner and stays inactive; the corner windows sit above everything else, so those 2x2 px do not reach the window below.

Settings live in `macshell.json` and are applied at once (inotify), also when you edit the file by hand:

```json
{ "hot_corners": { "top_left": "none", "top_right": "dashboard", "bottom_left": "lock",
                   "bottom_right": "custom:notify-send 'hello'", "dwell_ms": 150 } }
```

| Action | Runs |
| --- | --- |
| `none` | nothing (the corner gets no window) |
| `dashboard` / `session` / `spotlight` / `launchpad` | `macdash` / `macsession` / `macspot` / `maclaunch` (they toggle, so re-entering closes them) |
| `overview` | `macexpose` when it is in `PATH`, else nothing |
| `lock` | `rdwm-lock` |
| `tile-all` | `rdwmctl action tileall` |
| `notifications` | `macnotify history` |
| `show-desktop` | accepted but does nothing yet: rdwm has no action for it |
| `custom:<shell command>` | the command through `sh -c`, detached |

`machot get` prints the corners, `machot set top-right spotlight` changes one (`machot set dwell_ms 200` the dwell time), `machot actions`
lists the names. System Settings has the same four dropdowns and a dwell slider under Desktop & Dock > Hot corners.

**Starting it.** Autostart with `contrib/macshell-hot.desktop` (installed to `/etc/xdg/autostart`; it is skipped under rdwm, which starts
its services itself: add `machot daemon` there, like `macclip daemon`) or `contrib/macshell-hot.service` (systemd user unit, X11 sessions
only). `tests/e2e/hot.sh` (`make e2e-hot`) runs it on a private Xvfb with shimmed commands and xdotool as the pointer.

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

## Caelestia style (vertical bar)

`macthemectl style caelestia` makes the shell a port of caelestia-shell for rdwm/X11: its Material You colours and a **vertical bar** on the left screen edge
(`bar_style = "vertical"` in a theme; `macbar` anchors left and publishes a `_NET_WM_STRUT_PARTIAL` left strut, so windows tile to its right) with the OS icon (launcher),
rdwm workspace pills (occupied / active / urgent, click or wheel to switch), the focused window, the layout / quick-settings tray, status icons (sound, Wi-Fi,
Bluetooth, battery, lock keys) whose popups open on hover, the clock stacked as `HH` over `MM` (opens the dashboard) and the power button (opens the session menu).
The dock is hidden while that bar is up (`"dock": {"enabled": "auto"}`); `macthemectl style macos` brings the horizontal bar and the dock back without restarting anything.
The dashboard, launcher, session menu, OSD, notification history and utilities are the existing programs under the Caelestia colours.
See [docs/caelestia.md](docs/caelestia.md) for the layout, hotkeys and limits, and `tests/e2e/vbar.sh` (`make e2e-vbar`) for the end-to-end check on a private Xvfb and rdwm.

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

## Session menu (`macsession`)

Ported from caelestia-shell's session drawer. A full-screen dimmer on the primary monitor (override-redirect X11 window, keyboard and pointer grabbed)
with a centred row of big rounded buttons. It is opened from the Apple menu ("Session…") and from any key you bind:

```toml
# rdwm
[[bind]]
mods = ["super", "shift"]
key = "e"
action = "spawn"
args = ["macsession"]
```

| Key | Button | Command |
|---|---|---|
| `L` | Lock | `rdwm-lock` (else `loginctl lock-session`) |
| `O` | Log Out | `rdwmctl action quit` while rdwm runs, else `loginctl terminate-user $USER` |
| `S` | Suspend | `systemctl suspend` |
| `H` | Hibernate (only when `/sys/power/state` offers `disk`) | `systemctl hibernate` |
| `R` | Restart | `systemctl reboot` |
| `P` | Shut Down | `systemctl poweroff` |

Arrows / Tab move the selection, Return runs it, Escape (or a click on the dimmed background) cancels, a second `macsession` closes the first.
Log Out, Restart and Shut Down are armed by the first key press or click ("Press again" in red) and run on the second one on the same button;
moving to another button disarms. The command starts after the overlay is gone, so a screen locker can grab the keyboard. The buttons are drawn
before any font is loaded and the labels follow a frame later, which keeps the overlay up in well under 80 ms (`MACSESSION_TIMING=1` prints the time to the first frame).
The dimmer is translucent under a compositor and opaque without one; with several monitors only the primary one is covered.

## Utilities (`macutil`)

Ported from caelestia-shell's utilities drawer (screen recorder, idle inhibitor, game mode, toasts). Every command needs no daemon of its own and is idempotent; `status` prints `on` or `off`.
State lives in `$XDG_RUNTIME_DIR/macutil/`; toasts go through the notification daemon (macnotify, D-Bus, else `notify-send`); `-q` or `MACUTIL_QUIET=1` silences them.

| Command | What it does |
|---|---|
| `macutil record start\|stop\|toggle\|status [--region]` | `ffmpeg -f x11grab` (libx264, 30 fps) on X11, `wf-recorder` on Wayland, into `~/Videos/Recordings/recording-DATE.mp4` (`$MACUTIL_RECORD_DIR`). `--region` asks for an area with `slop` (`slurp` on Wayland). Stopping sends SIGINT, waits for the file to be finished and notifies "Recording saved" with its path; a recorder that dies shows as `off` |
| `macutil idle-inhibit on\|off\|toggle\|status` | `xset s off -dpms`, so `xss-lock` never fires and the monitors never blank; the values read from `xset q` (timeout, cycle, DPMS timeouts) are put back on `off` |
| `macutil gamemode on\|off\|toggle\|status` | stops `picom` and sets the `performance` power profile (`powerprofilesctl`, when it offers it); `off` starts picom again with its old command line (found with `pgrep -ax picom`; a supervisor that restarts picom itself will undo this) and restores the previous profile |
| `macutil dnd on\|off\|toggle\|status` | do-not-disturb of macnotify (its `dnd` flag in `~/.config/macshell/notifications.toml` when it is not running); `dunstctl` / `makoctl` when one of those is the daemon |
| `macutil status` | `record=off idle-inhibit=on gamemode=off dnd=off` |

The Control Center has a row of four tiles for these (Record, Inhibit Idle, Game Mode, DND) with live state read from the state files, and a Notifications tile that opens the macnotify history panel.
`tests/e2e/session.sh` (`make e2e-session`) drives macsession and macutil on a private Xvfb with shimmed `systemctl`, `loginctl`, `ffmpeg`, `xset`, `picom` and friends.
