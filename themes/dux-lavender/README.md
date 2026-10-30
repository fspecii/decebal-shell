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
At 100% text the four-card layout is checked at 1280×720 and 1920×1080.
All four card bounds also fit at 200% text on 1280×720 and 1920×1080, proven by
combined native renders. Existing clock/media cards have fixed geometry: the clock
clips time/date at 200% and media text truncates. This is a known readability gap,
not a fully accessible 200% profile; shared-card reflow is a parent follow-up. Very short/narrow displays
or larger desktop scaling may need the user to reposition/reduce cards. Settings/accessibility work
belongs to the independent native thread, not this package.

Primary references, exact commits and MIT notice: [ATTRIBUTION.md](ATTRIBUTION.md)
and [LICENSE.md](LICENSE.md). Regeneration/tests and inspected evidence:
[../../docs/theme-ports/dux-lavender.md](../../docs/theme-ports/dux-lavender.md).


Layout follow-up: media now anchors lower-right, opposite lower-left focus. Shared
clock/media accessible sizing is being implemented separately. Existing committed
combined screenshots predate this move and are historical until regenerated with
the exact shared fix; their 200% clipping is not a current acceptance claim. The
reported larger dimensions are provisional until the native agent hands off its
commit and measured bounds. No widget DSL implementation is duplicated here.
