# Combined Dux review acceptance gates

Independent inspection of ac7a4b7 confirmed the focused regression harnesses and
source contracts below. This inspection is not execution acceptance of a newer
combined binary. Root owns the subsequent complete tests and optimized build;
previous debug evidence must not be substituted for that candidate.

## Exact candidate provenance

Pin the candidate binaries and record their SHA-256 values with the source
revision and build log. The harnesses record the hashes they actually execute.
A timestamp or a matching version string alone does not identify a revision.
In particular, `/tmp/dux-bar-review-frozen-bin/decbar` and
`/tmp/dux-bar-cc-final-lib-tests` predate ac7a4b7.

## Private native gates

Run from the shell repository, using the same pinned candidate directory:

```sh
BIN_DIR=/absolute/candidate /usr/bin/python3 tests/e2e/dux-placement-native.py /tmp/dux-accept-placement
BAR_BINARY=/absolute/candidate/decbar /usr/bin/python3 tests/e2e/dux-title-redraw.py themes/dux-lavender /tmp/dux-accept-title
BAR_BINARY=/absolute/candidate/decbar /usr/bin/python3 tests/e2e/dux-cc-fallback.py /tmp/dux-accept-fallback
```

The placement harness needs `decbar`, `decnotify`, and `decmon`. It measures the
actual painted band, banner and monitor positions and requires banner clearance
of eight pixels and monitor clearance of four. The title harness changes only
`_NET_WM_NAME` on a fake EWMH application, without resource widgets or pointer
input; the bar must repaint within three seconds. The fallback harness selects
classic macOS with the independent Dux Control Center look and no package fonts.
It verifies actual person/settings/network glyph pixels, inert unfocused Return,
and an explicitly focused Settings action routed to a private fixture.

These harnesses create private HOME/XDG directories, owned Xvfb displays and
private or nonexistent buses. They must never be pointed at the live display.

## Focused source and painter gates

Use the exact test executable built from the same source candidate, running each
filter serially:

- `comfortable_source_grid_matches_pinned_dashmetrics_equations`
- `source_grid_and_large_text_reflow_keep_cards_separate_and_in_bounds`
- `slider_targets_cover_the_full_row_and_icons_use_source_names`
- `dux_symbols_have_semantic_fallback_without_package_fonts`
- `control_center_wheel_keeps_highlight_and_repaint_consistent`

The Dash equation gate asserts the 128px columns, eight-pixel gaps, 96px row
unit and exact card coordinates at 832px. The reflow gate checks separate cards
at narrow widths and 100/200% text. Slider tests check the full 48px row,
including its top and bottom hit regions, rather than only the painted track.

Run `native_dux_surface_evidence --ignored --test-threads=1 --nocapture` alone,
with explicit private `DECEBAL_CONFIG`, `DECEBAL_THEMES`, copied Dux package fonts,
private HOME/all XDG/buses, stubbed external commands and
`DUX_SURFACE_EVIDENCE_DIR`. Its forty PNGs cover light/dark, 100/200% text,
available/missing data, Control Center and Dash pages. It also exercises shared
hit geometry, pointer commits and explicit keyboard-focus safety assertions.

Offscreen painter assertions do not prove real X wheel/drag interactions at
every scale, Wayland popup placement, hardware frame rate or accessibility APIs.
A successful native no-font keyboard gate does not establish all those claims.

## Inspection outcome

Harness Python syntax and shared diff whitespace checks passed. A minor wheel
highlight edge was reported to root: scrolling outside the panel cleared state
without requesting repaint. Root subsequently corrected it and added the focused
regression named above. No live input/configuration, installation, restart or
release build was performed by the independent reviewer.

## Real-X full-row slider interactions

```sh
BAR_BINARY=/absolute/candidate/decbar /usr/bin/python3 tests/e2e/dux-cc-sliders-native.py /tmp/dux-accept-sliders
```

This bounded two-case harness uses 100% text on a 960×720 private Xvfb and 200%
text on a deliberately short 960×240 Xvfb. Only the sound/display groups are
selected. Source-derived row coordinates cover the top and bottom of each full
48/68px target, outside the slim painted track. Real X pointer press/release
must set volume and brightness to 25%/75%; wheel input at the same edges must
set 30%/80%. Exact emitted setter arguments, coordinates, viewport scroll,
binary hash and screenshots are saved. The short case scrolls over a label to
reveal the clipped brightness row. Return after pointer/wheel input must remain
inert. This covers the standalone Control Center host; the embedded bar host's
viewport still needs its own interaction gate.

