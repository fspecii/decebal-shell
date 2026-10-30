# Dux Lavender — native package evidence

Inspected pinned Dux Shell README desktop/dashboard screenshots, stock theme source,
current dashboard QML, first-party plugin README/Pomodoro manifest and registry.
See [package attribution](../../themes/dux-lavender/ATTRIBUTION.md) and
[source hashes](dux-lavender/sources.json). Shell/plugins MIT notice is retained;
registry has no root licence, and no third-party plugin/artwork/code is copied.

The package adapts stock Purple dark/light: `#141218`/`#fef7ff` backgrounds,
`#D0BCFF`/`#6750A4` accents, stepped tonal containers and 20px cards. Existing
Materia native captions, left-aligned titles,26px floating top bar, no bottom dock,
installed Inter/Adwaita/Noto fonts and Adwaita/hicolor icons. No cursor or branded
art/font bundle. Source metadata remains ordinary supported theme fields.

## Integration

Parent08c07a2 was inspected and synchronized in this checkout. Merge3e79026 has
exactly the parent tree; it retains all earlier specialist commits in history,
without replacing newer parent runtime/Settings/preview source.

Shared widget-only commits from the explicitly requested separate native thread:

- `de48c1ce48a3bffdb7c42778ce095fb31d39473a` → local dependency`fb3cc1d`.
- `37c115fc9fcae93356a3bf65a0ff145a831eaf6d` → local dependency`842c8a7`.

Parent should integrate the originals once, then cherry-pick this package commit;
the dependency cherry-picks need not be applied twice. Settings commit1f4f30b and
later Settings work are not included here. No shared widget implementation was
written by this theme task. No extra builtin-theme registration is needed:
installed package directories are discovered by Source/store and Make installs
all theme package files; source/discovery tests explicitly find`dux-lavender`.

## Actual renderer evidence

- [Dark package](automatic/dux-lavender-dark.png),
  [light package](automatic/dux-lavender-light.png): production theme plan/mappings,
  actual native bar/widget painters and native rdwm caption helper,1920×1080 plus
  labelled120px footer. **FIXED SAMPLE DATA**, no live compositor shadows/motion.
- [Widget renders/layout dumps](dux-lavender/widgets):16normal/missing images across
  four actual widgets and both modes; four further new-card200% text images.
  Missing media hides transport; absent resources say Unavailable. Sample Notes
  title/timer/resource readings are fixture values, not the user's data.
- [Materia native sheets](dux-lavender/captions): dark/light1×/2× focused/unfocused
  and hover states from the actual rdwm renderer. Helper asserts three in-bounds
  button rectangles and matching Close/Minimize/Zoom centre hit tests. No live
  window actions were invoked. Apply-plan tests preserve maximize and enable
  captions/buttons even from a saved buttons=false configuration.
- `/tmp/decebal-dux-photo-evidence-final/dux-lavender-{dark,light}.png`: private
  actual previews with exact reviewed l3edjp/39zjq6 originals. Photos and derivatives
  stay outside Git. Both original full files were viewed again; 16:9 fill trims
  the dark photo's sky/reflection, while the light gate/sun remains unobscured.
  Source photographer/licence remain unknown; screenshots do not confer rights.

100% text:resource320×248,focus320×292.200% text after follow-up widget fix:
resource320×324,focus320×363. Normal profile uses left clock/media, right-top
resource and left-bottom focus. Native dimensions show no overlap/edge escape
at1280×720/1920×1080 normal text; the two new cards also fit together at1920×1080
with200% text. Clock/media are existing fixed-size widgets, **not certified for
200% reflow**. Smaller/scaled desktops can require reducing/repositioning cards.

## Reproduce without live apply

```sh
cargo build --locked --offline --bin decthemectl -j2
DECEBAL_DUX_EVIDENCE="$PWD/docs/theme-ports/dux-lavender/widgets" cargo test --locked --offline --test dux_theme -j2 -- --test-threads=1
cargo test --locked --offline --test curated_theme -j2
python3 tools/build-decoration-preview.py /home/ambsd/Projects/rdwm
DECEBAL_DECO_RENDERER="$PWD/target/decoration-preview-helper/target/debug/decebal-deco-preview" python3 tests/theme-preview-evidence.py dux-lavender
```

Photo evidence uses `tools/render-curated-theme-evidence.py dux-lavender` with
`--dark-photo`/`--light-photo` exact reviewed cached originals and `--out` under
`/tmp`. Helper validates catalogue SHA256, stages private independent backgrounds,
and isolates HOME/XDG/config/bus/display; it neither mutates package manifests nor
downloads/applies anything. `-o FILE` is the actual pack CLI output flag.

