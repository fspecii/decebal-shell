# The Caelestia style

`macthemectl style caelestia` (or **Shell style** at the top of the Theme pane) turns the shell into a port of
[caelestia-shell](https://github.com/caelestia-dots/shell) for the rdwm X11 session: its Material You colours, a **vertical bar**
down the left screen edge, no dock, and the other Caelestia widgets one click or one key away. `macthemectl style macos` and
`windows` switch back at runtime; nothing is restarted by hand (the running programs notice within about a second).

```
macthemectl style caelestia      # theme caelestia, bar_style vertical, dock hidden
macthemectl style macos          # the horizontal bar and the floating dock again
```

## What changes

| piece | Caelestia style |
|---|---|
| theme | `caelestia` (the shell's shipped scheme, or `macthemectl dynamic` for colours from the wallpaper) |
| bar | `bar_style = "vertical"`: `macbar` moves to the left edge (`Anchor::Left`), 52 px wide, and reserves that strip with `_NET_WM_STRUT_PARTIAL` (`left = 52`, `left_start_y`/`left_end_y` = the monitor's rows), so rdwm tiles windows to its right |
| dock | `macshell.json` `dock.enabled = "auto"`: hidden while the theme's bar is the vertical one (see below) |
| dashboard, launcher, session, OSD, notifications | unchanged programs, restyled by the theme; positioned for a bar that is not at the top |

### The vertical bar, top to bottom

| item | what it shows | hover / click | wheel |
|---|---|---|---|
| OS icon | your distribution's logo (Nerd Font, from `/etc/os-release`; a grid without a Nerd Font) | click: launcher (`macspot`) | |
| workspaces | pills for rdwm tags 1-9 (more or fewer follow `[general] tags`): the active one filled in the accent colour, occupied ones with a tinted pill, empty ones a dot, **urgent** ones in the danger colour | click a pill: rdwm `view N` | next / previous tag (`viewnext` / `viewprev`) |
| focused window | its icon and its title turned 90 degrees (the app name when there is no title); hidden when no window has focus or the screen is too short | hover or click: popup with the app, its title, Close, Minimise, Float or tile | |
| tray | the window layout of rdwm, stay awake, agents, and the quick-settings tile | layout: popup with every layout; tile: the Control Center panel beside the bar (Wi-Fi, Bluetooth, Focus, Night Shift, dark mode, sliders, Now Playing, the record / idle-inhibit / game mode / DND tiles, notification history) | layout: cycle |
| status icons | lock keys (only while Caps or Num Lock is on, X11), sound, Wi-Fi or Ethernet, Bluetooth, battery with its percentage | hover (or click): the same popups the horizontal bar opens: sound with volume and brightness sliders, Wi-Fi list, Bluetooth devices, battery status | sound icon: volume; middle click on it mutes |
| clock | `HH` over `MM` | click: dashboard (`macdash`) | |
| power | the power symbol | click: session menu (`macsession`) | |

On the rest of the strip the wheel works like Caelestia's: the upper half of the screen changes the volume, the lower half the
brightness. Popups open to the right of the bar next to their icon after a short rest of the pointer (140 ms); sliding to the
next icon switches at once; they close when the pointer leaves for a moment, on Escape, or on a click elsewhere.

The tray is not a StatusNotifier tray (macshell has none); it holds the items the horizontal bar already had.

### Why the dock is hidden, and how to get it back

Caelestia has no dock. The least invasive way to match that is the same switch `bar.enabled` already is: `macdock` keeps running
(the session supervisor restarts a process that goes missing) but has no surface while `dock.enabled` says so, and comes back by
itself, without a restart, when it changes.

```json
{ "dock": { "enabled": "auto" } }
```

* `true` (the default when the key is absent) always shows the dock, `false` never does.
* `"auto"` hides it while the **theme** has a vertical bar. `macthemectl style caelestia` writes it; `macos` and `windows` write `true`.
  Picking any other theme in the Theme pane brings the dock back by itself, because only Caelestia's bar is vertical.
* To keep a dock in the Caelestia style: set `"enabled": true` after choosing the style. The dock then floats above the bottom edge as usual and
  does not collide with the bar (it is centred on the monitor, the bar owns the left 52 px).

## Reaching the other widgets

All of them already exist as programs and follow the shared theme, so under Caelestia they use its colours, radii and font.

| Caelestia | macshell | open it from | key (suggested binding, see below) |
|---|---|---|---|
| launcher | `macspot` (applications, files, calculator, units, commands, web) and `maclaunch` (grid) | the OS icon | `Super+Space` |
| dashboard | `macdash` (overview, media, performance; weather, calendar, resources) | the clock; hot corner | `Super+D` |
| session | `macsession` (lock, log out, suspend, hibernate, restart, shut down) | the power button | `Ctrl+Alt+Delete`, `Super+Shift+E` |
| OSD | `machud` (volume, brightness, mic, **workspace card**) | volume / brightness keys and every workspace switch (`machud watch`) | |
| sidebar (notifications) | `macnotify` banners and its history panel | the Control Center's Notifications tile | `Super+N` |
| utilities (record, idle inhibit, game mode, DND) | `macutil` and the Control Center's tile row | the quick-settings tile | `Super+Shift+R` (record), `Super+Shift+I` (idle), `Super+Shift+G` (game mode) |
| window overview | `macexpose` | | `Super+Up`, `Super+Shift+Up` (all workspaces) |
| area picker, lock screen | `omarchy capture screenshot`, `rdwm-lock` | the Control Center's Screenshot tile | `Super+Shift+S`, `Super+L` |

### Hotkeys

Caelestia is driven by global shortcuts of its compositor. rdwm binds keys itself, so add these to `~/.config/rdwm/config.toml`
(they are suggestions: change the keys to taste, `macsettings` has a Shortcuts pane for it). Each program toggles when started twice.

```toml
[[bind]]
mods = ["super"]
key = "space"
action = "spawn"
args = ["macspot"]

[[bind]]
mods = ["super"]
key = "d"
action = "spawn"
args = ["macdash"]

[[bind]]
mods = ["super", "shift"]
key = "e"
action = "spawn"
args = ["macsession"]

[[bind]]
mods = ["ctrl", "alt"]
key = "Delete"
action = "spawn"
args = ["macsession"]

[[bind]]
mods = ["super"]
key = "n"
action = "spawn"
args = ["macnotify", "history"]

[[bind]]
mods = ["super", "shift"]
key = "r"
action = "spawn"
args = ["macutil", "record", "toggle"]

[[bind]]
mods = ["super", "shift"]
key = "i"
action = "spawn"
args = ["macutil", "idle-inhibit", "toggle"]

[[bind]]
mods = ["super", "shift"]
key = "g"
action = "spawn"
args = ["macutil", "gamemode", "toggle"]

# workspaces: the bar's pills follow rdwm's tags, these are the usual keys
[[bind]]
mods = ["super"]
key = "1"
action = "view"
args = ["1"]
# ... 2 to 9 the same way; "viewnext" / "viewprev" step through them
```

Hot corners (`machot`, Desktop & Dock > Hot corners in macsettings) can open the dashboard, session menu, launcher and
notification history from a screen corner, the closest thing to Caelestia's edge-hover drawers.

## What is not in it

* No StatusNotifier (application indicator) tray, no keyboard-layout and wallpaper pickers in the launcher, no drawers that slide from the
  screen edges: the widgets are separate popups, as in the rest of macshell.
* The focused window's title is read when the focus or the window list changes, not on every title change.
* On Wayland the layer-shell surface is anchored left with an exclusive zone, but only X11 (rdwm) has been exercised; lock-key indicators are X11 only.
* Only the primary monitor gets a bar.