All audio/brightness tools are private fixture scripts. Probes return fixed
50% values and setters only append fixture logs; actual backend setters never
run. The test owns its Xvfb and removes its processes/private configuration.
It does not perform a real-system volume/brightness or hardware animation test.

Harness debugging passed against the explicitly older debug bar SHA
`6d5d059a0a38b72f628ec3c693f45fd7fb913180c170d38dec188881a709411a`:
`/tmp/dux-slider-harness-smoke-final/result.json`. Both cases, eight pointer
edge commits, eight wheel commits and inert stale Return passed. The 200%
short screenshot was visually inspected: the brightness row is fully visible
after scrolling. This smoke result verifies the harness and is **not acceptance
of the subsequent combined optimized candidate**. Re-run with that pinned binary.

The slider harness additionally navigates with real Down/Down keys from no
focus through Settings to Volume (the source target order for the restricted
tile list). It requires the complete target to be revealed and more than thirty
additional blue perimeter pixels away from the painted track, saves
`volume-keyboard-focus.png` at both scales, and asserts navigation and Enter on
the focused slider emit no setters. Harness-only smoke against the older hash
above passed: 388/322 focus-ring pixels at 100/200%, with no baseline ring.

## Optimized candidate acceptance, source 55c90da

Root supplied `/tmp/decebal-review2-release-artifacts/bin` with source/build
provenance. The four harnesses above were executed sequentially against its
exact optimized `decbar` SHA-256
`a3c2ee808e48cbdedc4eef1303d68f56e993f2c59fc524e027ac8f0db2b4ba75`.

- `/tmp/decebal-review2-native/placement/result.json`: pass. Logical bar bottom
  46px, actual painted bottom 42px, banner y54, process monitor y50. Companion
  `decnotify` SHA `7748ff81a8def9818beceb43a5fd3ec92980709089bf08a4b65e8fb91376cfcc`;
  `decmon` SHA `b653462b0fc0a3b7050cab10dc0e88da574bcd8b8886849d23176aa44f1b8a3e`.
- `/tmp/decebal-review2-native/title/result.json`: same-app title-only repaint
  passed in 0.056s.
- `/tmp/decebal-review2-native/fallback-isolated/`: all three no-font semantic
  icon crops passed; unfocused Enter inert and explicitly focused Settings
  reached only the private fixture.
- `/tmp/decebal-review2-native/sliders/result.json`: both scales passed all
  eight pointer commits and eight wheel commits with exact expected setter
  values. Stale Enter remained inert. Keyboard navigation revealed the full
  Volume target, including the 200% short viewport at scroll44; 388/322 blue
  perimeter pixels were measured against zero baseline ring pixels. Navigation
  and focused-slider Enter emitted no setters. Focused 100/200% screenshots were
  visually inspected via panel crops, confirming a clear full-row focus ring.

The first optimized fallback attempt in `/tmp/decebal-review2-native/fallback`
failed its Settings fixture check because the candidate contains a real sibling
`decsettings`; production correctly prefers that sibling over PATH. The harness
now copies the exact-hash bar into its private fixture directory beside the fake
Settings executable, preventing real sibling execution. The isolated rerun
supersedes that harness failure. No production code changed for this correction.

This acceptance covers the named X11 regressions and standalone slider host;
all earlier limits regarding other hosts, Wayland, hardware and accessibility
remain. No live display input, actual setters, installation or restart occurred.

## Standalone header-action investigation

The broader optimized matrix later exposed an unresolved Settings launch exit
race; the preceding bounded fallback pass does not supersede this failure.
`DUX_SURFACE_HOST_PART=control DUX_SURFACE_CASE=100-dark` restricts
`tests/e2e/dux-surfaces-native.py` to that single case. It now saves live X
geometry, pointer position, current panel dump, intended click, observed process
exit timing, fixture entry/completion and private DLOG files before cleanup.
`DUX_SETTINGS_FIXTURE_DELAY=.2` optionally separates fixture entry and completion
for lifecycle diagnosis; normal runs add no sleep.

