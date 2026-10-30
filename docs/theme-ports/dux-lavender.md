# Dux Lavender — native package evidence

## Replica acceptance reset — authoritative current scope

The earlier lavender/four-corner implementation is **not a Dux replica**. Its
readability/geometry checks remain useful engineering evidence, not visual parity.
All earlier “acceptance” labels below refer only to that superseded approximation.
The user requires source-by-source replica sections; the accepted second-pass
Settings is preserved separately. No screenshot or painted mock is live native UI.

Authoritative READ-ONLY clones:

- `/home/ambsd/Projects/dux-shell-reference` at
  `87312075dd414a6f60691464bcd6c6a9bb242a7e`.
- Its pinned `dux-qml-common` at`43e05926fcdacc53675623169bcef686faa95301`.
- `/home/ambsd/Projects/dux-plugins-reference` at
  `a55bb40da2f70640600a520ea05eac9c3540f1af`.

Earlier shell46a12d9 was inspected too; stockPurple pairs are unchanged. Do not mix
README's historic compact26px screenshot geometry with current comfortable source
38px band defaults and claim an exact same-preset comparison. Source correctness
and screenshot-profile overrides must be recorded separately.

### Ordered parity ledger

| Order/section | Exact source target | Current native/package state | Remaining contract / owner |
| --- | --- | --- | --- |
|1.Bar islands/items|Three independent section groups; launcher/workspaces/focus left, media/clock/weather centre, SNI/clipboard/CPU/RAM/notifications/battery/control centre right|**Implemented difference:** existing application-menu pill/Apple/Finder is rejected; not parity|Native owner implements opt-in Dux composition, hit maps, overflow, source icons and isolated captures; package activates only after validated schema|
|2.Dashboard/control centre|One anchored tabbed surface; actual overview grid, clock/weather/calendar/user/system/media; source tonal controls|**Remaining:** four desktop cards are not a substitute|Native owner supplies dashboard.style/control_center.style Dux, real routes/data/actions; specialist maps tokens/layout/preset|
|3.Clock/weather/media/system widgets|Actual DDash card topology, artwork/transport, vertical clock where configured, calendar dates and honest missing states|**Implemented difference:** current generic shared cards only prove accessibility bounds|Native owner supplies definitions/renderers; specialist selects profile and verifies exact appearance, no duplicate widget code|
|4.Launcher/notifications/rest|Pinned source lists, spacing, typography, actions and mode states|**Remaining:** no replica claim|Native owner follows earlier sections; preserve accepted Settings and independent notification functionality|
|Typography/assets prerequisite|Google Sans Flex400/500/700; Material Symbols Rounded; actual source family/axes|**Exact assets prepared:** static400 derived at source defaults, static500/700 and Material variable copied; private native400/500/700 glyph test passes|Native owner process-private FontDb/PUA cache; font.files activation pending validated handoff, no system font install|
|Stock colour pairs|Full Purple material roles, not simply accent substitution|**Partial exact:** background/foreground/accent match; old popup/card role mapping and20px generic radius differ|Package owner corrects role mapping and radius with staged native section; missing extra roles must be typed, not invented manifest fields|

### Section1 — source geometry and behaviour contract

Source paths are relative to dux-shell-reference. `quickshell/Common/settings/SettingsSpec.js`
barConfigs defaults: spacing4,innerPadding2,widgetPadding8,borderEnabledfalse,
widgetOutlineEnabledfalse,widgetStylepills,maximizeWidgetIconsfalse,
maximizeWidgetTextfalse,fontScale1.0,transparency1.0,widgetTransparency1.0.
Exact order:

- Left:`launcherButton`,`workspaceSwitcher`,`focusedWindow`.
- Centre:`music`,`clock`,`weather`.
- Right:`systemTray`,`clipboard`,`cpuUsage`,`memUsage`,`notificationButton`,
  `battery`,`controlCenterButton`.

`Common/Theme.qml`:barHeight48 is a **base**, not final band size.
`barWidgetThickness(2,1)=snapEven(max(20,26+2*.6))=28`;
`barThickness(2,1)=snapEven(max(28+2+4,48-4-(8-2)))=38`.
At38/48=.792,barTextSize takes its middle branch:12px normal or16px maximized;
it does not multiply text size by.792. Default icon size is
`2*round((38/48)*(24-6)/2)=14px`.
`SurfaceWidgets/BarPill.qml`:horizontal padding`snap(8*(28/30),DPR)`=7px atDPR1;
background comes fromBarMetrics.widgetFill→Theme.widgetBaseBackgroundColor→cardSurface.
Source exact snap implementation uses rounding toDPR pixels. Pill face outer radius
is14 at28px face with expressive pill controls; joined inner corners8,pressed4,
ordinary pill gaps4 and grouped segment gaps2. No arbitrary1px outline by default.
Preserve native mouse/key/scroll actions and accessibility hit targets, not just paint.
No player: no phantom song or transport; no battery: no fake percentage; unavailable
weather/system metrics remain explicit or source-appropriate hidden state.

