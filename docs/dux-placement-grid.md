# Dux placement and dashboard grid review repair

Pinned source: dux-shell `87312075dd414a6f60691464bcd6c6a9bb242a7e`,
`quickshell/Modules/DDash/DashMetrics.qml` and `Overview/DashCardGrid.qml`.
Comfortable density uses six preferred 128px columns, 8px gaps, 96px rows and 12px
outer padding: `6*128 + 5*8 + 2*12 = 832` preferred width. The native README
composition now derives every card span from these equations rather than
12px gaps and unrelated fixed card heights.

At 832px/100%, relative to the retained native navigation chrome's grid origin
`y=92`, the selected profile is:

| Card | Column/row origin | Column/row span | Actual rectangle x/y/w/h |
|---|---|---|---|
| Clock |0/0|1/2|12/92/128/200|
| Weather |1/0|2/1|148/92/264/96|
| User |3/0|3/1|420/92/400/96|
| Resources |0/2|1/3|12/300/128/304|
| Calendar |1/1|4/4|148/196/536/408|
| Media |5/1|1/4|692/196/128/408|

The preferred 616px height contains the five grid rows, navigation and outer
padding. The header origin and historical README card assignment are the native
profile contract, not a claim of matched whole-current-default DDash pixels.
Narrow outputs/enlarged text retain the row units, reflow to two columns and
scroll; fonts are not reduced or raster-scaled to squeeze them in. One-row user
cards use a 48px avatar instead of the tall hero avatar, keeping labels in bounds.

`theme::bar_bottom_for(theme, visible)` is a pure logical top-edge extent shared
with the native bar. Dux 38+2*4=46, classic 26, top/split rail 40, side/bottom rail 0,
and disabled 0. `theme::bar_bottom()` resolves current gate settings only for
popup placement, not painting. Banner margins remain 8px, popup margins 4px;
existing pill placement gaps remain unchanged. The bar's X surface includes
transparent popup room and is **not** the edge reservation height.

The private native regression validates actual band pixels (`y=4..42`) and popup
X geometry. Old optimized binaries reproduce banner Y=34/process-popup Y=30 below a
46px reservation; repaired debug binaries produce 54/50. It uses performance tab
to avoid capturing process arguments, private HOME/XDG/buses, guarded owned Xvfb,
mutator stubs, suppressed notification audio and no live inputs. Evidence:
`/tmp/dux-placement-before-final` and `/tmp/dux-placement-after` (exact hashes,
JSON and viewed PNGs). These debug placement artifacts precede the final derived
Dash preferred-height constant; no Dash binary is used in the placement gate.

```
BIN_DIR=/absolute/bin python3 tests/e2e/dux-placement-native.py /tmp/new-dir
# Old versions: EXPECT_OVERLAP=1 records only the expected before-fix overlap.
```

Keyboard fixes: Space activates only the focused target; no focus/slider focus
never issues PlayPause or commits a zero slider value. Media Up/Down scroll unless
a slider is focused; focused volume/seek arrows adjust that slider. Page changes
clear obsolete card focus. `decdash performance` keeps the existing actual shared
Performance page; it no longer silently substitutes the Dux Weather page.

Unit coverage: exact source equations/rectangles, non-overlap and bounds at
normal/narrow 100/200%, focused media command routing, page focus reset, and pure
classic/Dux/rail/disabled top extents. Parent must rerun final optimized native
and visual gates after integration. This does not claim hardware mixed-DPI,
complete source motion/artwork, or repair of the separate legacy process-monitor
Performance content layout.


## Separate process-monitor Performance follow-up

The placement screenshot also exposes an existing **procmon** content defect on
this 24-CPU host; it is not the dashboard's separate shared Performance page.
`procmon/ui.rs::perf_cpu` reserves 92px for stats, but seven stat items in three
columns need three 46px rows (138px). Its minimum 34px core cells can overrun a
small viewport. `Monitor::scroll` currently refuses Performance scrolling.
At 200% font scale, header/tab/sidebar and graph/stat pitches are still fixed.

A proper next batch needs scale-aware header wrapping, sidebar selection/hit
coordinates with its own reachable scrolling, and a main Performance viewport
with accurate content extents across all existing metrics painters. Merely
compressing core cells, dropping rows or shrinking text would not fix this.
No half-finished scroll or Performance-body change is included in this repair;
only its default popup placement changed. Parent explicitly deferred the larger
bounded repair to a dedicated batch.
