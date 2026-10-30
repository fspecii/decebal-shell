# Native variable Dux clock on production baseline1816055

This is a bounded clock-only implementation on
`1816055c44280aa6f01a56823eedc3f7c4718066`. It supersedes the old
`a8068eccdefe28829e0ffb1aa193d3fd084722da` handoff; integrate only the new commit.
It preserves production's source grid repair3ebc727 and media/focus behavior55c90da.
No other card layout, data provider, Settings, WM/audio lifecycle, theme, palette,
asset, manifest, global version or PKGBUILD is changed.

## Activation and original font

The deployed Dux1.2 four-font profile retains its production0.2 clock rendering
and starts no clock preparation worker. A registered Google Sans Flex variable
face, or its canonical `GoogleSansFlex-Variable.ttf` package reference, selects
the new capability. An unlisted font merely present in the package does not
activate it. Registration/asset/version failures do not trigger direct file I/O
or substitution inside the variable renderer: it prepares an unavailable state.
Unresolvable references are handled by the existing Source validation; the clock
does not bypass that loader. Parent must gate the capable shell version before
integrating the specialist's separate fifth-entry activationf9f2169.

The specialist alone owns the original licensed asset, asset commit66e1822:
`fonts/GoogleSansFlex-Variable.ttf`, SHA256
`c6d53424121196b81de816b8daccf200e285dd506df43766db3d7e8cdf06ee30`.
It is unchanged GoogleSansFlex.ttf from `dux-qml-common`
`43e05926fcdacc53675623169bcef686faa95301`, under SIL Open Font License1.1.
The source equations are adapted from MIT-licensed
[DClockFace.qml](https://github.com/duxlabs-dev/dux-qml-common/blob/43e05926fcdacc53675623169bcef686faa95301/DCommon/Widgets/DClockFace.qml),
pinned by shell `87312075dd414a6f60691464bcd6c6a9bb242a7e`.
No font conversion, installation, QML runtime dependency or reference screenshot
is part of the native UI. Existing static UI400/500/700 remain selected at their
original weights; actual Source/package tests assert their file selection.

## Source geometry and glyphs

Production's grid remains six128px columns,8px gaps and96px rows. At832px
Dashboard width the clock cell stays128×200 at(12,92). Source ClockCard padding16
leaves96×168 for DClockFace. The clock uses this actual grid geometry rather than
the previous134×216 approximation.

Digits use the original font's actual outlines at `wght750`, `ROND100`, and
requested `opsz=height/2` for vertical clocks or `height` for inline clocks.
The font clamps coordinates to its own declared ranges. Source widest-digit
advance,100px reference ink bounds, support reservation,6% inter-row gap,
integer raster pixel-size rounding and real-valued block centering drive fitting.
The existing native12/24-hour setting remains in use.

`ttf-parser` stays at locked0.21.1, with its existing optional `gvar-alloc`
feature enabled. This original font has glyphs with over200 variation tuples,
exceeding the default32-tuple stack limit. No crate version, lockfile or package
installation is changed. Tests require actual A/digit outlines and verify
weight, roundness and optical-size deformation across applicable clock glyphs.

## Shared snapshots and bounded worker

Clock acquisition uses root17b987c's pure
`fontdb::package_font_bytes(path,version)` lookup. The original bytes are the
same immutable `Arc<[u8]>` retained by Source's bounded versioned cache. There
is no clock-owned font Vec, filesystem fallback, reread or duplicate4.15MiB copy.
Unknown versions become unavailable instead of reopening the path. Tests prove
Arc identity across clock instances and Source, plus version-mismatch rejection.

The variable clock's event thread only queues input, polls immutable results and
paints coverage. Font validation/parsing, glyph rasterization, date fitting and
existing UI-font preparation happen on one named worker per mapped Dashboard.
Before a waker is installed, only the latest geometry is retained. The queue has
one active job, one replaceable pending job and one completed result. Resize
storms coalesce; stale revisions and theme-generation changes cannot publish
old data. While preparation is pending, three native vector dots are shown.

Minute-only changes share the existing coverage Arc. Palette-only and reduced-
motion generations retain it. Font paths/versions/roles or geometry/date changes
invalidate prepared state. Closing clears pending/output slots and the waker;
the worker exits after its current bounded CPU job without blocking the UI on
join. Tests join it separately and assert no late wakes. No polling timer, image
read/decoder, network request or animation is added to the render path.

Each prepared frame is capped at4MiB coverage; only a small bounded number of
old/current frames can coexist while a result changes. Current captures use
well below this limit. Original font snapshots obey the shared16MiB-per-file,
128MiB/32-entry cache bounds rather than a second clock cache. Invalid geometry
produces no clock frame; input is bounded to4096px per axis and short time/date
strings. These are memory bounds, not a hard CPU deadline for arbitrary fonts.

## Genuine source comparison at current geometry

Actual unmodified DClockFace was rendered on private headless Sway/pixman,
quickshell0.3.2,1920×1080 DPR1. Source fontScale1/2 is text scale, not DPR2.
Four bounded startups captured five real content sizes in both modes/scales;
no reference checkout was edited. Logs, raw item crops, wrapper, runner and
source-metrics.json remain in `/tmp/decebal-dux-clock-current-source`.

| Content | Native viewport/profile producing it | Requested opsz |
| --- | --- | ---: |
| 96×168 | 1920-wide,100% | 84 |
| 192×336 | 1920-wide,200% | 168 |
| 368×336 | 960-wide reflow,200% | 168 |
| 256×168 | 640-wide reflow,100% | 84 |
| 208×336 | 640-wide reflow,200% | 168 |

All20 primary digit ink bounding boxes are within1px of the actual source
(the portable gate allows2px). This is not pixel identity: Qt and tiny-skia
antialiasing differ. At requestedopsz84, Qt reports76px reference ink height
versus native75px. The resulting digit-size difference reaches1.111px and block
centering difference0.795px. The renderer retains native ink fitting rather
than hardcoding a correction for selected sizes. Atopsz168, reference ink ratio
is.75 in both engines and digit-size differences are at most.009px. Raw metrics
and every difference are recorded in source-comparison.json.

Dates use the effective static500 UI chain. They fit to minimum12×textScale,
then elide rather than overflow. Small source HorizontalFit/date output visibly
cramps/clips; native containment is deliberate. The date remains the existing
native English formatter. The digit font is the original Google Sans Flex
replica profile; custom DClockFace font/weight editors, analog mode, seconds,
per-row alternate colors and digit transitions are outside this bounded work.
Matched source cases are vertical; inline sizes have native-only regression
coverage. Native host QA is X11/Xvfb; a native Wayland host was not exercised.

## Reproduction and provenance

```sh
DUX_CLOCK_VARIABLE_FONT=/readonly/original/GoogleSansFlex-Variable.ttf \
python3 tests/e2e/dux-clock-native.py BIN_DIR CURRENT_FOUR_FONT_PACKAGE TEST_BIN EVIDENCE
python3 tests/e2e/dux-clock-compare.py EVIDENCE/painter SOURCE_CLOCK_DIR EVIDENCE/source-comparison.json
```

The harness copies the unchanged baseline Dux1.2 package and original asset
privately, verifies the source font SHA and adds the fifth entry only in that
fixture. It preserves the package's version/minimum; no capable public version
is advertised by this private test. HOME/all XDG paths/both buses/displays are
isolated; weather networking is blocked. Only owned Xvfb processes are closed;
no live:0, nested:430, installed binary, config, input or audio is touched.

The core gate renders28 variable clocks plus4 missing states and repeats them
in a separate process with reduced motion enabled, requiring identical PNGs.
It also tests shared snapshots, background preparation, latest-request behavior,
coverage reuse, version failures and bounded shutdown. Twelve actual variable-
clock Dashboard windows cover1920×1080,960×720 and640×420, dark/light,100/200%,
including keyboard scrolling and one named clock worker. An additional four-font
host verifies that the unregistered asset does not change the deployed clock or
create a worker. Source/native comparison covers20 genuine fixed-size cases.

Current evidence is `/tmp/decebal-dux-clock-current-native`; provenance.json
records the baseline, package/asset hashes, actual binary hashes, compiler,
build flags, capture setup and isolation. The isolated full library passed
1680 tests,0 failed,17 ignored. Grid/media regressions passed, and strict
all-target X11 Clippy plus formatting pass. Isolated core preparation across
measured geometries took11–40ms; production performs that work on the worker.
Cached paint including whole-target clear
was0.10–3.0ms locally, with at most133702 coverage bytes in these cases. These
are debug-profile observations, not portable latency guarantees. No screenshot
is used as application pixels.
