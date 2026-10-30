# Glyph Rice for Decebal — six-card refinement

The existing port of [Nebelung-cloud/glyph-rice](https://github.com/Nebelung-cloud/glyph-rice)
is recorded against revision `39a2a097300bd41e2927706e85bac0f9f8d57d8b` in
its original Decebal import. This refinement follows the actual user-provided
monochrome dashboard screenshot ending
`e808c311-08d1-4b1c-bcab-88b002ce1d9e.png`, inspected 2026-10-29. The screenshot is
a reference only, not redistributed and not used to guess any additional identity.
The stopped monochrome worker's definitions were adapted read-only, not discarded.

## Mapping

The original palette and GPL license are preserved. Static dot-grid/halftone
wallpaper assets are removed; dot typography remains actual native widget pixels. Black background, rounded monochrome cards and red transport
control retain the package's visual identity. A giant dot clock/date and greeting
occupy the left; three 220 px weather/calendar/day cards sit above MPRIS and
system cards at right. Clock and numeric metrics use Decebal's original safe
`dot-text` primitive, independent of installed fonts. Names and explanations use
normal readable text. The greeting uses the real shared user's display name.

Weather uses the shared provider, hides temperature/icon without weather, and
shows Unavailable. Calendar uses the actual local date. The circular day card is
an explicitly labelled **24-hour day progress** ring, not a fabricated date or
step count. MPRIS uses real title/artist/cover art and play/pause state. Its red
transport button only appears with a player; the glyph switches between play and
pause. Missing artwork shows NO ART, never a borrowed album cover. System CPU,
memory, root-disk and history use shared sources; absent values use dashes or
Unavailable. Missing CPU history shows Unavailable instead of a fake graph.

The functional left pill rail remains the native shared Decebal bar: 40 px pill
in a 52 px strip. No fake rail was drawn into wallpaper. There is no dock.
Minimal captions retain real window buttons on the right; the previous
border-only/`buttons = "none"` package combination was deliberately removed.
All six cards are optional through Settings → Widgets. Existing customized
layouts still require Keep / Replace / Cancel, and mode/palette changes preserve
placement. Wallpaper selection is independent of mode/theme changes.

## Attribution and limits

Upstream Glyph is GPL-3.0; its full license remains in LICENSE.txt. Manifest,
widgets retain GPL-3.0-only distribution.
The shared original Decebal dot primitive is MIT. No upstream font, reference
screenshot, third-party album art or new external imagery is redistributed.
Nothing/Glyph names remain trademarks of their owners; this port is unaffiliated.

This is a faithful composition within Decebal's engine, not the upstream QML
shell. The native rail's grouping/icons differ; dynamic island/OSDs, Cava dot
visualizer, earbuds, animated transitions and lock screen are not ported. CPU
history remains a line graph and the day ring is continuous, not a dotted arc.
Google/Doto/Space font families are requested with installed-font fallbacks;
ordinary widget text uses the UI role. A cover-art path that exists but fails to
decode can still leave an empty disc; the DSL has no decode-failure predicate.
The six-card seed targets 1920 × 1200 (16:10); smaller/scaled outputs require
layout editing. Long metadata is truncated to keep the cards within bounds.

## Native evidence and validation

`docs/theme-ports/glyph-rice/` contains 24 native widget PNGs (six × dark/light ×
sample/empty data), plus four labelled compositions. Sample MPRIS/system/weather
values are fixed fixtures, not claims about the user's machine. No live provider,
D-Bus control or desktop configuration was used to create them. Compositions do
not include native rail/window chrome and are **not desktop captures**.

```
DECEBAL_CONFIG=/tmp/private-theme-config DECEBAL_SYSTEM_AUTOMATIC=0 \
 DECEBAL_THEME_EVIDENCE="$PWD/docs/theme-ports" cargo test --test theme_ports --test glyph_rice
python tests/theme-ports-compose.py glyph-rice
```

`tests/glyph-rice-render.py` is now a wrapper for this six-card renderer. The
package test verifies reproducible safe packing; isolated apply plans check
wallpaper preservation in both modes, palette reset, Ask/Keep/Replace and cancellation without
writes. Native render tests check content bounds, honest empty states and all
six widgets. Composition assertions check 1920 × 1200 anchor bounds/non-overlap.

CLI validation reports one existing engine warning for `decoration.buttons`:
left/right is only a side hint. The application engine still writes
`buttons = true`, which is tested starting from the old port's `buttons = false`;
`minimal` determines the actual right-hand position. This diagnostic does not
mean the controls remain disabled. No unsupported side/order override is claimed.

## Wallpaper choices

Open **Wallpaper → For this theme** and select the separately curated Wallhaven
photograph: dark [zpzqeg](https://wallhaven.cc/w/zpzqeg), light
[kxygyd](https://wallhaven.cc/w/kxygyd). The source catalogue at
`data/wallhaven-curated.json` records uploader versus unverified author, unknown
license, source links and crop/readability notes. These links are recommendations,
not package-owned remote fields or a redistribution license.

This package sets no wallpaper image, scaling mode, background colour, slideshow
or download action. Theme apply and mode changes preserve the independent
Wallpaper selection; no automatic downloads or generated artwork fallback.
Former generated artwork and packaged screenshots containing it were removed.
The palette, layout, captions and native widget definitions remain unchanged.

Native full-resolution QA replaced the dark aerial-city recommendation with a
quieter photographic Swiss valley: bright rooftop edges competed with fine grey
greeting/host labels. The dot clock and widget layout are unchanged.
