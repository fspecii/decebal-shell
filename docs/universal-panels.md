# Universal panels and applets

Every Decebal bar and taskbar can sit on any screen edge, and every applet can be turned on or off on its own,
whatever the theme. This page is the contract shared by the renderers, Settings, `decthemectl`, the native previews
and theme packages (including the KDE Plasma and GNOME ports). The code is `src/panel/` (`applets.rs` is the
registry, `mod.rs` the resolution, `frame.rs` the edge geometry). The acceptance requirements are in
`docs/universal-panel-requirements.md` (parent branch).

## Implementation status

This section is updated with each commit of the universal-panel work.

- Done: registry, keys, resolution order, user/theme/legacy precedence, workers rule, manifest schema, `LaunchAnchor`.
- In progress: renderer edges and applets, Settings → Panels, `decthemectl panel`, worker gating, previews.

## Surfaces and processes

| surface | program | drawn by | when |
|---|---|---|---|
| the bar | `decbar` (one process; `decbar --role bottom` only for a split rail) | menu bar, floating bar, pill rail or Dux bar, from the theme's `[bar] style` | `bar.enabled` (`auto` = hidden while the dock is the taskbar) |
| the dock | `decdock` | the macOS dock | `dock.enabled` and `dock-settings.json` `style = "mac"` |
| the taskbar | `decdock` (the same process, other style) | the Windows taskbar | `dock.enabled` and `style = "windows"` |

The dock and the taskbar are one process with two looks, so a theme never runs both. The macOS dock holds
applications only. On a theme whose bar is off (a dock-only or empty desktop), Settings → Panels says so: turning an
applet on there also turns the bar on (`bar.enabled = true`, the same single `decbar` process), so every applet can be
enabled on every theme without a second dock or bar process. The bar is a separate process
and may run beside either (a theme with `[bar] enabled = true` and a taskbar dock shows both). When the bar and the
dock or taskbar share an edge, both reserve their strut and the dock/taskbar stands off the bar
(`Theme::bar_insets`), so neither covers the other.

## Edges

Every renderer draws all four edges in its own style. Text and icons stay upright: a vertical panel uses stacked clock
digits, icon cells and short labels instead of rotated text. Popups open on the screen side of the strip.

| renderer | `[bar] style` | top / bottom | left / right |
|---|---|---|---|
| Menu bar | `flat` (default) | the strip with application menus; menus and popups open away from the edge | a 40 px rail; the application menus fold into one menu button |
| Floating bar | `floating` | the floating pill | a rounded floating rail |
| Pill rail | `vertical` | horizontal pills | vertical pills; also `split` (top bar + bottom bar) |
| Dux bar | `dux` | one rounded band with individual widget faces in three layout groups (start / centre / end); the `islands` background profile keeps separate faces | the band turned: the three groups stacked along it |
| Taskbar | dock `style = "windows"` | Windows 11 / 10 taskbar | Windows 10-style vertical taskbar |
| Dock | dock `style = "mac"` | magnifying dock (top mirrors bottom) | dock on the side, magnifying along it (as in macOS) |

## Applets

Keys are stable. Old spellings (the Dux item ids among them) still parse. Every renderer can draw every applet; each
renderer only has its own *default* set (what it showed before applets could be chosen). An enabled applet is absent
only when the system lacks what it needs, and Settings says which (the "needs" column).