**Schema coordination:** native owner agreed`bar.style="dux"`,
`dashboard.style="dux"`,`control_center.style="dux"`; these are contracts, not
validated support at this checkpoint. Proposed staged bar knobs: height38(default)
or26(explicit compact screenshot preset),margin8,spacing4,radius14. Item identifiers
must be the canonical names above; no silently ignored arbitrary list keys.
Package must not write new selectors until native tests confirm wiring. General
Settings, bar/controls/widget engine and schema are native-thread ownership.

### Source tokens, typography and later-section geometry

`Common/Theme.qml`:spacingXXS/XS/S/M/L/XL=2/4/8/12/16/24;
fontSmall/Medium/Large/XLarge/XXLarge/Display/DisplayLarge=12/14/16/20/28/36/57;
weights400/500/700; iconSmall/Medium/default/Large=16/20/24/32;
minimumTouchTarget48,iconButton40. `DCommon/Common/Shape.js`:corners
2/4/8/12/16/20/28/32/48; radiusStrength50=>scale1,cardM12,popupXL28.
Default densitycomfortable and interfaceexpressive; density steps are separate from
saved grid cell sizes and must not be approximated by globally enlarging all gaps.

| StockPurple role | Dark | Light |
| --- | --- | --- |
|primary/onPrimary|D0BCFF /381E72|6750A4 /FFFFFF|
|primaryContainer|4F378B|EADDFF|
|secondary|CCC2DC|625B71|
|surface/background|141218|FEF7FF|
|surfaceText/backgroundText|E6E0E9|1D1B20|
|surfaceVariant/onVariant|49454E /CAC4CF|E7E0EB /49454E|
|outline|948F99|7A757F|
|surfaceContainerLowest|100E14|FFF9FF|
|surfaceContainerLow|1D1B20|F4EEF6|
|surfaceContainer|211F24|F2ECF4|
|surfaceContainerHigh|2B292F|ECE6EE|
|surfaceContainerHighest|36343A|E6E0E9|

Default cardSurface=surfaceContainer,chipSurface=surfaceContainerHigh,nested chip=
surfaceContainerHighest. Existing package popup=high is a role difference, not exact.
`Common/PopoutMetrics.qml`:contentPadding16,contentGap12,chromeButton32,
chromeIcon16,panelChromeInset8 at comfortable density.
`Modules/DDash/DashMetrics.qml`:default6columns,preferredColumn128,minimumPopout700,
contentPadding12,contentGap12,gridRowUnit96,gap8,cardRadius12,surfaceRadius28.
Derived default width`6*128+5*8+24=832px`; source caps to screen width minus32.
`ControlCenter/CcMetrics.qml`:default8columns,min6,baseTile64 (48+2*8),gap8,
sheetPadding16; default width`2*16+8*64+7*8=600px`. Source viewport caps/density and
actual availability determine any smaller layout; do not fake missing components.

`DCommon/Common/Fonts.qml` loads actualGoogleSansFlex, FiraCode and displayDMSerif;
`DIcon.qml` pins unfilledFILL0,wght400,GRAD-25dark/0light,**opsz24**, not currentpx.
Filled state useswght500. Native owner's actualpx optical axis is an explicitly
tracked difference unless corrected. Full MIT shell/common, OFL font andApache2icon
notices must remain distinct. No screenshot wallpaper or album art is licensed by
softwareMIT alone. Existing reviewed photographic choices and Follow theme stay
independent; no generated wallpaper substitution.

### Comparison evidence acceptance

For each section, capture actual upstream module/native surface at the **same**
viewport1920×1080 (and compact1280×720), stockPurple mode and100/200%text/DPR1,
same configured38px default or explicit26px compact, and labelled sample/missing
data. Capture live runtime only on private displays/processes with private HOME/XDG/
bus. Root's:0 and:430 desktops are forbidden. Do not replace source UI with a hand
painted mock to manufacture similarity. README screenshot pixels are reference,
not proof of current source settings/DPI and not shipped as a live wallpaper/UI.

Actual native bar handoff and matched upstream captures are pending section1.
Existing generic package/native screenshots below are **baseline differences**,
not replica proof. Until measured native section capture exists, status remains
remaining even if schema compiles. No changes to accepted Settings, gestures or
live configuration. Only this package and this report are specialist-owned now.

---


**Current acceptance:** the four-corner layout and shared companion fix now pass
combined100/200% text QA at1280×720. See the final section below. Earlier clipping
and pending-handoff notes are historical and superseded; regenerated images are
current. Parent1af0592 already resolved the historical NextKDE audit mismatch.

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


## Final companion readability acceptance — shared380cd1a

Shared source `380cd1a802a5d1693aebf960d11ebee0878f05e0` cherry-picked **separately**
as local25839b5. Package layout commit is`db79caad96e031d7586d4149fbbe5e16bdc0d58b`:
media bottom-right(-32,-32), focus bottom-left(32,-32), clock top-left(32,72),
resources top-right(-32,72). Parent integrates the original shared commit once,
then layout/evidence deltas; the evidence commit contains no shared widget source.
No package palette/font/art/default change is needed.

