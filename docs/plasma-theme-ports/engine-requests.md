# Engine requests from the Plasma ports (for the universal-panel owner, via the root)

Generic renderer/engine requirements with primary-source evidence. They are **not** palette substitutes: a palette cannot move an
indicator to the right side of a task, rebuild a pager or lay out a vertical strip. None of these needs a theme-specific code path
or a theme id; each is a property any theme could use. Sources: plasma-desktop `c68d33a9c7cd8e3d32c989a9eadf95e25cc6e79c`,
plasma-workspace `dec158c08fbda8f1dbfd3a86449977dde6347146`, libplasma `6a9ded3bab634d4dbea6683d727d956e278c582c`, kirigami
`1ea0abb59d20705832c121eeebd06e1954f15d0c` (pinned clones in `~/.cache/decebal-plasma-refs`). Observations are from native previews
made with shared commits `71e3434` and `b28cca5` (`decthemectl theme render`, fixed sample data).

## Implementation status (updated: the requests are implemented on the Plasma branch, merged on root `8b3f822`)

Root approved the API and the wiring (not helpers): `src/panel/plasma_style.rs` holds the pure typed model and formulas, and the real
taskbar model, painter, dock-app geometry and native preview read it. Everything is a typed, theme-neutral `[taskbar]` key set; a package
without the section gets `TaskbarStyle::default()` and the Windows 11 and Windows 10 looks are unchanged (tests compare the metrics byte
for byte). No theme id appears in any condition.