Targeted evidence `/tmp/decebal-review2-native/header-diagnostic` and
`header-lifecycle` reproduces the failure with stable panel origin352,54,
surface960×638 and content576. The clicked gear location908,106 is correct in
the inspected before-header capture; no stale coordinate or asynchronous resize
is required. The bar exits normally and the Settings fixture never enters.
Source inspection identifies a plausible launch-lifetime defect: the Settings
action closes the standalone overlay while `sys::spawn` merely queues a thread
using the probe runner, whose child has a parent-death kill signal. The thread
or child can disappear with the overlay before application launch. This needs a
production repair and targeted candidate rerun; no production edit was made by
the investigating agent. Deployment should remain gated on that result.

## Launch lifetime repair: exact debug native proof

Root's repaired debug bar was copied to an immutable private directory and
hashed `d22a040353815e4f0c4ec0138e66212a96dcee7afbbe100999f620c828ec10ec`.
The targeted header case and then all four standalone Control Center cases
(100/200%, dark/light) passed with `DUX_SETTINGS_FIXTURE_DELAY=.2`.
`/tmp/decebal-popup-launch-after-matrix/*click-diagnostic.json` records fixture
entry/completion wall-clock timestamps and the already observed normal overlay
exit. Completion occurs 103–199ms after observed exit in the four cases. This
proves the child survives the caller, rather than merely winning a fast fixture
race. Delayed mode now asserts that timestamp ordering.

An optional product launch check is available with
`REAL_SETTINGS_BINARY=/absolute/decsettings` on the fallback harness. It copies
the real Settings binary beside the exact bar in private configuration/buses,
selects Settings by real keyboard navigation, requires the owned Settings
PID/executable and XID to remain alive for two seconds after popup exit, captures
the window, then terminates that owned PID. It does not interact with the live
Settings process. The repaired debug bar plus pinned optimized Settings SHA
`e2cf1aaf39f4ba48913a3b44485c8f368df7477e99a24f7b120746626ef49135`
passed: `/tmp/decebal-popup-launch-real-settings-final/real-settings.json`.
The captured Settings General page was visually inspected.

This resolves the demonstrated caller-lifetime failure for the exact repaired
debug bar. Root still owns the final optimized rebuild and corresponding gates;
the older 55c90da optimized candidate remains superseded for launch acceptance.

## Final optimized candidate acceptance, source 0e2fd63

All requested native gates passed on the first attempt against root's pinned
`/tmp/decebal-review3-release-artifacts/bin`. Exact hashes and gate paths are
recorded in `/tmp/decebal-review3-native/acceptance-manifest.json`:

- `decbar`: `fa1747f7918721e2f96c2a65622acb16dccdd03fed50587fb0a11b5ab0501a7d`
- `decsettings`: `96e25b700af19ab8659244f2b80500afda7ca6c3d85316b7156583d82a9ae607`

`surfaces-control` passed all four standalone cases with delayed Settings
completion 106–107ms after already observed popup exit. `real-settings` proved
the matching real Settings PID/XID remained alive for two seconds after popup
exit, then terminated that owned private process. The real Settings screenshot
was visually inspected. These optimized results supersede the earlier launch
failure and debug-only launch repair evidence for these gates.

`placement` passed bar bottom46/banner54/monitor50. `title` repainted the same-app
title-only change in59ms. `fallback` passed all three no-font icons, inert
unfocused Return and explicitly focused Settings fixture launch. `sliders`
passed eight full-row pointer commits, eight wheel commits and stale-Enter
safety at100/200% with the short viewport. Focused Volume perimeter checks and
full-target reveal passed; both scale screenshots were visually inspected.

These are exact optimized X11 regression results, not claims about hardware
animation, Wayland, AT-SPI or every embedded-host interaction. Root owns the
remaining combined desktop gates and any subsequent deployment. This reviewer
performed no cargo build, production edit, live input, install or restart.


Final optimized Shell source0e2fd63 also passed stock bar10captures, mainbar
ControlCenter32captures, Applications launcher32captures, and the four-case
Dashboard page/scroll/Settings-action matrix16captures. A tests-only `dash`
host filter permits independent Dashboard execution without repeating the two
ControlCenter hosts. Results: `/tmp/decebal-review3-native/full/result.json`
and `/tmp/decebal-review3-native/dashboard`. Production fingerprints remain
unchanged after these tests.