| key | aliases | does | needs |
|---|---|---|---|
| `launcher` | `launcherButton`, `logo`, `start` | opens the launcher (`declaunch`, at the applet, see below) | — |
| `app_menu` | `appMenu`, `menus`, `global_menu` | system menu and the focused application's menus | — |
| `workspaces` | `workspaceSwitcher`, `tags` | workspace switcher; the wheel cycles | a WM that reports workspaces |
| `window_title` | `focusedWindow`, `window` | focused window; its menu (close, minimise) | — |
| `tasks` | `taskButtons` | task buttons: focus, minimise, launch pinned | a WM that publishes its window list |
| `layout` | — | tiling layout picker; the wheel cycles | rdwm |
| `media` | `music`, `mpris` | track, play/pause, previous, next | an MPRIS player |
| `clock` | `date` | clock and date; the calendar | — |
| `weather` | — | conditions; the forecast | a weather reading (wttr.in) |
| `tray` | `systemTray`, `indicators` | StatusNotifierItem applications and their menus | tray applications |
| `clipboard` | — | clipboard history (`decclip`) | `decclip` |
| `cpu` | `cpuUsage` | processor load; the process monitor | — |
| `memory` | `memUsage`, `ram` | memory use; the process monitor | — |
| `notifications` | `notificationButton` | notification history (`decnotify history`) | `decnotify` |
| `stay_awake` | `awake`, `caffeine` | Stay Awake: turn on/off; right click: timed sessions | by default shown while on; turned on explicitly it stays as a control |
| `agents` | `process_monitor` | the process monitor (`decmon`) | `decmon` |
| `lock_keys` | `lock`, `capslock` | Caps / Num Lock | by default shown while on; turned on explicitly it always shows their state |
| `volume` | `audio`, `sound` | output slider; wheel, middle-click mute | an audio output |
| `network` | `wifi` | Wi-Fi networks, connection editor | a network interface |
| `bluetooth` | — | paired devices | a Bluetooth adapter |
| `battery` | — | charge details | a battery |
| `brightness` | `backlight` | brightness slider; the wheel | a backlight |
| `search` | `spotlight` | search (`decspot`) | `decspot` |
| `dashboard` | `dash` | the dashboard (`decdash`) beside the applet | `decdash` |
| `control_center` | `controlCenterButton`, `control` | quick settings | `control_center.enabled` |
| `power` | `session` | the session menu (`decsession`) | `decsession` |
| `show_desktop` | `showDesktop`, `peek` | minimise all / restore | a WM that publishes its window list |

Default sets (when neither the user nor the theme says otherwise):

| renderer | default applets |
|---|---|
| Menu bar, Floating bar | `app_menu`, `workspaces`, `layout`, `weather`, `stay_awake`, `agents`, `tray`, `battery`, `bluetooth`, `volume`, `network`, `dashboard`, `search`, `control_center`, `clock` |
| Pill rail | `launcher`, `workspaces`, `window_title`, `layout`, `stay_awake`, `agents`, `control_center`, `lock_keys`, `weather`, `volume`, `network`, `bluetooth`, `battery`, `tray`, `clock`, `power` |
| Dux bar | the items of `bar.left` / `center` / `right` (default: `launcher`, `workspaces`, `window_title`, `media`, `clock`, `weather`, `tray`, `clipboard`, `cpu`, `memory`, `notifications`, `battery`, `control_center`) |
| Taskbar | `launcher`, `search`, `tasks`, `layout`, `weather`, `bluetooth`, `network`, `volume`, `battery`, `tray`, `control_center`, `clock`, `show_desktop` |

Each renderer places applets in its own reading order (the menu bar keeps menus at the start and status items at the
end; the rail keeps head, window and tail pills; Dux keeps its three layout groups, and an applet enabled outside the Dux lists
joins the group it belongs to (launcher/menus/workspaces/window/tasks start, media/clock/weather centre, the rest end); the taskbar keeps Start/search/tasks then tray/clock). Applet order is not configurable yet.

### Workers

Turning an applet off stops the work only it needed: the weather fetch (`weather`), the MPRIS subscription
(`media`), CPU/memory sampling (`cpu`, `memory`), the lock-key watcher (`lock_keys`), the tray host (`tray`), the
layout/tag subscription (`layout`, `workspaces`) and the volume subscription (`volume`; kept while the Control Center
button is shown, whose sound tile needs it). Probes shared with the Control Center (battery, network, Bluetooth) stay
as long as either needs them. `panel::workers_for` is the rule.

## Theme packages (`theme.toml`)

