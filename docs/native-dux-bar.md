# Native Dux bar: stage 1 contract

The accepted Settings code is untouched. Actual upstream dark/light 100/200%
captures were viewed from `/tmp/decebal-dux-replica-specialist/upstream-capture`. This stage adds native bar capabilities;
it does not assert that the complete Dux shell has been replicated.

Reference: `duxlabs-dev/dux-shell` commit
`87312075dd414a6f60691464bcd6c6a9bb242a7e`, inspected read-only in the shared
reference checkout. Root MIT attribution and the original pinned reference are
recorded in `dux-adaptation-handoff.md`. No QML runtime or screenshot is used to
paint the native surface.

## Source measurements and native contract

| Source | Measured role | Native implementation |
| --- | --- | --- |
| `Common/Theme.qml:barThickness`, `barWidgetThickness`, `barTextSize`, `barIconSize` | Selected default band 38px, face 28px, text 12px, icons 14px | `[bar] style="dux"`, height 38; compact height 26 remains an explicit package option. At source fontScale 2, text grows to 24px while band, face, padding and icons remain fixed; widths grow from real font metrics. Actual ink bounds center text. |
| `Modules/SurfaceWidgets/BarPill.qml:horizontalPadding` | `snap(8 * face / 30)` at DPR 1 | 7px at each end of a 28px face. At compact 16px face, 4px. Source text enlargement does not enlarge this padding. |
| `Modules/DBar/BarSection.qml:widgetSpacing`, `BarMetrics.qml` | Ordinary pills: 4px. Joined segments: 2px. | Individual widget faces use ordinary 4px spacing; the three sections are placement groups, not three giant faces. This stage does not expose a falsely equivalent joined-segment mode. |
| `Modules/DBar/BarPillSurface.qml`, `BarMetrics.qml:pillRadius` | Expressive pill curves, no outline by default | Package radius 14, bounded by half the face thickness; no outline painted. Default full background band: x=4,y=4,width=output−8,height=38; first widget face x=8,y=9,height=28. Individual faces use the bar surface token. Band radius is source windowRadius L16, independently configured by band_radius; pill radius remains 14. Band reserves 46px including outer margins. Explicit background="islands" removes the band. |
| `Modules/DBar/Widgets/WorkspaceSwitcher.qml`, `BarMetrics.qml:indicatorRatios` | Default pills: inactive along 0.7 × face; active 1.05 × face; cross 0.5 × face | Native variable-width workspace pills using actual active/occupied WM tags; pointer cells use those same extents. |
| `Common/Theme.qml:expressiveEffects`, `stateLayerHover` | 200ms cubic-bezier(.34,.8,.34,1), surface-text overlay alpha .08 | Native cached hover transition, at most 16ms animation wake; reduced motion settles immediately without animation ticks. |
| `DCommon/Widgets/DIcon.qml` | Material Symbols Rounded, FILL 0, weight 400, dark GRAD −25 | Root's native cached variable-outline renderer; font assets are prepared outside paint. No SF-style fallback in Dux. |
| `Modules/DBar/Widgets/CpuMonitor.qml`, `RamMonitor.qml` | `memory` / `developer_board`, numeric percent | Cached native sampler on a worker; missing CPU/RAM widgets are hidden, measured zero remains `0%`. No sampler unless the configured bar requests these items. |
| `Modules/DBar/Widgets/Media.qml` | Native MPRIS transport/title | Existing typed MPRIS worker supplies the cached active player and play/pause/previous/next. Missing MPRIS hides music; its controls/title share one face. No metadata, image or bus query in drawing. |

`[bar] left`, `center`, `right` accept the canonical IDs exposed by
`config::DUX_BAR_ITEMS`. They are declarative names, never commands. Explicit
empty slots stay empty; duplicated and unknown runtime entries are discarded.
The strict package validator checks Dux entries. Legacy styles keep their
previous item semantics and geometry.

Defaults: launcher/workspaces/focused window on the left; music/clock/weather in
the center; SNI/clipboard/CPU/RAM/notifications/battery/control center on the
right. The default centre uses upstream index centering: the middle clock stays on the output centre even when music is absent. Other custom layouts currently use the clock as the anchor when present, otherwise geometric group centering. Layout,
painting, hit testing and glass use the same geometry. Default band receives
input across the band, including inter-widget space. Only explicit islands mode
leaves those gaps transparent and click-through. The band uses background
`#141218` and faces use `#211f24` in the stock Purple dark package; no outlines.
SNI icons share one widget face, rather than each icon becoming a separate pill.
Missing weather follows the actual source clear-day icon plus `--°C`.

