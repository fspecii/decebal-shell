# The island taskbar as one content-sized, centred cluster

UKUI's second panel mode (`panelNext`, "three islands") is **one** run of three rounded islands, sized by what they hold and centred on the bottom edge, not three groups spread
to the start, middle and end. The earlier packages approximated it with the stock Dux placement; this increment adds the smallest generic support and uses it.

## Pinned source (openkylin/ukui-panel `openkylin/huanghe` @ `3429618`, GPL-3.0+; measured, no code copied)

| fact | where | value |
|---|---|---|
| the islands are siblings of one `GridLayout`; spacing between them | `panel/qml/org.ukui.panelNext/ui/MainContainer.qml` `columnSpacing` | `8 * ratio` (ratio 1 at the default 56 px strip) |
| the container's width | same file, `Binding on width` | `data + settings + app + columnSpacing * (islands - 1)`, **unless** an island `fillWidth` (then the whole width minus the 8 px margins) |
| it is centred | same file, `anchors.horizontalCenter: parent.horizontalCenter` | content-sized and centred |
| `fillWidth` on a horizontal panel | every plugin (`widgets/*/main.qml`, `ukui-task-manager/plugin/qml/main.qml`) | `Layout.fillWidth: !isHorizontal` → **false**: the cluster is content-sized |
| island height | `MainContainer.qml` (`mainLayout.anchors.top/bottomMargin 4*ratio`, strip 56) | 56 − 2 × 4 = **48** |
| island corner radius | `ApplicationsIsland.qml`, `SettingsIsland.qml` | `GlobalTheme.kRadiusWindow * ratio` = **12** (theme token), 1 px border `buttonTextActive` at 25 % |
| padding inside the application island | `ApplicationsIsland.qml` flickView margins | **12 * ratio** each side; item spacing `4 * ratio` |
| distance from the screen edge | `panel/src/view/panel-next.cpp` margins `L 8 / T 0 / R 8 / B 8`, island inset 4 | **8 + 4 = 12** |
| default contents | `panel-next.cpp` | data island: start, idm, task view; app island: task manager, search; settings island: system tray, calendar |

## What the generic support is (no theme id anywhere)

Three optional `[bar]` keys, for a horizontal Dux bar (`bar.style = "dux"`, top or bottom):

* `cluster = true`: lay the `left` / `center` / `right` groups out as **one run, in that order, centred on the bar and sized by its content**; each group draws **one face** (rounded, with the
  theme's `outline` stroke). Empty groups have no face and no gap. Off by default: the stock placement and per-item faces are byte-for-byte unchanged.
* `group_gap` (0..48): space between faces (UKUI: 8).
* `group_padding` (0..32): space between a face's edge and its first/last item (UKUI application island: 12).
* The three group lists now accept **applet registry keys** (`tasks`, `search`, `volume`…), not only the first batch of Dux ids; the manifest validator and `BarLayout::from_json` accept the
  same set. Existing manifests keep working.
* Geometry follows from existing Dux numbers: a face is `bar.height − 10` high with its top at `bar.margin + 5`, so `height = 58`, `margin = 7` give **48 px faces standing 12 px above the
  edge**; `radius = 12`; `spacing = 4` is the item gap.
* If the run does not fit (short output) the usual optional content goes first (windows, then optional applets), then whole groups, the middle one first; faces never overlap and the overflow
  cell lists what gave way. Side edges keep the turned band (UKUI's island panel exists only on the bottom edge).

Code: `src/config.rs` (`BarLayout`), `src/menubar/segmented.rs` (`place`, `Layout::faces`, paint), `src/theme/engine/{schema,manifest,apply}.rs`, `src/panel/mod.rs` (the keys need the 0.3.0
framework). Tests: `src/menubar/segmented/cluster_tests.rs` (numbers against the source above, content sizing, centring, empty groups, short outputs from 1920 down to 160 px, the stock layout
unchanged, key clamping, manifest checks) and `tests/e2e/openkylin-native.py` (measured on real captures).

## Not modelled (stated, not faked)

* the **drag handles** (`IslandDragControlRod`, width `height * 16/56`) and drag-and-drop re-ordering of islands; data island padding is `rod 13.7 + 16` and settings island `8 + rod 13.7`
  in the source, all three faces here use the application island's 12 px;
* island **minimum / maximum widths** (data 64..208, app ≥ 64, settings ≥ 192, `* ratio`) and the application island's scrolling / shrinking when the cluster outgrows the screen;
* the panel-size ratio (0.85..1.43) and size policies; the top-bar mode; auto-hide;
* task view and IDM (no applets), the start menu (`ukui-menu` is empty on the mirror) and the sidebar/Control Center look: **no replica of the UKUI launcher or Control Center is claimed**.
