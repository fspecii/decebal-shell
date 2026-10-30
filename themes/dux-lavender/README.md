# Dux port — bar, dashboard, Control Center and Applications

The original lavender/four-corner theme was not a replica. This package is being
ported in source order: **bar → dashboard/control centre → native widgets →
launcher/notifications**. The accepted second-pass Settings is preserved. Each
section needs actual upstream/native comparisons, not a generic palette mockup.

Section1 selects the native `bar.style="dux"` with the source38px band,
28px widget face,12px text/14px icons,7px per-face content padding,4px item gaps,
and canonical left/centre/right item order. Source Purple dark/light roles are
mapped to native surfaces; pill radius is14, band radius16, card radius12 and default outlines are off.
The source bar is a rounded band containing individual widget pills and three
**layout groups**, not three enormous pills. Independent-island treatment is a
separate profile, not stock parity. Requires native renderer/schema/font-registration commit `8addeef` (or its integrated equivalent).
The band starts at4,4; first face8,9. Text scale200% enlarges text only, not band geometry.

## Version1.2.0 compatibility and scope

Requires Decebal0.2.0. This metadata checkpoint targets parent candidate`0c01f41`:
stock Purple bar, historical README dashboard, Control Center and section4
Applications-only list launcher. It does not claim a complete Dux replica.
Native source/integrated dependencies are explicit:

| Capability | Original native commit | Parent integrated commit |
| --- | --- | --- |
|Bar/private font registration|`8addeef`|`4c20ef5`|
|Dashboard/Control Center/explicit empty desktop|`1b45fdf`|`eff0175`|
|Applications-provider launcher|`8eedcc0`|`b56ca4b`|

This metadata-only delta adds no selectors, layouts, assets or font registrations.
The parent checkpoint retains its four font.files entries (static400/500/700 and
Material). Original variable-clock preparation/registration is separate future
integration work and is not a dependency of this checkpoint. Applications excludes
source All/Files/Plugins/history/richer actions/fuzzy/motion. Dashboard profile
`readme` is historical, not current upstream defaults; expressive shapes/controls
and other parity gaps remain. Accepted Settings and independent wallpaper choice
are preserved. Parent owns integration, minimum-version enforcement and activation.

## Native typography and icons

Bundled Google Sans Flex static400/500/700 uses the actual upstream typeface.
400 is a documented static default-axis instance;500/700 are unchanged upstream
static faces. Material Symbols Rounded is the exact upstream variable icon font.
`font.files` registers those4UI/icon files plus the original variable clock font as fifth entry only in the shell process. No system font
installation, fontconfig-cache edit, or unsupported variable regular face. Semantic
icons use the real source outlines/PUA mappings; Adwaita/hicolor remains only the
application-icon preference. Source DIcon axes are FILL0,GRAD-25dark/0light,opsz24,
weight400 unfilled/500 filled. The selector/API must not silently fall back to SF
symbols/Inter and still claim parity.

See [FONT-BUILD.md](FONT-BUILD.md), [OFL.txt](OFL.txt),
[MaterialSymbolsRounded-LICENSE.txt](MaterialSymbolsRounded-LICENSE.txt),
[ATTRIBUTION.md](ATTRIBUTION.md) and [LICENSE.md](LICENSE.md).

## Independent user choices

Wallpaper → For this theme still recommends previously reviewed real photographs:
[moon/Mount Cook,l3edjp](https://wallhaven.cc/w/l3edjp) and
[lavender torii sunset,39zjq6](https://wallhaven.cc/w/39zjq6). Author/licence remain
unknown as documented; uploader is not presumed photographer. No image is bundled,
downloaded or selected by this package. Follow theme remains a separate user
preference. No generated wallpaper substitutes for the upstream design.

No bottom dock. Functional native close/minimise/maximise controls remain enabled;
Materia is a native decoration choice, not a claim that Dux owns WM decorations.

The validated section2 selectors choose `dashboard.style="dux"`, explicit historical
`dashboard.profile="readme"` and `control_center.style="dux"` (native `1b45fdf`,
after `8addeef` and parent CPU/popup fixes). This is not the current upstream default
card/tab preset. Settings remains an action. `[desktop] widgets="none"` replaces
the previous generic corner-card profile only through existing Ask/Keep/Replace/Cancel
confirmation; no `[[widgets]]` entries accompany this explicit empty profile.

Expressive artwork/motion, calendar event creation, hourly
weather, Control Center device/detail sheets and editable grids remain differences.
Missing telemetry/artwork stays unavailable. Parent controls integration/activation.

## Evidence and remaining work

[REPLICA-EVIDENCE.md](REPLICA-EVIDENCE.md) records actual source-module captures.
They are review evidence only: no manifest screenshots field, no screenshot as live
UI, no copied wallpaper/album-art, and no hand-painted native comparison surface.
Source capture runs on private headless Wayland; native capture uses isolated native
renderers. Modes, viewport, font scale, provider state and any differences must be
explicit. Full ordered parity ledger/source metrics:
[../../docs/theme-ports/dux-lavender.md](../../docs/theme-ports/dux-lavender.md).

Current source defaults differ from the historic README screenshot: source bar
band38 atDPR1, widget faces28, default6-column dashboard832px, and a configurable
card/tab registry. The README four-tab/user/sysmon composition is a historical
configured profile. Do not mix those presets and call the result exact. Settings
is an action, not a fictitious fourth content page. Missing data remains honest,
even where upstream code falls back to zero. Replica completion is not claimed
until each native section passes its own visual and action checks.

## Section4 Applications provider

`[launcher] style="dux"` requires native`8eedcc0` after section2. It selects the
source full-style/compact-size Applications **list**,620×620 when the output fits,
with real cached application icons/comments and typed launch actions. At200%,
rows grow84px to fix the upstream56px overlap; icons36/search48 remain fixed.
This is Apps-only: source All/Files/Plugins, history, richer actions, fuzzy ranking
and expressive motion are not implemented. No painted inert provider tabs imply
those capabilities. Notification visuals remain separate. No full replica claim.

## Section3 variable clock

Requires native`a8068ec`. The fifth font.files entry registers exact original Google
Sans Flex variable bytes under the same OFL; static UI400/500/700 remain selected
for ordinary text. The README clock requests wght750/ROND100/opszheight/2 and fits
actual ink metrics. Eight fixed-size actual source/native comparisons pass within
2px; antialiasing/advance differences remain. Supporting dates deliberately fit
and elide at the12×text-scale minimum rather than reproduce Qt's cramped clip.
Missing font shows Clock font unavailable, never generic replacement digits.
No seconds, analog mode, animated digits or native Wayland-host acceptance claimed.