Clock opens the anchored native Dashboard; launcher opens the native app grid,
not Spotlight search. Notifications open native history; clipboard opens the
native picker. Existing sound wheel/middle-click and SNI actions use the shared
handlers. While the Dux control panel owns keyboard focus, Up/Down select a
control, Enter activates, and Escape closes. No shell interpolation is added.

## Verification entry points

The isolated renderer test requires private config with the real candidate
package and its native font assets:

```
DUX_BAR_EVIDENCE_DIR=/tmp/dux-bar-raster \
  cargo test --lib dux_bar_native_render_evidence -- --ignored --test-threads=1
```

It emits 24 PNGs in dark/light modes plus `bar-layouts.json`: 100/200% interface text, 38/26px bands,
and 720/1280/1920px output widths. Assertions cover actual group extents,
non-overlap, struts, input exclusion of outer margins, and transparent canvas.
Missing media/resource snapshots hide their widgets; missing weather shows the source placeholder. Metadata records missing provider snapshots as null.

`tests/e2e/dux-bar-native.py BIN THEMES_DIR THEME_ID EVIDENCE_DIR` launches only a
private Xvfb, complete private HOME/XDG/runtime paths and a nonexistent private
bus. It intercepts native launcher/history app opens in private sibling fixtures,
blocks external system/weather probes, verifies actual 100/200% window pixels,
input actions and default-band versus explicit-islands gap pixels, then cleans up every child. It never uses `:0` or
`:430`. It does not install, restart or change an existing desktop.

## Differences still outstanding

- Stock full band plus individual ordinary widget faces are implemented.
  Per-item joined segments, asymmetric joined radii and press shape morphs are not yet exposed.
- Crowded outputs remove wide optional items before allowing overlap; Dux's
  per-section overflow popover is not implemented yet. Essential right-side
  controls remain visible until an output is physically too narrow for them.
- Media cover thumbnails, marquee, waveform and lyric presentation are not yet
  implemented in the bar. Transport, title and player state are real.
- Workspace width changes snap to their new extent. The hover effect matches
  the source timing/curve; source spring and expressive width/radius morphs
  remain outstanding.
- The battery icon/percentage is real state; the source's alternate battery
  pill, charging pulse and time/power variants are not implemented here.
- The native bar's popups retain the existing sound/network/calendar component
  behavior except where the Dux control panel and dashboard explicitly opt in.
- This is X11 native QA. Wayland global popup coordinates, full keyboard entry
  into the closed bar, screen-reader exposure and hardware mixed-DPI acceptance
  are not claimed.

## Reviewed stage-one evidence

Exact stage-one sources compile independently of the pending dashboard/Control
Center implementation. Existing menubar regression suite: 171 passed, zero
failed, two ignored. The actual-package font renderer gate passed 24 dark/light
PNGs covering 100/200% text, 38/26px bands and 720/1280/1920 widths. Evidence:
`/tmp/decebal-dux-bar-stock-raster` (includes `bar-layouts.json`).

Private Xvfb native input and pixel QA passed stock band and explicit islands;
launcher and notification actions are intercepted by private sibling fixtures.
Four additional default-layout captures use the matched 1920×1080 viewport:
`/tmp/decebal-dux-bar-stock-native/native-{dark,light}-1920x1080-{100,200}.png`.
`native-checks.json` identifies the fixture and available native sampler data.
The reference side is actual read-only QML on private headless Sway at the same
viewport, with no DMS backend; its unavailable CPU/media/weather states differ
from native host metrics. These are comparable native surfaces, not a claim of
identical provider values or pixel identity. Reference capture path:
`/tmp/decebal-dux-replica-specialist/upstream-capture/`.

Source ControlCenterButton is itself a configurable composite indicator group.
This stage uses the real audio state icon for its default trigger; additional
network/microphone/brightness/battery grouping options remain section-two work.
The generic existing Control Center/dashboard bodies remain until that next
commit. Their future selectors are not an acceptance claim for those bodies.