| Request | State | Where |
| --- | --- | --- |
| R1 stateful inner-edge indicator | **Done** for the states the shell knows: normal, hover, active (and the active task while dragged), on all four edges, strong line towards the desktop, numbers from tasks.svg. `minimized` and `attention` are now drawn from real window state: the tracker reports `_NET_WM_STATE_HIDDEN`, `_NET_WM_STATE_DEMANDS_ATTENTION` and the ICCCM urgency hint (Wayland: foreign-toplevel `minimized`; urgency is never reported there), `TaskButton` carries both flags and `task_state` orders them focus > pointer > attention > minimised. Proven natively under rdwm (see "Real window manager proof"). | `[taskbar] indicator = "frame"`; `taskbar::draw::draw_frame_cell` |
| R2 thickness, gap, padding, icon | **Done**: `thickness`, `icon_size` (derived from thickness by Plasma's padding rule when absent), `cell_width`, `gap`, `inset`, `float_gap` (a rounded floating plate; surface, strut, input region, popups and Control Center placement follow it; clicks in the gap pass through). | `Metrics::styled`; `dock_app/windows.rs` strip geometry |
| R3 actual pager | **Done**: one cell per workspace on the **real desktop shape** (`_NET_DESKTOP_GEOMETRY`, else the root window; 16:9 only when the window manager reports no size, i.e. Wayland), current cell highlighted, each workspace's windows drawn at their real geometry (clamped to the desktop; minimised windows and windows without geometry are not drawn, an occupied workspace without geometry keeps the old centre plate), an urgent workspace tinted with the warning colour, upright on side strips, click switches the workspace, widens the applet through the layout and the tray capacity. The window outline colours (ink / accent at fixed alphas) are an approximation of Plasma's pager item, not taken from its SVG. | `[taskbar] workspaces = "cells"`; `plasma_style::pager_cells` |
| R4 side strips | **Done and gated**: with the merged root code the Breeze side strips are full-length, correctly ordered and Plasma-sized; `breeze_taskbar_is_a_full_length_floating_native_strip_on_every_edge` is a normal test (no longer ignored). Native Xvfb proof: `tests/e2e/plasma-panel-native.py` (see the root document for its results). One root bug found: the preview subtracted a *hidden* bar's thickness from the taskbar's edge (`Theme::bar_insets` ignores `bar.enabled = "auto"`); fixed in the preview hook only, the runtime taskbar surface never used it. `Theme::bar_insets` itself still reports a hidden bar and the Mac dock's `surface_spec` still uses it. | `dock_app::preview_desktop_with` |
| R5 text-colour Start tint | **Done**: `[branding] logo_tint` repaints in the strip's text colour on the taskbar too (an untinted logo keeps the accent). | `Painter::draw_start` |
| R7 show desktop, clock | **Done** for the icon form: `[taskbar] desktop = "icon"` draws a square cell with the desktop glyph (the sliver stays the default). The clock already stacks time over date on every edge. | `Painter::draw_desktop` |

Still not done, and not claimed: Plasma's application-menu applet (bridge owner), the Plasma popups (Kickoff, tray, calendar), Wayland
window geometry and urgency (the foreign-toplevel protocol carries neither, so the pager there keeps the 16:9 shape and the centre plate and no
task is ever painted as demanding attention), and minimised / attention painting under the default (Windows) indicator, which keeps its own marks.

### Real window manager proof (immutable rdwm, private Xvfb)

`tests/e2e/plasma-wm-native.py` drives the real `decdock` under the real `rdwm` release at `/var/tmp/rdwm-layout-decorations-release-artifacts-v2/release`
(binary hashes in `native/wm/wm-report.json`) on an owned private Xvfb (display >= 700, ownership verified in both socket namespaces before any
client; the client environment is built from scratch and every client call refuses to run before that proof). Fake clients are python-xlib windows.
**24 / 24 cases pass**: bottom, top, left, right x 100 / 125 / 200 % x dark and light. Per case: the dock's strut equals the work area rdwm
reports (`_NET_WORKAREA` and `rdwmctl state`) exactly and every client lies inside it; real clicks focus a task, minimise the focused one
(rdwm sets `_NET_WM_STATE_HIDDEN`, the tracker flags it, the task is painted minimised) and restore it; an urgency hint and an EWMH
`DEMANDS_ATTENTION` state each tint their task in the warning colour and clear again; real clicks on pager cells switch `_NET_CURRENT_DESKTOP`
and back, and the busy workspace's cell differs from an empty one by its window outlines; a clock click opens a popup that stays on the output
and Escape closes it; clicks in the floating gap neither focus a task nor open anything; the clock text keeps WCAG contrast >= 9.3 (dark >= 16.1).
Captures: `native/wm/*.png`. Earlier runs of the same script failed and are kept, not hidden: 8 failures on the side strips (the dock's layout dump
omitted the floating-gap offset along the strip, so probes landed in the neighbouring pager cell / task) and, after fixing the dump, 16 failures
caused by the test's own probe patch sitting on the constant icon; both were fixes to the hook / probe, none to the product behaviour.
**Tracker robustness (review findings, fixed).** The X11 collector reads queued events in bounded passes (at most 64 events or 25 ms),
checks the last-owner stop between events and again before it publishes a snapshot, so a producer that never lets the queue run dry (a window
drag, a client rewriting a property in a loop) can neither delay a rebuild nor outlive its owner; deterministic tests drive continuous, slow and
stopping producers. The root window now selects structure events as well as properties, so a root resize refreshes the snapshot when the window
manager publishes no `_NET_DESKTOP_GEOMETRY` (rdwm does not); a test follows the fallback through a resize. A tracker change that only moves or
resizes a window (or only resizes the desktop) now repaints the pager: the dock compares exactly what the pager draws on each wake, from the
tracker's snapshot, never by asking the display (`dock_app` regression, checked to fail without the comparison). Native sequence: with the
pointer parked, rdwm `setlayout monocle` then `setlayout tile` resizes every window (a ConfigureNotify burst, no property or focus change) and the
pager cell must change and change back. `Window::geometry` is the client rectangle in root coordinates (no frame or decoration), `None` for a
minimised window, on Wayland, or when the request fails; the map state is never queried.

Not covered: Wayland sessions, a native root-window resize (covered by the unit test only), a keyboard walk of the taskbar under rdwm (the unit test `decdock_focus_walks_the_taskbar_by_keyboard_on_every_edge`
covers the keyboard model; the native run only exercises Escape), more than three windows, and windows on other workspaces.

## R1. Active / hover / minimised / attention task indicator, per edge

Plasma does not draw a generic highlight. `applets/taskmanager/qml/Task.qml` (468–483, 640–690) selects an SVG frame by state:

| state | base prefix | source condition |
| --- | --- | --- |
| normal | `normal` | default |
| hovered | `hover` (then the state's own prefix as fallback) | `highlighted && taskHoverEffect` |
| active window | `focus` | `IsActive` |
| minimised | `minimized` | `IsMinimized` |
| demands attention | `attention` | `IsDemandingAttention` or an urgent launcher badge |
| launcher (not running) | none (empty prefix) | `IsLauncher` |

`code/TaskTools.js taskPrefix(prefix, location)` prepends the edge: left → `west-`, top → `north-`, right → `east-`, otherwise (bottom)
`south-`, always falling back to the plain prefix. `libplasma/src/desktoptheme/breeze/widgets/tasks.svg` implements them:

- `focus-*` (bottom panel): an outline frame in `ColorScheme-ButtonFocus` (the accent, #3daee9) at **opacity .45**, 1 px outer plus 2 px
  inner line, with the edge nearest the screen interior (`focus-top` on a bottom panel) at **opacity .90**. The `north-focus-bottom`
  (top panel), `west-focus-right` and `east-focus-left` sets put the strong 2 px line on the *interior-facing* side of each edge.
- `hover-*`: `ColorScheme-ButtonHover` fill at opacity .34. `minimized-*`: `ColorScheme-Text` at .08. `attention-*`:
  `ColorScheme-NeutralText` at .35.
- frame margins `normal-hint-{top,bottom,left,right}-margin` are 4 px; the icon margin is `round(smallSpacing/4)` = 1
  (`code/LayoutMetrics.js`).

Request: a taskbar/dock cell style `plasma` (name free) that draws the frame above in the theme's accent/text/warn tokens, with the
strong line on the side facing the screen interior for each of the four edges, plus hover/minimised/attention states. The current
Windows-style taskbar draws a Windows 10 underline (classic) or a rounded plate (Windows 11); neither matches.

## R2. Panel metrics (all edges, same values)

- **Thickness.** `layout-templates/org.kde.plasma.desktop.defaultPanel/contents/layout.js`: `panel.height = 2*ceil(gridUnit*2.5/2)`
  (46 at gridUnit 18; the upstream Breeze preview image shows 44). On a vertical panel this value is the width. Same number on every edge.
- **Floating gap.** `desktoppackage/contents/views/Panel.qml`: floating panels leave 8 px to the screen edges when the Plasma theme has no
  `floating-*` SVG prefix (Breeze has none), otherwise the SVG's fixed margins. Work-area reservation excludes the gap (panelview.cpp
  reserves thickness plus gap in current Plasma; not read here, verify before use).
  Observed: the Breeze bottom taskbar preview is 44 px tall at `71e3434` but about **70 px** at `b28cca5` (hairline at y = 1010 of a 1080 px
  desktop), well above Plasma's 44–46. Request: keep Plasma-sized thickness for this style at scale 1.
- **Inner padding.** `Panel.qml` top/bottom/left/right padding = `round(min(thickSvgMargin + smallSpacing(4), spacingAtMinSize))` with
  `spacingAtMinSize = floor(max(1, thickness − iconSizes.smallMedium(22)) / 2)`.
- **Icon size.** The task icon is drawn at `thickness − 2 × inner padding` = 44 − 2×8 = **28 px** (measured about 28 px in upstream's
  `fullscreenpreview.jpg`, 44 px panel); the highlight cell fills the full panel height. Kirigami `roundedIconSize` (<16 as is; 16–21 → 16;
  22–31 → 22; 32–47 → 32; 48–63 → 48) only selects the source image (22 here); it does not set the drawn size. The Breeze package declares
  `dock.size = 28` accordingly (an earlier revision said 32, which was wrong).
- **Fill and radius.** panel fill alpha .85 (Breeze translucent SVG), corner radius 5 (kirigami `cornerRadius`). Packages already declare
  `dock_alpha`/`bar_alpha`; the engine should apply the same radius and gap on vertical panels as on horizontal ones.

## R3. Workspaces as pager cells, not dots

`applets/pager/qml/main.qml`: one **cell per desktop** (and per activity for the activity pager), each a miniature of the screen with
the screen's aspect ratio (`pagerModel.pagerItemSize`; the whole grid's `aspectRatio` sizes the applet: `minimumWidth = height *
aspectRatio` on horizontal panels, `minimumHeight = width / aspectRatio` on vertical ones, which fill the panel width). Cells show the
desktop's windows as rectangles (text colour at alpha .6 active-on-active, .35 inactive-on-active, .5 active, with a full-colour border
for the active window), the label is `index + 1` or the desktop name (`displayedText`), and the frame comes from `widgets/pager`
prefixes `normal`/`hover`/`active`. The pager hides itself when `pagerModel.shouldShowPager` is false (it follows the desktop and
running-activity counts; `pagermodel.cpp`). Click switches, the wheel cycles.

