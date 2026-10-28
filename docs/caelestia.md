# The Caelestia style

`macthemectl style caelestia` (or **Shell style** at the top of the Theme pane) turns the shell into a port of
[caelestia-shell](https://github.com/caelestia-dots/shell) for the rdwm X11 session: its Material You colours, a **pill bar** on a
screen edge (the left one unless you choose another, see [Layouts](#layouts)), no dock, and the other Caelestia widgets one click or one
key away. `macthemectl style macos` and `windows` switch back at runtime; nothing is restarted by hand (the running programs notice
within about a second).

```
macthemectl style caelestia      # theme caelestia, bar_style vertical (the pill bar), dock hidden; keeps the bar's position
macthemectl style macos          # the classic horizontal bar and the floating dock again
macthemectl layout top           # the same bar along the top edge (left | right | top | bottom | split)
```

## Layouts

`bar.position` in `macshell.json` (`left` by default) says where the bar sits. Set it with `macthemectl layout POSITION`, with a style word
(`macthemectl style caelestia-top`, also `caelestia-left`, `-right`, `-bottom`, `-split`), or with the **Layout** row under Shell style in the
Theme pane (shown while the Caelestia style is on). `macthemectl layout` alone prints it. The running `macbar` re-lays out by itself within
about a second, the same way it follows a style switch: the surface moves, the strut is republished and rdwm re-tiles. Anything that was open
is closed; no restart.

```json
{ "bar": { "enabled": true, "position": "split" } }
```

| `bar.position` | what you get | strut | popouts and the Control Center open |
|---|---|---|---|
| `left` (default) | a vertical bar down the left edge | `left = 52` over the monitor's rows | to the right of the bar |
| `right` | the same on the right edge | `right = 52` | to the left of the bar |
| `top` | a horizontal bar along the top edge: the vertical bar's content turned 90 degrees (OS icon, workspaces, window title in the middle, tray, status icons, clock as `HH:MM`, power) | `top = 52` over the monitor's columns | downwards |
| `bottom` | the same along the bottom edge | `bottom = 52` | upwards |
| `split` | two bars, as in Caelestia-like and Windows-like desktops: **top** = OS icon, workspaces, active window title; **bottom** = tray, status icons, clock, power (standing on the right end) | two struts: `top = 52` and `bottom = 52` | down from the top bar, up from the bottom bar |

An unknown value (or a key of the wrong type) means `left`, and never spoils the rest of `macshell.json`. The setting only matters
while the theme is Caelestia (`bar_style = "vertical"`: that is the theme's name for the pill bar, whatever edge it is on); with any
other theme `macbar` is the classic menu bar and `bar.position` is remembered but ignored. `macthemectl style macos` keeps it, so
`macthemectl style caelestia` brings the same layout back.

One layout, rotated: `src/menubar/vbar.rs` computes the pills once for a bar running down an edge and maps them onto the surface of
whichever edge is chosen (`Geo`), so hit-testing, popout placement and drawing are shared by all five. Only the things that cannot be
turned differ: the workspace chip is a little wider along a horizontal bar, the clock is one line, the battery lies down, and the window
title is read left to right instead of turned.

### How `split` is run

`split` is two surfaces, so two processes, each with a single-instance lock in `$XDG_RUNTIME_DIR` (`macbar.lock`, `macbar-bottom.lock`;
a second copy of either role exits at once without disturbing the first):

* `macbar` (the main one, the one the session supervisor starts) draws the **top** bar. While the layout is split it also starts
  `macbar --role bottom` and notices (every couple of seconds) when that process has ended, restarting it after a short pause.
* `macbar --role bottom` draws the **bottom** bar (X11 window name `macshell-bar-bottom`, the main one is `macshell-bar`). It leaves by
  itself, within a second, when the layout stops being split, when the theme is not Caelestia, or when `bar.enabled` hides the bar; the
  main bar starts it again when it is wanted. It is not supervised by anything else.
* They share nothing but the settings and the theme: each has its own workers and its own popups. Restarting the menu bar from the
  Dock & Menu Bar pane (`macbar` by name) restarts both.

### The dock

Caelestia has no dock, so it stays hidden at every position (`"dock": {"enabled": "auto"}`, see below). If you set `"enabled": true` the
dock comes back, floating above the bottom bar when the layout is `bottom` or `split` (its margin grows by the bar's 52 px), and clear
of the left and right bars because it is centred on the monitor. The dashboard hangs below a top bar (`top`, `split`), notification banners
keep clear of a top or right bar.

## What changes

| piece | Caelestia style |
|---|---|
| theme | `caelestia` (the shell's shipped scheme, or `macthemectl dynamic on` for Match wallpaper colours) |
| bar | `bar_style = "vertical"`: `macbar` moves to the edge `bar.position` names (`Anchor::Left`, `Right`, `Top` or `Bottom`), 52 px thick, and reserves that strip with `_NET_WM_STRUT_PARTIAL` (e.g. `left = 52`, `left_start_y`/`left_end_y` = the monitor's rows), so rdwm tiles windows beside it |
| dock | `macshell.json` `dock.enabled = "auto"`: hidden while the theme's bar is the pill bar (see below) |
| dashboard, launcher, session, OSD, notifications | unchanged programs, restyled by the theme; positioned for a bar that is not at the top |

### The bar, in reading order

(down the left or right bar, across a top or bottom bar; the sizes below are for a vertical bar)

| item | what it shows | hover / click | wheel |
|---|---|---|---|
| OS icon | your distribution's logo (Nerd Font, from `/etc/os-release`; a grid without a Nerd Font) | click: launcher (`macspot`) | |
| workspaces | pills for rdwm tags 1-9 (more or fewer follow `[general] tags`): the active one filled in the accent colour, occupied ones with a tinted pill, empty ones a dot, **urgent** ones in the danger colour | click a pill: rdwm `view N` | next / previous tag (`viewnext` / `viewprev`) |
| focused window | its icon and its title (turned 90 degrees on a vertical bar; the app name when there is no title); hidden when no window has focus or the bar is too short | hover or click: popup with the app, its title, Close, Minimise, Float or tile | |
| tray | the window layout of rdwm, stay awake, agents, and the quick-settings tile | layout: popup with every layout; tile: the Control Center panel beside the bar (Wi-Fi, Bluetooth, Focus, Night Shift, dark mode, sliders, Now Playing, the record / idle-inhibit / game mode / DND tiles, notification history) | layout: cycle |
| status icons | lock keys (only while Caps or Num Lock is on, X11), sound, Wi-Fi or Ethernet, Bluetooth, battery with its percentage | hover (or click): the same popups the horizontal bar opens: sound with volume and brightness sliders, Wi-Fi list, Bluetooth devices, battery status | sound icon: volume; middle click on it mutes |
| clock | `HH` over `MM` (one line, `HH:MM`, on a horizontal bar) | click: dashboard (`macdash`) | |
| power | the power symbol | click: session menu (`macsession`) | |

On the rest of the strip the wheel works like Caelestia's: the first half of the bar (upper, or left) changes the volume, the second half the
brightness. Popups open on the screen side of the bar next to their icon after a short rest of the pointer (140 ms); sliding to the
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
* `"auto"` hides it while the **theme** has the pill bar (at any position). `macthemectl style caelestia` writes it; `macos` and `windows` write `true`.
  Picking any other theme in the Theme pane brings the dock back by itself, because only Caelestia has the pill bar.
* To keep a dock in the Caelestia style: set `"enabled": true` after choosing the style. The dock then floats above the bottom edge as usual and
  does not collide with the bar (it is centred on the monitor, the bar owns the left 52 px); under a bottom bar it floats above it.

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
* On Wayland the layer-shell surface is anchored to the chosen edge with an exclusive zone, but only X11 (rdwm) has been exercised, and only for the layouts the end-to-end checks cover; lock-key indicators are X11 only.
* `split` is two macbar processes; the dashboard opened from the bottom bar still hangs from the top of the screen (macdash has no bottom anchor), and a top or bottom bar's popouts are limited to the 720 px of surface under (over) the bar.
* Only the primary monitor gets a bar.
