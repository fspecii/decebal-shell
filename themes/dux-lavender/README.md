# Dux Lavender

A native Decebal adaptation of Dux Shell's stock Purple colour system and soft,
tonal dashboard cards. Dark charcoal/lavender and warm porcelain/violet modes,
one 26px floating top panel, existing Materia captions with working window buttons,
and **no bottom dock**. Four optional cards leave the central desktop open:

| Shared widget | Logical size | Placement |
| --- | --- | --- |
| `builtin:clock-card` | 236×136 | upper-left,32/72 |
| `builtin:now-playing-mini` | 310×76 at 100% | lower-right,-32/-32 |
| `builtin:resource-overview` | 320×248 | upper-right,-32/72 |
| `builtin:focus-hub` | 320×292 | lower-left,32/-32 |

Resources and focus cards are shared native definitions from the separate Dux
Settings/widgets work, not private copies or QML plugins. They use existing local
providers and allowed actions. Weather needs the user's weather setup; MPRIS needs
a player; Notes/Timer use their shared native stores. Missing states remain explicit.
No arbitrary scripts, downloads, plugin install, fake uptime or populated accounts.
Existing custom layouts still require **Keep / Replace / Cancel**; cancel writes nothing.

Wallpaper → For this theme offers individually re-reviewed photographic choices:
[moon over Mount Cook,l3edjp](https://wallhaven.cc/w/l3edjp) for dark and
[lavender torii sunset,39zjq6](https://wallhaven.cc/w/39zjq6) for light. These deliberately
reuse existing reviewed catalogue entries, not new unverified art. User selection,
image fitting and downloads remain independent. No photograph is bundled, selected
or downloaded by theme apply. Authorship/licence remain unverified as recorded in
the catalogue; uploader is not assumed to be photographer. The upstream screenshot's
bright illustrated mountain is **reference only**, not redistributed or claimed human-made.
The separate generated Tidal Loom trial is unrelated and is not a default here.

Uses installed Inter / Adwaita Sans / Noto Sans with a mono fallback and Adwaita icons
(hicolor fallback). No unsupported Google Sans or cursor/font bundle. No branded Dux
launcher glyph. System/application integration is limited to Decebal's existing
theme machinery; no upstream matugen/dank16 scripts are installed or executed.

## Fidelity limits

This is an inspired native adaptation, not Dux Shell running inside Decebal.
Decebal's floating bar is one pill with its existing application-menu content; it cannot reproduce Dux's three independently
segmented islands without engine changes. Cards are optional desktop widgets,
not a tabbed popover with a calendar, avatar, per-core/temperature bars or art player.
Materia is an existing rdwm structural design, not a Dux-owned compositor decoration.
Blur/shadows depend on compositor support; evidence uses actual native foreground
renderers and labelled fixed samples, not live telemetry or compositor shadows.
At 100% and 200% text the complete four-card layout is checked at 1280×720 and
1920×1080 in dark/light and sample/missing-provider states. Shared commit
`380cd1a802a5d1693aebf960d11ebee0878f05e0` preserves standard sizes/fonts and enables
measured companion-card growth. At 200%: clock472×233 with weather /472×199 without,
media620×91, resource320×324, focus320×363. Full time/date and meaningful sample
title/artist fit; transport hit targets stay in bounds. Focus note/title ellipsis
for long prose is intentional, not a claim that arbitrary text fits unbounded.
Very short/narrow displays, larger scaling, other fonts/locales and physical DPI
still need review. Settings/accessibility work belongs to the independent native
thread, not this package.

Primary references, exact commits and MIT notice: [ATTRIBUTION.md](ATTRIBUTION.md)
and [LICENSE.md](LICENSE.md). Regeneration/tests and inspected evidence:
[../../docs/theme-ports/dux-lavender.md](../../docs/theme-ports/dux-lavender.md).


Final four-corner layout: clock upper-left, resources upper-right, focus lower-left,
media lower-right. Updated combined screenshots/layout JSON and readability/action
assertions supersede the earlier clipped200% evidence. Shared implementation is
reused as a separate dependency; no widget code is copied into this package.