```toml
[bar]
style = "flat"            # flat | floating | vertical | dux: the renderer (unchanged)
position = "bottom"       # this theme's default edge: top | bottom | left | right (| split, vertical style only)
applets = ["launcher", "tasks", "tray", "volume", "network", "battery", "clock", "show_desktop"]   # optional
# Dux only: left / center / right item lists still give the island order (and the default set when `applets` is absent)

[dock]
style = "windows"         # mac | windows
position = "bottom"       # this theme's default dock / taskbar edge: top | bottom | left | right
```

- `position` and `applets` are *defaults*. They are read from the package each time the theme is built and never copied
  over the user's choices. A palette, accent, mode or wallpaper-colour change does not touch the panel.
- `applets` is the full default set: keys not listed are off by default (the user can turn them on). Leave it out to
  keep the renderer's default set. Unknown *applet keys inside the list* are ignored by this engine, so a package that
  names an applet from a later release still loads here.
- Compatibility with older Decebal, checked against the 0.2.0 validator: it rejects unknown keys, so a package with
  `[bar] applets` fails to install there ("unknown key"). `[bar] position` and `[dock] position` are known keys there and
  validate, but 0.2.0 draws the menu bar and Dux bar only on top and the taskbar only top/bottom (a left/right dock
  position is left out with a note). A package that uses `[bar] applets`, or relies on a non-top edge for a non-rail bar or
  a side dock, must declare `min_shell_version` as the first release that ships this framework. The parent assigns
  that number at integration (`crate::panel::SINCE`); `decthemectl theme pack`/export writes it automatically for such
  packages. Older packages and configurations keep loading unchanged.
- No per-theme special-case settings: a Plasma-like port is `[dock] style = "windows"` with `[bar] enabled = "auto"`
  (or a flat bar on the bottom), a GNOME-like port is `[bar] style = "dux"` with `left = ["launcher", "workspaces"]`,
  `center = ["clock"]`, `right = ["tray", "volume", "network", "battery", "power"]` and `[dock] enabled = false`, or a
  flat bar with its default set.

## User settings

`decebal.json`:

```json
"bar": {
  "edge": "left",
  "split": false,
  "applets": { "weather": false, "media": true }
}
```

`dock-settings.json`: `"edge": "left"` (dock or taskbar).

- `bar.edge`: the user's bar edge, for every bar style. `bar.split`: rail only.
- `bar.applets`: one boolean per key, for the bar and the taskbar. A key that is absent follows the theme.
- Settings → Panels writes these; "Use theme default" deletes them (and nothing else). `decthemectl panel` does the
  same from a terminal.
- They persist across theme, palette, mode and accent changes. Applying another theme keeps them; that is the only
  theme-switch behaviour, so a reset is always one explicit action.

### Resolution (first match wins)

| value | 1. user | 2. earlier key | 3. theme package | 4. renderer |
|---|---|---|---|---|
| bar edge | `bar.edge` / `bar.split` | `bar.position` (pill rail only, as before) | `[bar] position` | top (rail: left) |
| dock / taskbar edge | `dock-settings.json` `edge` | `windowsPosition = "top"` (taskbar) | `[dock] position` | bottom |
| applet | `bar.applets.<key>` | `workspaces: false` | `[bar] applets` (Dux: its lists) | default set |

Old configurations keep exactly their look: without the new keys the menu bar and Dux bar stay on top, the rail stays
where `bar.position` put it, the taskbar where `windowsPosition` put it, and every renderer shows its default set.
Settings writes `bar.position` too when the user picks an edge, so an older `decbar` agrees.

## Programs opened at an applet

A bar starts a program that opens beside an applet with

```
--edge top|bottom|left|right --anchor X,Y --thickness N
```

`X,Y` is the applet's centre in global logical pixels (X11 root coordinates; on Wayland output-local), `N` the panel's
thickness. `declaunch` (the launcher styles, owned by the launcher worker, see `docs/launcher-styles.md`) reads these;
without them a program uses its own natural placement. `panel::frame::LaunchAnchor` writes and parses the flags.

## Native previews

The theme gallery's generated preview draws the effective edge and applets of the theme being previewed (with the
user's own panel choices applied), and its cache key includes `Effective::fingerprint()` (renderer, edge, split,
applet bits), so a change of edge or applets regenerates it and nothing else does.