Actual Dux-native sizes match the frozen handoff:

| Widget | 100% both states | 200% sample | 200% missing |
| --- | --- | --- | --- |
| clock-card |236×136|472×233|472×199|
| now-playing-mini |310×76|620×91|620×91|
| resource-overview |320×248|320×324|320×324|
| focus-hub |320×292|320×363|320×363|

All16 combined native canvases (1280×720/1920×1080 × dark/light ×100/200% ×
sample/missing) are in `dux-lavender/widgets/combined-*`, with exact JSON bounds.
At1280×720/200%sample: clock(32,72)-(504,305), focus(32,325)-(352,688),
resources(928,72)-(1248,396), media(628,597)-(1248,688). Minimum clock/focus gap20px;
no pair overlaps or escapes the output/bar inset. Missing weather shrinks the clock
height to199px; it does not move other anchors. Four shared definitions and actual
native layout/paint sizes drive composition, not hardcoded per-theme mockups.

Viewed combined100%/200% dark/light and missing-state canvases. Full HH:MM and
complete sample date render; sample media Midnight City/M83 remain meaningful and
unclipped. Tests compare clock/date leaf text to the full native formatter output,
check title/artist presence, every node/action rectangle against root bounds and
centre-hit action equality. Sample player has all three transport buttons; missing
player has Nothing playing and no transport. No action was dispatched to the live
desktop. Focus long-note/title ellipsis is intentional and remains visible in QA.

Validation:4focused Dux integration tests pass with serial process-local font
publication; lib+targeted-test strictClippy/format/diff checks pass. To avoid an
all-binaries rebuild, built library only (`cargo build --lib --locked --offline
-j2 --message-format=json`), used its actual compiler-artifact dependency paths to
compile `tests/dux_theme.rs` directly with rustc --test, then ran that one test
binary serially. Only decthemectl was additionally built for updated automatic
package previews. No release/all-binary build, live provider probe, install or
restart. Build command/dependency record is in `dux-lavender/validation.json`.

Automatic100%package previews regenerated with actual new renderer definitions;
all combined/individual evidence hashes refreshed in evidence.json. Earlier
clipped companion PNGs are replaced by final renders at the same paths. The earlier
pending/readability caveats in this chronological report are therefore **resolved
for these measured fonts, fixture strings, scales and viewport sizes**. Other
fonts/locales, >200%text, narrower/shorter screens, physicalDPI and live compositor
shadows remain outside this bounded acceptance. General host packing was not added.

Follow theme choices remain exact reviewed l3edjp/39zjq6 hashes, no package wallpaper
default or download. MIT source attribution unchanged. No runtime/Settings/rdwm
implementation copied/edited by this evidence delta; no live configuration changes.

### Section3 original font and actual DClockFace probe (not activated)

Exact4153300-byte original variable font is now available as
`fonts/GoogleSansFlex-Variable.ttf`, same pinned common source/OFL; SHA256
`c6d53424121196b81de816b8daccf200e285dd506df43766db3d7e8cdf06ee30`.
font.files remains FOUR entries: static UI400/500/700 and Material. Private native
fixtures may add the fifth for tests; package activation awaits renderer SHA/QA.

Actual unmodified common DClockFace was instantiated twice per private headless
Sway startup:102×184 and408×400; hours09/minutes08, date Friday,30 October,
roundedtrue, weight750, stock Purple dark/light, fontScale1/2. Four startups yielded
eight raw component grabs, no painted wrapper/chrome or mock. Source uses automatic
vertical layout for both sizes. fontFamily stays empty so variableFont=true,
actual Google Sans Flex variable axes ROND100/wght750 are selected. Optical size
is derived from vertical height/2:92 and200 (requested axis values, not a claim
that the font's own axis range extends to200). No manual forced opsz108.

| Content |Text scale|Digit size (pre-rounding)|Support size|Support height incl gap|Block top|
|---|---|---|---|---|---|
|102×184|100|80.832095|15.156018|23.546805|14.788930|
|102×184|200|80.832095|28|44.4|4.988430|
|408×400|100/200|201.786667|50|78|0|

Dark/light geometry matches. Reference-size100 digitInk heights75 =>inkRatio.75;
small rowAdvance1.261875/widthSize80.832095, large1.2159375/widthSize335.543562.
The visible digit font pixel sizes round to81/202. Source support uses HorizontalFit
+elide/minimumPixelSize; small date is visibly cramped/tiny, especially with enlarged
minimum text. Native comparison must distinguish ink-based display sizing from
ordinary UI text enlargement and document any intentional date readability repair.
Raw properties/child boxes: `/tmp/decebal-dux-replica-specialist/clock-capture/source-metrics.json`;
harness/config/logs/captures in that directory. No live display, global fonts,
reference writes or application launch. Native comparison awaits native renderer;
these source captures do not certify native axes or whole-clock parity.