Targeted checks:3Dux tests,4curated-package tests,3applicable Python art-policy tests;
strict targeted Clippy/format/diff checks. Folder/archive validation and repeated
pack bytes checked. One expected schema warning remains: decoration.buttons side
hint has no separate rdwm side key. The boolean is applied and tested; Materia
chooses side/order. No unsupported side/order behaviour is claimed.

**Historical validation (resolved by parent `1af0592`):** the earlier full Python
artwork audit reported a NextKDE hash mismatch:
current`f27882467c6fa346d1d2e91877be66ede14d08671b0910f732dc5903f2061234`
versus older audit`eedb3bf63306a97771ada102140a6fbad5da6a1e6f80a289b80b1d2daa904520`.
No old audit record or NextKDE manifest was weakened/edited to hide it.

## Rejected approaches and follow-ups

No copied screenshot art, generated wallpaper, custom font install, QML runtime,
arbitrary script/plugin actions, invented telemetry, manual generic gallery art or
disabled window controls. Dux's segmented islands and tabbed dashboard/calendar
need engine features; this package uses one existing native bar and desktop cards.
The bar still has native application-menu content (including renderer's labelled
Finder fixture), not Dux's exact widget order. Do not claim a native Dux bar port.

An early evidence attempt wrote temporary wallpaper metadata; parent independent
preview logic correctly ignored it for catalogued themes. The owned helper now
stages the reviewed private background cache instead. Bare LICENSE/SOURCES.json
were rejected by package data-only rules; licence moved to LICENSE.md and source
hashes to docs. These restrictions are preserved, not relaxed.

Catalogue now covers22visibleIDs/44modechoices/42distinctreviewedphotos: intentional
two-photo reuse avoids invented IDs/new searches. Theme apply preserves independent
Wallpaper preferences; custom widget Keep/Replace/Cancel remains intact.
No install, live config mutation, restart, push or rdwm edit. Parent owns rollout.


## Short-output acceptance follow-up (parent fca4442)

Focus now anchors bottom-left(32,-32), opposite top-right resources. The earlier
both-right arrangement needs807px at enlarged text and is retired. Combined native
**all-four-card** renders and JSON bounds in `dux-lavender/widgets/combined-*` cover
1280×720/1920×1080, dark/light,100/200%text. At720/200%: clock(32,72,268,208),
media(32,224,342,300),focus(32,325,352,688),resources(928,72,1248,396).
No overlap or output-edge escape. Canvas contains actual widget painters over theme
background, fixed labelled sample data described by JSON/filename, no live host.

**Readability limitation:** 200% fixed clock clips time/date and media truncates text.
The new resource/focus cards remain readable. Geometry passing is not full200%
visual acceptance. Parent can own shared clock/media accessible reflow; general
host responsive packing is not required for this measured1280×720 arrangement.
Narrower/shorter outputs remain unverified. No responsive-profile feature invented.

Follow theme acceptance: `dux-lavender` already has real dark/light catalogue entries,
no wallpaper section/default. Dark l3edjp reviewedSHA256
`931e8e461a521afe0eb53438358a487b7dd6b40f62adc2ad83df32d6571e7237`;
light39zjq6 reviewedSHA256
`b6a4fbaaaf5719f87b69d9911b0192ad8b6b493e9f1de77475c00815fba77b0c`.
Both were previously reviewed, purple-suited photographs; metadata remains22IDs,
44mode recommendations,42uniquephotos. Parent fca4442 curated lookup inspected
read-only; no runtime/fetch changes. No new photo searches/downloads or defaults.

Follow-up validation:4Dux+4curatedintegrationtests pass, targeted strictClippy and
format/diff checks clean. Actual combined profile evidence and known clipped200%
clock were viewed. No release/all-target build or install was requested/run.
Dependencies remain originalde48c1c+37c115f, package006f0c5 then this follow-up.


## Four-corner layout for shared readable sizing — pending native handoff

Media moved from upper-left(32,224) to bottom-right(-32,-32). Clock stays upper-left,
resources upper-right, focus lower-left. This package-only change anticipates the
separate shared WidgetDef.text_scale_width/auto_height implementation; it neither
copies that implementation nor changes100% widget fonts/sizes.

Previously committed combined/automatic screenshots are **historical**, predating
this move and the new shared clock/media sizing. Do not use them as acceptance of
the new readability fix. Regenerate with the exact handed-off shared commit and
actual measured dimensions, including full time/date/media text and action bounds
at1280×720 in both modes and100/200%text. Reported clock472×~231/media620×~91
are preliminary, not substituted for actual renderer measurements.

Parent source312de68 inspected read-only. NextKDE audit mismatch was resolved in
parent1af0592; earlier failure text above is historical, not an outstanding failure
of current root. No new root cherry-pick or runtime edits in this layout checkpoint.
