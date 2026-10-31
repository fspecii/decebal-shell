# Window decoration metadata (source-backed) and how the packages carry it

The shared package schema carries a window decoration only through `[decoration]` (`style`, `height`, `radius`, `title_align`, `buttons`;
`buttons` side and order are `Pending`: the rdwm style decides) plus the visual `[rdwm]` gaps and `border_px`. The packages use exactly
those keys. Everything below is upstream metadata recorded so that a future schema field, or a native decoration renderer, can be checked
against numbers instead of memory. Nothing here is a claim that rdwm draws it.

## KDE Breeze (breeze `75561b7f`, `kdecoration/`)

| Metadata | Value | Source | Package key |
| --- | --- | --- | --- |
| Frame radius | `Frame_FrameRadius = 2.5` small-spacing units; small spacing is 2 on Wayland, so 5 px | `breeze.h`, comment on `smallSpacing` | `decoration.radius = 5` |
| Title alignment default | `AlignCenterFullWidth` | `breezesettingsdata.kcfg` | `title_align = "center"` |
| Button size default | `ButtonDefault` = `gridUnit * 2` (tiny 1, small 1.5, large 2.5, very large 3.5 grid units) | `breezedecoration.cpp buttonSize()` | style default |
| Titlebar margins | top 2, bottom 2, side 2, button spacing 2 (small-spacing units) | `breeze.h` | style default |
| Titlebar height | `max(font height, buttonSize) + smallSpacing * (TopMargin + BottomMargin)` | `breezedecoration.cpp` (≈ line 409) | `height = 30` is rdwm's native Breeze default, **not** derived from this formula (inputs depend on font and DPI) |
| Border size default | `BorderNone` (window-specific override only) | kcfg `BorderSize` | `[rdwm] border_px = 1` is the visual hairline |
| Rounded corners / outline | `RoundedCorners` true, `OutlineEnabled` true, `OutlineCloseButton` false | kcfg | style default |
| Shadow | size default `ShadowLarge`, strength 255, colour black. Large = offset (0,12) with a 48 px / alpha 0.8 layer and a 24 px / 0.2 layer (small 16/1 + 8/0.4, medium 32/0.9 + 16/0.3, very large 64/0.7 + 32/0.1) | kcfg, `s_shadowParams` | no key: rdwm has separate window-shadow keys that a theme must not set |
| Maximised borders | `DrawBorderOnMaximizedWindows` false | kcfg | n/a |
| Title colours | WM `activeBackground` / `inactiveBackground` of the colour scheme | `BreezeLight.colors`, `BreezeDark.colors` `[WM]` | `titlebar`, `titlebar_inactive` |

## Layan (Layan-kde `a0b6a495`, `aurorae/themes/Layan/Layanrc`, `look-and-feel/*/contents/defaults`)

| Metadata | Value | Package key |
| --- | --- | --- |
| Decoration engine | Aurorae SVG theme `__aurorae__svg__Layan` (or `Layan-light`) | rdwm `adwaita` round-button style is the nearest renderer |
| Buttons | `ButtonsOnRight=IAX` (minimise, maximise, close), none on the left; round, 16×16, margins 2, spacing 12 | `buttons = "right"` (side is a hint only) |
| Title | centred, vertically centred, height 15, title edges 10/5, text shadow on (offset 1,1) | `title_align = "center"` |
| Borders | left/right 2, bottom 8, padding 10; `Shadow=true`, `Animation=0` | `border_px = 1`, visual |
| Colours | `ActiveTextColor 223,223,223`, `InactiveTextColor 200,200,200`, tab colours 78/120 greys in the Aurorae rc; the colour scheme's `[WM]` is authoritative | `titlebar`, `titlebar_inactive` |
| Corner radius | not in the rc; about 8 px read from the upstream preview | `decoration.radius = 8` |

## Gaps

- Breeze's real titlebar height depends on the user's font and DPI; the package keeps rdwm's native 30 px.
- Aurorae SVG frames and text shadows cannot be expressed; rdwm draws its own buttons. Layan's grey round buttons are approximated by
  `adwaita`. A native capture of captions was unavailable in the preview worker.
- Window shadows are an rdwm session setting, outside a theme.
