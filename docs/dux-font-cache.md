# Private package font snapshots and Material glyph evidence

The loader keeps a process-local bounded cache of immutable package-font snapshots.
A key is the canonical package font path plus its opaque metadata version: size,
mtime and (on Unix) device/inode/ctime. Replacing a file while retaining size and
mtime therefore invalidates the entry. Unchanged `Source::theme_for` generations
reuse both parsed family metadata and the same `Arc<[u8]>` bytes. Font metadata
remains process-private; installed fontconfig databases are not changed.

`SymbolCache::prepare_material` performs a cache lookup only. It neither opens
nor copies the Material file, and multiple bar/panel icon caches share the same
snapshot. Its raster entries survive preparation when face, bytes and dark/light
mode are unchanged. Dark/light changes clear raster entries because the Material
GRAD axis differs. `get_material` performs no file I/O. Its raster map is limited to 256 entries /
4 MiB, with supported icon squares up to 256px. Palette/hover colour churn
therefore cannot grow the cache indefinitely or force another font-file read.

Each input remains limited to 16 MiB, eight package files are considered, and
retained cache entries are limited to 32 / 128 MiB. Active icon caches can keep
an older snapshot alive until their generation changes; that is necessary for
safe replacement, rather than reading mutable file memory during a paint.
Oversize, unreadable, changing or invalid fonts generate a rejection diagnostic,
negative-cached per file version to avoid a log/read flood. Invalid/missing or
escaping package references have a separate bounded, deduplicated diagnostic
path; oversize inputs are no longer silently filtered before the cache. A missing snapshot
returns `None` rather than falling back to filesystem work in drawing. The
Control Center's semantic fallback handles unavailable package symbols separately.
Initial reads and filesystem metadata probes retain their normal OS I/O boundary;
this is not a claim that cold theme loading has no filesystem work. Version
checks apply when the existing theme-loading/generation path runs; this change
does not add a new watcher for standalone font-file edits without a theme refresh.

## Source glyph corrections

The actual pinned Dux `WidgetModel.qml` uses `contrast` for Appearance,
`do_not_disturb_on` for DND and `local_cafe` for Keep Awake. The Weather service
uses `clear_day`, `clear_night` and `cloudy_snowing`. Those aliases now resolve to
the shipped font's actual glyphs, rather than two identical dark-mode crescents,
a coffee mug variant, sunny, or the legacy weather-snowy variant. `screen_record`
was already correct.

`tests/e2e/dux-material-cmap.py` verifies **all 60 native aliases** against both
the packaged font's cmap and its GSUB ligature names. This checks glyph semantics,
not merely that a PUA slot exists. The font SHA-256 is
`7f3a16d9aa1b6f367eadc1614ad27308ae2c0e91e617e2069f122afd9f0c3c58`.
It uses FontTools as a QA dependency, not a runtime dependency. The already
verified extracted wheel can be used without installation:

```sh
FONTTOOLS_PATH=/tmp/decebal-dux-replica-specialist/font-build/python \
  python3 tests/e2e/dux-material-cmap.py /tmp/material-cmap.json
```

Upstream DCommon `DIcon.qml` centers `StyledText` by its font metrics/advance,
not its ink bounding rectangle. Native symbols now retain their side bearings
and use a centered font ascent/descent box. The pinned font has UPEM960,
ascender1056 and descender-96, producing a baseline at `px` within a `px` square.
The native regression checks exact alpha placement for asymmetric play/forward
symbols. This is a metric-placement check, not a claim of identical Qt/FreeType
and tiny-skia antialiasing or complete variable-icon visual parity.

## Validation boundary

- Local-cache tests: 20 unchanged loads perform one read/parse and share Arcs;
  same-size/mtime atomic replacement produces a new snapshot while held old bytes
  remain unchanged. Oversize inputs reject before byte allocation; negative
  caching and retained-entry bounds are checked.
- Unit cmap check covers every mapping arm in the implementation.
- The isolated `package_material_icons_are_actual_cached_outlines` test draws all
  60 aliases at 14/28px (120 PNGs), verifies two SymbolCaches share bytes, repeated
  preparation keeps raster entries, checks em-box pixel placement, and bounds 300 unique colour requests. Run alone
  with a private DECEBAL_CONFIG and the explicit DUX_MATERIAL_FONT asset.
- Parent must rerun relevant final native package/window gates against its final
  optimized build. This change does not install fonts or modify the live desktop.

## Combined source acceptance at `ac7a4b7`

A separate read-only check reran the actual cmap/GSUB verifier against the
combined root: all 60 aliases passed with the same pinned Material font hash.
All four candidate files were inspected with FontTools: Google Sans Flex static
weights 400/500/700 plus Material Symbols Rounded variable weight 400. Every file
is below the 16 MiB per-input limit; no fifth/original variable clock font was
included. Evidence is in `/tmp/dux-material-cmap-combined-ac7a4b7.json` and
`/tmp/dux-font-combined-acceptance-ac7a4b7.json`.

The source grid constants and formulas were compared with pinned upstream
DDash `DashMetrics.qml`, `CcMetrics.qml` and the comfortable density tokens:
six 128px columns, five 8px gaps, two 12px outer pads produce 832px; a four-column
span is 536px and a four-row span is 408px. The native wide grid now uses those
span equations. Narrow/enlarged-text reflow and its 92px navigation/content inset
are explicit native adaptations, not a claim of a matched section-two upstream
screenshot. These checks inspect source/metadata; they do not replace the
parent's compiled grid tests or final optimized native window tests.

The reviewed integration still routes Source's strong file versions into the
bounded metadata/byte cache and uses lookup-only Material preparation. Source
font/cache/symbol/grid files were unchanged from `ac7a4b7` at this check. No
additional production regression was identified. This check did not run Cargo,
change production, touch a display/audio device or exercise the live desktop.
The OS I/O/cache-retention boundaries above, glyph rasterizer antialiasing,
standalone-font-edit refresh limitation, variable clock typography and complete
replica/native-window/hardware acceptance remain outside this source check.
