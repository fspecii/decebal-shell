# Universal panels and applets

Every Decebal bar and taskbar can sit on any screen edge, and every applet can be turned on or off on its own,
whatever the theme. This page is the contract shared by the renderers, Settings, `decthemectl`, the native previews
and theme packages (including the KDE Plasma and GNOME ports). The code is `src/panel/` (`applets.rs` is the
registry, `mod.rs` the resolution, `frame.rs` the edge geometry). The acceptance requirements are in
`docs/universal-panel-requirements.md` (parent branch).

## Implementation status

Done (0.3.0 batch): the registry, keys and resolution order; every bar style (menu bar, floating bar, pill rail, Dux band)
on all four edges with every applet, overflow lists and worker gating; the Windows taskbar on all four edges with every
applet; the macOS dock on all four edges; `[branding]`; Settings → Panels; `decthemectl panel`; keyboard mode
(`decbar --focus`, `decdock --focus` for the taskbar); every bar style at the display scale on X11 (`DECEBAL_SCALE` /
`Xft.dpi`, checked at 100/125/200%); previews that show the effective panels and key their cache on them.

Known limits:

- The macOS dock has no keyboard mode (its menus take Up/Down/Enter/Escape); the taskbar's is `decdock --focus`.
- On a side taskbar, a StatusNotifierItem's Activate gets the item's along/across position as its x/y hint.
- `decbar --control-center` summoned by a key or hot corner follows the resolved edge of the bar or taskbar
  (`Host::Panel` for a bottom or side bar and a side taskbar; `auto` opens at the tray end, clear of the strip at the
  display scale). On X11 it shares the panels' monitor (RandR primary); under Wayland the compositor picks the output
  of each layer surface, so with several outputs it can open on another output than the bar.
- The responsive Control Center (below) zooms a look that has no narrower layout (Dux 600, card/fluent 340 logical px) when
  the host's room is narrower; the zoomed panel is smaller to read. Under Wayland the compositor decides a surface's size, so
  a side surface asking for more than the output is not bounded by Decebal there.
- Applet order inside a renderer is fixed (the renderer's own reading order).
- Native evidence is X11 (private Xvfb without a window manager: strut hints are checked as values, not as a work area
  a WM honoured). Wayland layer-shell anchoring is unit-tested only. Latest runs (source 9336240, debug binaries decbar
  `64cb3037…`, decdock `88eba4f7…`): `/var/tmp/decebal-panel-evidence/9336240-matrix` (105 cases: four bar styles × four
  edges × light/dark × all/default/none applets, bars at 1/1.25/2× with summoned Control Center clearance, taskbar and dock
  on four edges, taskbar `--focus`; 105 of 105 after the two checker corrections stated in its SUMMARY.md) and
  `/var/tmp/decebal-panel-evidence/9336240-probes` (recorded probe calls: none with only the clock, none after switching
  all applets off live).

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
| `app_menu` | `appMenu`, `menus`, `global_menu` | system menu and generic menus named after the focused application (common shortcuts and window commands, not the application's own menus; those need the exported-menu bridge, `docs/appmenu.md` on its branch) | — |
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

### Responsive Control Center

Every host gives the Control Center the room it has: the menu bar the output's width, a rail the rest of its surface on the
screen side (side rails ask for the look's natural width; the X11 backend never makes a surface larger than its monitor and
tells the app the size it got), the taskbar's engine the strip's length or the surface beside a side strip, the summoned
overlay its corner's room. `controlcenter::layout_fitted(snapshot, tiles, style, room)` lays the look out for that room:

- GNOME: two `12em` columns when they fit, else one column (toggles stacked; the battery readout and the Now Playing controls
  on lines of their own). Every control stays.
- Any look still wider than the room (Dux's 600 px grid, card/fluent 340 px, GNOME's one column at a large text scale) is
  drawn at a zoom `room / width`: sharp (drawn at render scale × zoom), and hit-tested through the same zoom.
- Every look is clipped to the height it has and scrolls (wheel; keyboard focus reveals the control; a scroll thumb shows
  the position). Up/Down walk every control, Enter runs it, Left/Right step a focused volume or brightness slider by 5 %.

`DECBAR_DEBUG_PANEL=file` reports, from every host, the panel rectangle, zoom, scroll and each control where it is drawn
now (surface coordinates, logical pixels), plus the keyboard focus and the pointer's hover.

### Keyboard

`decbar --focus` (bind it to a key, as KDE binds "focus the panel") puts the running bar into keyboard mode: a focus
ring sits on the first applet, Left/Right (or Up/Down) move it in reading order, Enter or Space does what a click does
(the same typed action, no synthesized pointer events), the arrows walk the opened popup's rows, Left/Right step its
slider, the calendar's month or (in the application menu) back, and move between the classic bar's menus; Escape closes
what is open, then leaves keyboard mode. The bar never takes the keyboard by itself. The Control Center keeps its own
keyboard routing.

`decdock --focus` does the same for the running Windows-style taskbar on any edge: the ring walks Start, search, each
task button (a button scrolled out of the row scrolls into view), the tray cells, the clock and show desktop. Enter on a
tray cell or the clock opens its popup with the first row focused and the popup's keys apply (the quick settings keep
the Control Center's routing); Enter on Start, search, a task or show desktop does the click and leaves keyboard mode,
since the launcher or window takes the focus. Escape closes an open popup or menu, then leaves; a pointer press leaves.

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
- `[branding]` (optional, data only) gives every renderer the theme's own identity instead of the style's:

  ```toml
  [branding]
  logo = "icons/plasma.png"      # a package image (png/jpg/webp), or "icon:start-here-kde" (the user's icon theme),
                                 # or "builtin:apple|windows|os|grid|none"
  logo_tint = true               # repaint it in the bar's text colour (symbolic logos)
  launcher_label = "Activities"  # text on the launcher of horizontal bars (rails show the logo)
  desktop_label = "Desktop"      # what the menu bar names when no window is focused (default "Finder")
  ```

  The logo replaces the menu bar's Apple menu glyph and launcher glyph, the rail's OS logo, the Dux launcher glyph and
  the taskbar's Start squares. A package image must be inside the package (checked when installing; a manifest whose
  path leaves the package does not load). Only an *absent* `logo` keeps the style's own logo. An explicit logo that
  cannot be had at run time (an icon name the user's icon theme lacks, such as `icon:start-here-kde-symbolic` without
  Breeze installed; a missing or corrupt image) is drawn as the neutral `builtin:grid` in every renderer, never as
  Apple or the Windows squares, and the labels stay. `builtin:none` draws no logo (the label alone; the taskbar's
  Start, which needs a target, shows the neutral grid). Without `[branding]` nothing changes: the macOS bar keeps Apple and "Finder", the
  taskbar its four squares, the rail the distribution logo.
- Compatibility with older Decebal, checked against the 0.2.0 validator: it rejects unknown keys, so a package with
  `[bar] applets` fails to install there ("unknown key"). `[bar] position` and `[dock] position` are known keys there and
  validate, but 0.2.0 draws the menu bar and Dux bar only on top and the taskbar only top/bottom (a left/right dock
  position is left out with a note). A package that uses `[bar] applets` or `[branding]`, or relies on a non-top edge for a non-rail bar or
  a side dock, must declare `min_shell_version = "0.3.0"`, the first release that ships this framework
  (`crate::panel::SINCE`); `decthemectl theme pack`/export writes it automatically for such
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