Request: a `workspaces` presentation (theme- or user-selectable alongside dots and Dux pills), for example `[bar] workspace_style =
"cells"`, drawing miniature cells with window rectangles, upright and stacked on side strips; hidden when there is a single workspace.
The current dot/pill form is not Plasma's pager.

## R4. Side Breeze strips are collapsed: blocker

At `71e3434`, `b28cca5` and `8042ff7`, `decthemectl theme render plasma-breeze --mode dark` with `[bar]`/`[dock]` set to `left` or `right` draws
**no full-height strip**: the interior of the left render contains no panel hairline, and only a ~40×80 px cluster (the Kickoff glyph
overlapping the stacked clock) appears at the foot of the screen. The bottom render, by contrast, has a 1 px `#404345` hairline at
y = 1036 across the whole width (44 px above the 1080 px desktop area).

Request and acceptance (positive, native):

1. On left and right, a full-height strip of Plasma's thickness (40–52 px; 44–46 in the source) with a hairline of the panel border colour along its
   interior edge covering at least 90 % of the desktop height, Kickoff's logo first (top), tasks stacked, tray, the clock as stacked
   upright digits near the foot, the show-desktop item last; text and icons upright.
2. The same on top and bottom with the hairline across at least 90 % of the width.
3. `tests/plasma_theme_ports.rs::breeze_side_strips_are_full_height_native_panels` encodes (1) and (2) as pixel assertions on the real
   native preview; it is `#[ignore = "BLOCKER ..."]` today and must be un-ignored (and pass) on the final engine. Running it with
   `--ignored` on `b28cca5` fails on the left and right strips, which is the reproducible blocker evidence.

