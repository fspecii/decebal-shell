# Native Dux README dashboard and Control Center capability

This stage implements opt-in native surface composition. The dashboard is the
explicit `dashboard.profile="readme"` historical configured profile, not the
current upstream default. The accepted Settings
implementation is preserved. It does not run QML or use reference screenshots as
application pixels. Theme packages remain owned by the existing specialist.

## Source contract

Current shell source inspected at
[`87312075dd414a6f60691464bcd6c6a9bb242a7e`](https://github.com/duxlabs-dev/dux-shell/tree/87312075dd414a6f60691464bcd6c6a9bb242a7e).
The previously inspected shell pin is `46a12d915203a6bf296f26cab251c55efa6499ab`;
the first-party plugin pin remains
`a55bb40da2f70640600a520ea05eac9c3540f1af`. Root MIT notices and separate asset
license boundaries are recorded in `dux-adaptation-handoff.md`. No upstream
wallpaper, avatar or album artwork is copied by this capability.

The [README dashboard image](https://github.com/user-attachments/assets/a937cf35-a43b-4558-8c39-5694ff5fcac4)
is a concrete composition reference: icon-and-label navigation, stacked HH/MM,
weather and user at the top, calendar in the middle, narrow vertical resource
meters left and portrait artwork/transport right. It is an older configured
layout, not proof that every current installation has those exact cards.

| Source | Measured contract | Native behavior |
| --- | --- | --- |
| `Common/Theme.qml` | comfortable density: touch target 48; spacing S/M/L = 8/12/16; font sizes 12/14/16/20/28/36/57 | semantic text roles and actual font scaling; package-selected native fonts |
| `DDash/DashMetrics.qml` | six preferred 128px columns; 8px gap; 12px outer content padding: **832px** width; row unit 96 | opt-in `dashboard.style="dux"`, 832px preferred width; actual physical viewport bounded by output |
| `DDash/Overview/ClockCard.qml` and DClockFace | separate hour/minute strings, display role, primary ink | genuine vertically stacked native display glyphs; date below |
| `DDash/Overview/CalendarOverviewCard.qml` | month caption, chevrons, weekday/date grid, current-date treatment | real calendar grid and previous/next-month hit regions |
| `DDash/Overview/SystemMonitorCard.qml` | narrow grouped metrics | cached CPU/temperature/RAM vertical tracks; missing values shown as `--` |
| `DDash/Overview/MediaOverviewCard.qml` | artwork, title/artist, seek track, previous/play/next | worker-cached real artwork and typed MPRIS actions; no guessed artwork |
| `DDash/DashRegistry.qml` | Overview/Media/Weather content, Settings is an action | native content pages and entry to existing Settings |
| `ControlCenter/CcMetrics.qml` | tile cell = 48 + 2×8 = 64; eight columns; 8px gaps; 16px sheet padding: **600px** width | opt-in `control_center.style="dux"`, two expanded tile columns with leading tonal icon boxes |
| `ControlCenter/Widgets/CcTileContent.qml` | primary label, supporting state, rounded leading icon box | real network/Bluetooth/DND/night/appearance/awake states with separate primary/supporting text |
| `ControlCenter/Widgets/CcSliderRow.qml` | slim track, primary fill, vertical handle | shared native slider hit rectangles and tonal strip painting |

Current upstream SettingsSpec.qml903–969 defaults instead enable Overview,
Media, Wallpapers, Weather and Wellbeing; Notifications and Settings are
disabled. Default cards are clock2×1, weather1×1, notifications3×5, calendar3×3
and media3×1. The README profile deliberately preserves the user-requested
historical four-item navigation, user band and vertical system meters.

The 832/600 values are current comfortable-density equations. Earlier 840/560
estimates came from assumed padding and screenshot dimensions and are not used
as current-source measurements.

## Interaction, scaling and rendering

Dashboard layout and hit regions use the same native geometry. Its wide view
keeps the reference composition; narrow outputs or 200% text reflow the cards
into two columns with a full-width calendar and user band. Content scrolls inside
the viewport while the navigation stays fixed. Text remains actual enlarged
font output, never a scaled screenshot. Keyboard navigation includes page
selection, Tab/Enter focus, Home and Page Up/Down; source media controls use
existing worker-owned MPRIS capabilities. Missing players have no transport
targets. Seeking requires actual seek capability and known track duration.

The dashboard and Control Center use package-supplied Material Symbols through
the native outline cache. Font discovery occurs at startup/theme reload, outside
painting. Dux painting does not silently substitute SF-style artwork when the
Material font is absent. Packages must supply licensed native font assets.

No file/network reads, image decoding or probes are added to the paint loop.
Existing worker snapshots supply weather, user information, uptime, resources
and artwork. No arbitrary script or command action is introduced. Ordinary
themes retain the existing dashboard dimensions, layout and Control Center look.
New surface transitions are currently immediate, including under reduced motion.

## Reproducible native evidence

The isolated renderer gate is intentionally ignored by the ordinary parallel
test suite because font scale is process-global:

```sh
DUX_SURFACE_EVIDENCE_DIR=/tmp/dux-surfaces \
DECEBAL_CONFIG=/tmp/private-dux-config \
DECEBAL_THEMES=/tmp/private-dux-themes \
cargo test --lib native_dux_surface_evidence -- --ignored --test-threads=1
```

The explicit private fixture must select the actual Dux candidate package and
enable dashboard.style="dux", dashboard.profile="readme" and
control_center.style="dux". The renderer resolves the package via Source and
asserts Google Sans Flex plus prepared Material Symbols metadata, rather than
using Nord geometry or palettes. The final gate passed in **3.54 seconds** and
retained **40 native PNGs** at `/tmp/decebal-dux-surface-evidence-final`: all
100/200% light/dark sample/missing states, three content pages, scrolled views
and full Control Center content. Source navigation/sysmon Material glyphs,
person fallback, actual package palette/font roles and cached hostname are
included. A separate pure geometry test checks containment and non-overlap at
wide/narrow widths and enlarged text.

The same isolated gate checks behavioral slider regressions. With a nonzero
42-second track and 67% volume, Enter on a focused seek/volume slider emits no
MPRIS command and neither resets cached data nor arms a pointer drag. Actual
pointer previews emit no repeated commands; release records a seek to 131.4
seconds and volume to 71% through the production sender's recording sink, with
no worker or bus. Both Control Center entry points additionally use dry-run
setters to prove Enter preserves 67% volume/73% brightness, emits no setter and
does not arm a pointer drag. Pointer dragging and slider wheel actions retain
their existing typed paths.

The real-window gate is `tests/e2e/dux-surfaces-native.py`. It copies the actual
package and binaries privately and overrides only test selectors in that copy.
`DUX_SURFACE_HOST_PART=control` exercises the standalone panel, and `bar`
exercises the panel opened from the actual declarative bar button. Both passed
100/200% light/dark at 960×720, including wheel scrolling, keyboard focus/reveal,
viewport bounds and Settings actions captured by private sibling fixtures.
The final combined evidence is `/tmp/decebal-dux-surfaces-host-reviewed`:
**36 native PNGs, four viewport JSON dumps and native.log**. It contains 16
Dashboard captures, 12 standalone Control Center captures and eight mainbar
Control Center captures. The 200% standalone panel occupied 600×658 at (352,54),
with 820px content accessible by scrolling. Final 200% weather values and missing
states, person fallback, host/card separation and real cached hostname were
visually inspected. Pixel capture reads the actual XCB framebuffer through
Pillow, with painted-frame readiness and explicit scroll-zero checks before
header input. Both session and system buses, HOME/XDG paths and displays are
private; inherited Pulse/Wayland/runtime addresses are removed. A private curl
fixture prevents test weather network requests, and private sibling executables
record Settings opens. All private processes were cleaned up.

The initial Dashboard host probe exposed a shared pre-existing X11 startup
FocusOut close: both ordinary Nord and Dux exited cleanly in under 800ms, with
or without a private WM. Parent fixed it by querying current server focus:
queued normal Pointer focus-loss events are stale while this window still owns
focus; genuine foreign focus still closes it. Fresh Nord and Dux probes stayed
open, and the complete host gate then passed all four modes. No runtime
behavior was bypassed. Central all-library/lint validation and serial integration
remain the parent's responsibility.

## Remaining differences to track explicitly

- Dux's arbitrary card/grid editor and plugin runtime are not imported.
- Calendar event/task creation, external agenda sources and week-number options
  are not yet supplied by this native calendar. Weekdays currently start Monday;
  the screenshot's configured Sunday start still needs a native option.
- Weather consumes available native current readings and daily forecasts; the
  original hourly chart/sky animation and forecast-provider options remain absent.
- Media artwork uses native rounded geometry; the original variable expressive
  shape and all press/transition curves are not yet reproduced.
- Clock variable digit axes and ink-based fitting are supplied by the bounded
  follow-up described in [native-dux-clock.md](native-dux-clock.md). Its original
  variable font requires a fifth package font entry; the deployed four-font
  profile keeps its existing clock until separate activation. Registered
  variable-font snapshot failures are explicit.
  Analog mode, seconds and digit transition animation remain separate capabilities.
- Control Center detail sheets, device selection subpages, microphone controls,
  lock/power header actions and its editable grid need separate typed capabilities.
  The source-correct Material person fallback is present; real Control Center
  avatar pixels still need a separate worker-cached asset path.
- Control Center currently uses the comfortable eight-column 600px width. The
  source's smaller-output column cap/reflow still needs a native width contract.
- Native window QA passed the stated gates; complete original typography,
  expressive shape/animation and all source feature parity are not claimed.
  No live desktop or nested `:430` was used by these tests.
