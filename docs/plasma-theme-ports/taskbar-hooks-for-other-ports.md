# Reusing the generic `[taskbar]` hooks in another desktop port (OpenKylin / UKUI and others)

The Plasma work added one optional, typed, theme-neutral section to the theme package format. Nothing in the renderer names a theme id: a
package that writes no `[taskbar]` keys gets exactly the Windows 11 / Windows 10 look it had before. These are the keys another port can
set today (all wired, validated by the strict manifest validator, sizes in px at scale 1.0; `src/theme/engine/schema.rs` is the list of record):

| Key | Range | What it does | OpenKylin-style value the owner would write |
| --- | --- | --- | --- |
| `[taskbar] thickness` | 24..96 | height of a top/bottom strip, width of a side strip | `48` |
| `[taskbar] icon_size` | 12..64 | drawn size of task and Start icons; absent with a thickness = thickness minus two paddings | `32` |
| `[taskbar] cell_width` | 24..120 | width of a task or Start cell along the strip | e.g. `48` |
| `[taskbar] gap` | 0..16 | space between cells | e.g. `4` |
| `[taskbar] inset` | 0..16 | how far a cell's plate stays inside the strip (0 = fills it) | e.g. `4` |
| `[taskbar] float_gap` | 0..32 | distance between the strip and the screen edge and both ends (a floating panel); 0 = flush | `0` flush, or `8` floating |
| `[taskbar] indicator` | `default` / `frame` | task state marking: the renderer's own, or a tinted cell with a strong line on the side facing the desktop | `default` unless the port wants Plasma's marking |
| `[taskbar] desktop` | `sliver` / `icon` | show-desktop applet form | `icon` |
| `[taskbar] workspaces` | `name` / `cells` | workspace applet: the name, or a pager (cells follow the real desktop shape when the window manager reports it) | `cells` if wanted |
| `[dock] style = "windows"` | | the taskbar renderer instead of the floating dock | required |
| `[dock] align` | `center` / `left` | task icon alignment (existing key) | `center` for centred icons |
| `[dock] position` | edge | default edge; the user's Settings choice wins | `bottom` |

A package that uses any `[taskbar]` key must declare `min_shell_version = "0.3.0"` (the writer and validator do this automatically).

## Centred "islands"

`[dock] align = "center"` centres the icon group between Start and the tray, which is the centred-island look of a 48 px / 32 px strip.
Separate rounded *islands* (the start group, the tasks and the tray each on their own rounded plate) are not a `[taskbar]` key: the renderer
draws one strip. If the OpenKylin design needs per-group plates it is an engine request (a typed `taskbar.groups = "islands"` with a plate
radius), not something to emulate with `inset`. It has not been implemented here and nothing in this branch claims it.

## What the window state gives any port

The window tracker now reports real per-window state, which every port inherits through the same renderer:

* `minimized` (EWMH `_NET_WM_STATE_HIDDEN`; Wayland foreign-toplevel `minimized`),
* `attention` (EWMH `_NET_WM_STATE_DEMANDS_ATTENTION` or the ICCCM urgency hint; never set on Wayland, which has no such state),
* `geometry`: the client rectangle in root coordinates (X11 only; `None` on Wayland, for a minimised window or when the request fails; the map state is not queried) and the desktop size (`_NET_DESKTOP_GEOMETRY`, else the root window).

With `indicator = "frame"` the minimised and attention states are painted. Under the default indicator they are carried in the model but not
painted (the Windows looks keep their own marks); a port that wants them under its own marking adds a state to the typed indicator rather than
a theme condition.

## Not done and not claimed

* No file of the OpenKylin package was touched; this note only lists the hooks. The owner picks the values and runs the same strict validator
  and the native four-edge gate (`tests/e2e/universal-panels-native.py`, `tests/e2e/plasma-wm-native.py`) against their own package.
* Icon sizes are pixels at scale 1.0 and the strip is native-pixel exact only at integer and 1.25 scales the gates cover (100 / 125 / 200 %).

## Integration scope

The shared tracker and renderer are integrated independently of the Plasma theme packages. Breeze and Layan remain outside this acceptance scope. The real-WM harness requires an explicitly supplied `plasma-breeze` fixture; it does not make that package a shipped or accepted theme. Its virtual X11 proof does not establish hardware GPU/input or Wayland behavior.