## R5. Start/launcher tint

`logo_tint = true` means "repaint in the bar's text colour". On the taskbar's Start the Kickoff glyph (`start-here-kde-symbolic`, a
`currentColor`/`ColorScheme-Text` icon in breeze-icons) is drawn in the **accent** colour in the previews, while Layan's flat bar draws it
in the text colour. Plasma's Kickoff glyph is foreground-coloured. Request: honour the text colour on the taskbar too.

## R6. Application menu

Not a renderer request: a genuine `app_menu` needs a menu exporter service. Exact API and consequences are in
`../plasma-theme-ports.md`, gap 1 (`com.canonical.AppMenu.Registrar`, `RegisterWindow`, `UnregisterWindow`, `GetMenuForWindow`, window
properties `_KDE_NET_WM_APPMENU_SERVICE_NAME`/`_OBJECT_PATH`, `com.canonical.dbusmenu`). Until then `app_menu` must show as unavailable,
naming the missing service, never fixed `File Edit View` titles; and with `launcher` also enabled it must not draw a second logo.

## R7. Smaller forms

- Show desktop: `applets/showdesktop` is a square `user-desktop-symbolic` icon of `iconSizes.medium` (32), peeking via `desktop-symbolic`,
  not a thin sliver.
- Clock: `digital-clock/DigitalClock.qml`, state `verticalPanel`: time above date, fitted to the panel width, never rotated.
- Neutral logo nit (from the b28cca5 review): the taskbar's `builtin:grid` still reads as four squares beside the Windows mark.

