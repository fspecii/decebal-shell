# Aster

Aster combines Breeze's small circular caption controls with a macOS-like division
of work: a 28 px top information panel and a compact 44 px launch dock at the bottom.
There is no side rail or second taskbar. Ten-pixel inner gaps, 18 px outer gaps,
a one-pixel border and modest five-pixel caption corners give windows a precise
rhythm. The deep slate / porcelain pair shares a subdued sea-blue accent. It is
an original Decebal design, not a Plasma or macOS replica or endorsement.

The panel is 94% opaque, the dock 88%, and popups 98%; the effect is a restrained
frost rather than transparent labels over a photograph. Refraction and grain are
zero. The shell shadow/blur metrics are hints to existing painters, **not** a
compositor shadow profile. Real compositor shadows, backdrop blur and motion are
owned by the parent integration and cannot be judged from headless images.

Two optional existing, theme-safe widgets align to a common 32 px right inset:
`builtin:clock-card` (time/date and weather only when available) and
`builtin:now-playing-mini` (real MPRIS data, “Nothing playing” and no transport
buttons when no player is available). Their different native widths deliberately
keep the time card compact and leave media titles room. No custom providers,
commands or made-up telemetry are supplied. Apply keeps the existing custom-layout
Keep / Replace / Cancel flow; these placements are the Replace/default layout.

## Wallpaper and fonts

There is deliberately **no bundled wallpaper, generated fallback, inherited art,
or unsupported remote manifest field**. The two individually inspected Wallhaven
recommendations are in [`../../data/wallhaven-curated.json`](../../data/wallhaven-curated.json)
under `themes.aster`: dark `9mqvz1` (quiet blue breaking wave), light `wex83x`
(still water and a pale sunset). Uploader attribution is not authorship; licenses
are unknown and these full artworks are not redistributed. See the catalogue for
exact pages, image/thumbnail URLs and crop/readability notes.

Until the parent adds the catalogue download/cache/selection wiring, select the
chosen photograph in the separate Wallpaper preferences yourself. Applying Aster
has no wallpaper section and preserves an existing wallpaper path, scaling mode
and background colour. Headless package-only previews therefore label the missing
wallpaper and show the theme's plain background colour; this is not a promised
photographic default. Private QA with the selected image is documented separately.

Uses installed Inter → Adwaita Sans → Noto Sans; no font is bundled or registered.
Adwaita icons are selected if installed, falling back to hicolor. Cursor selection
is left to separate preferences. Existing terminal/GTK/editor adapters can consume
the palette where supported; no external application stylesheet, icon recolouring,
Apple assets, font installer or upstream install script is included.

## Evidence

`docs/theme-ports/aster/` records actual native dark/light package renders and the
private-photo QA path. Captions use the existing read-only native rdwm-deco helper.
The footer labels fixed sample data; dock pins are renderer samples. No live theme
was applied, and no compositor shadows are painted into the evidence or wallpaper.
