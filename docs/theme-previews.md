# Automatic native theme previews

The Themes gallery now requests a generated preview automatically when it discovers
a package (including after create/import/edit). Generation is lazy: unopened themes
need no rendering or screenshot maintenance. Existing package screenshots remain
exportable evidence but are not the gallery's source of truth. Palette tiles show
semantic color swatches, not a hard-coded desktop illustration.

`decthemectl theme render ID --mode dark|light --png FILE [--palette ID]` uses the
same path. No theme IDs choose preview layouts. Effective definitions and
inheritance drive the production apply **plan**, which writes only private plan
files and never calls apply/reload. A separate `decthemectl` worker runs with private
HOME/XDG/config/runtime directories and a shared private preview fontconfig cache, no display or session bus and automatic
system theming disabled. It never starts the widget host, WM or desktop event loop.
Theme actions/commands are never executed. Original installed user font directories
can be read through private fontconfig; they are not changed or registered.

## What is rendered

- Actual resolved dark/light tokens, selected independent palette and font chains.
  Package defaults use the same wallpaper extraction path when requested by the
  manifest; current-theme custom palette/accent/dynamic settings are preserved.
- The real bundled wallpaper and its fill/fit/stretch/center/tile mode. Missing
  assets are labelled; no stock gradient is substituted. Paired wallpaper
  selection follows the existing production engine.
- Actual enabled desktop widget definitions, instance options, anchors/offsets,
  native host clearances and native `widgets::paint` pixels. Builtin references
  resolve to their real shared definitions. Fixed sample data is labelled.
- Actual `DecBar` geometry/painter, with correct edge/split layout and stable sample
  workspace/time data; absent system sensors are not fabricated. No live probing,
  push sources, companion bars or input actions are started by the preview method.
- Actual Mac dock / Windows taskbar painters and declared geometry, with two
  explicitly sample native pins (Launchpad and Trash). No pretend application
  windows or fake widgets are drawn. Taskbar edge and Mac margin rules are retained.
- Optional **native rdwm-deco** focused/unfocused caption samples in a separate
  labelled footer. This represents structure without inventing app-window
  placements over the package's widget layout.

Desktop canvas is a common 1920 × 1080 logical output, plus a 120 px evidence
footer. It is not a pixel capture of the running desktop or a multi-monitor
simulation. Compositor glass/refraction, animation, hover/input and backdrops
behind real app windows are not simulated. Unsupported bar widget placement and
missing render dependencies are labelled. No picture is presented as a full
working desktop when a component could not be rendered.

For captions, build against a **read-only** rdwm checkout:

```sh
python tools/build-decoration-preview.py /path/to/rdwm
export DECEBAL_DECO_RENDERER="$PWD/target/decoration-preview-helper/target/debug/decebal-deco-preview"
```

A helper installed beside `decthemectl` is discovered automatically; `DECEBAL_DECO_RENDERER` can override it.

No rdwm source or live binary is changed. The helper uses rdwm-deco directly; it
is not a second decoration renderer. Without this optional helper, previews state
that native decoration rendering is unavailable and draw no substitute buttons.
Packaging a stable native caption-render API with rdwm remains an integration
follow-up. Font files in packages remain subject to the engine's current manual
registration limit; previews show the real installed fallback, not a font mockup.

## Cache and lifecycle

Cache: `$XDG_CACHE_HOME/decebal/theme-previews` (normal XDG fallback).
SHA256 keys include raw manifests, inherited package assets, widget definitions,
builtin widget source, effective selection/palette, resolved installed font file
identity, native renderer version and helper binary identity. Screenshots and
root documentation are excluded to avoid self-invalidating generated evidence.
Wallpaper files outside the conventional directory are included too. Changes to
palette contents, layout, artwork or widget code therefore choose a new artifact.
Renderer changes must bump `preview::VERSION`; rebuilt helper identity also
invalidates caches automatically.

GUI card assembly only reads in-memory preview results and submits to a bounded queue;
it never fingerprints packages, reads assets, or discovers fonts. A separate bounded
inventory worker handles gallery metadata and integrity verification. Unchanged
metadata inventories reuse those results. Preview asset SHA256 digests are memoized
by path/device/inode/size/mtime/ctime (nanosecond precision on Linux); same-size writes,
atomic replacement and removals invalidate them. Content digests remain authoritative
for artifact names. Memoization is process-local; persistent PNG reuse still works
across launches. Revalidation is throttled to once per second per requested selection;
the last completed preview can remain visible until replacement finishes.

One bounded background render worker handles gallery requests. The fontconfig cache
lives at `theme-previews/fontconfig` across jobs rather than in disposable job homes;
system font caches remain readable, and original user font directories remain read-only.
The existing parent helper autodiscovery beside the running executable is preserved.
Queue order remains gallery order (32 pending maximum); viewport priority is deferred
because visibility belongs to card painting while package requests precede layout.
Offscreen themes may still queue; this patch does not claim faster native painting.
 Rendering times out after
45 seconds, publishes PNGs atomically, wakes the gallery and retains a failure
reason under the same key instead of retrying forever. The gallery fits the whole
image, including its sample-data footer, rather than cropping it. The persistent
cache is bounded to 256 own artifacts; decoded thumbnails are bounded to 64.
No generated preview is installed into a theme or changes its integrity hash.

## Verification and evidence

`tests/theme_preview.rs` runs the real isolated CLI worker. It checks cache reuse,
then changes a color, layout offset, widget definition, wallpaper pixels and mode
and verifies regeneration; both mode images differ. It also checks independent
palettes/builtin references affect keys and that source configuration is untouched.
The port contract tests continue to cover safe/reproducible packages, unavailable
states and custom-widget Ask/Keep/Replace/cancel behavior.

`tests/theme-preview-evidence.py ID...` generates evidence with no theme-specific
layout code. Current native dark/light examples live in `docs/theme-ports/automatic`
for Mountain Neon Dot, Glyph Rice, Sweeteh, Kiln Court, Tidal Loom, macOS and NextKDE.
These supersede the earlier widget-only compositions as complete available-native
component evidence. The earlier individual sample/empty cards remain useful QA.

Parent preview-gallery commits through `e33a0f1` were preserved via three local
cherry-picks before this change. This work does not replace its livewall gallery,
video preview implementation or unrelated Settings changes.

Performance/fitting regression (2026-10-30): the fixture measured 1.48–1.54 s cold and
47–49 ms persistent cached render on this machine (small 20px wallpaper, one widget;
not a full-theme benchmark). 100 memoized 4 MiB asset digest lookups took 261–292 µs.
`settings::themecards::tests::edge_marked_preview_is_contained_without_stretch_at_multiple_aspects`
checks all four distinct edge colors and the 2:1 aspect ratio in square/wide/tall
slots, with PNG evidence in `target/preview-fit-evidence/`. The gallery now uses a
uniform contain pixmap transform directly, without a cover crop or rounded edge mask.

The gallery queues rendering only for visible cards. Off-screen cards retain metadata and cached results without entering the render queue, so they cannot delay the previews currently in view.