## Proposed API for pure geometry helpers (only after the root agrees; no code written)

A new, isolated `src/panel/plasma_style.rs` (pure functions and plain structs, no drawing, no I/O, no changes to existing taskbar or scaling
blocks) could expose, for the renderer owner to adopt or ignore:

```rust
pub struct PanelMetrics { pub thickness: u32, pub float_gap: u32, pub inner_pad: u32, pub icon: u32 }
pub fn panel_metrics(grid_unit: u32, theme_has_floating_prefix: bool, thick_margin: u32) -> PanelMetrics;   // R2 formulas
pub fn snap_icon(px: u32) -> u32;                                                                            // roundedIconSize
pub enum TaskState { Normal, Hover, Active, Minimized, Attention, Launcher }
pub struct IndicatorSpec { pub frame_alpha: f32, pub strong_side: Edge, pub strong_alpha: f32, pub strong_px: u32, pub fill_alpha: f32 }
pub fn task_indicator(state: TaskState, panel_edge: Edge) -> IndicatorSpec;                                   // R1 tables
pub struct PagerCell { pub w: f32, pub h: f32 }
pub fn pager_cells(desktops: u32, rows: u32, screen_w: f32, screen_h: f32, thickness: f32, vertical: bool) -> Vec<PagerCell>;  // R3
```

with unit tests that reproduce each source number above. **Not started: waiting for the root's agreement on the API and file ownership.**


## Findings on root `7272017` that need a generic-renderer decision (reported, not patched)

**R8. The taskbar's Start (and Search) give the launcher no panel anchor.** `dock_app/windows.rs`, `Hit::Start`, runs
`effects.launch("launchpad")`, which starts `declaunch` with no `--edge`, `--anchor` or `--thickness` (the bar's launcher applet does pass them:
`menubar::applets::open_launcher` with `panel::frame::LaunchAnchor`). `docs/launcher-styles.md` says an anchored style opens from the applet towards
the screen, centred on it; without the flags it opens in its natural place. Measured under the pinned rdwm with the real `declaunch`
(`[launcher] style = "kde"`, Kickoff): bottom edge, 100 %: the card is 1 px above the strip but centred on the screen, 915 px from the Start button;
left edge, 100 %: the card is 598 px from the strip, nowhere near the panel (`native/727/finding-*-kickoff-*.png`). So Kickoff opens on the desktop side
of the panel on every edge (inside the output, clear of the strip) but does not hang from its button. Proposed fix (about ten lines, not applied):
run `declaunch toggle` with `LaunchAnchor { edge, x, y, thickness }` built from the Start cell's centre in root pixels and the strip thickness (the
same call the bar makes). `tests/e2e/plasma-packages-native.py --require-anchor` turns the measured gap into a failure so the fix is gated.
Search has the same shape (`search_argv()` is not given the anchor); KRunner is a centred floating box, so Search is correct as it is.

**R9. `launcher` and `app_menu` together draw the logo twice in the flat bar.** The system menu keeps its glyph beside the launcher button
(`native/727/finding-layan-launcher-plus-app-menu-two-logos.png`, top = both on, bottom = `app_menu` only). Layan upstream has one Kickoff button
followed by the application menu, so the package keeps `app_menu` off by default (the user can turn it on, and with a real exporting window it
shows that window's own titles, see the native run). A renderer rule (the system menu does not repeat the logo when `launcher` is shown) would let
Layan enable `app_menu` by default.
