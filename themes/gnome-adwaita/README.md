# GNOME Adwaita

A native Decebal theme package that reproduces the **GNOME 51 / libadwaita 1.10** look in light and dark: the stock named
colours, the 32 px top panel, 47 px headerbars with 15 px window corners, Adwaita Sans, symbolic Adwaita icons and the
quick-settings card. Unaffiliated with the GNOME Project. The full research record, derivation tables, per-key status and framework
blockers are in [`docs/gnome-theme-ports.md`](../../docs/gnome-theme-ports.md).

## Pinned references

| Source | Revision | Used for |
|---|---|---|
| [GNOME/libadwaita](https://gitlab.gnome.org/GNOME/libadwaita) `src/stylesheet` | tag `1.10.0`, `c9ab118612eee92324143783c4641c98eaded3f0` | window/headerbar/sidebar/card/popover colours, accent, `--border-opacity`, `--dim-opacity`, `--disabled-opacity`, headerbar 47 px, `--window-radius` 15 px |
| [GNOME/gnome-shell](https://gitlab.gnome.org/GNOME/gnome-shell) `data/theme/gnome-shell-sass` | tag `51.1`, `33da06854e229c256afd5872fedd83c28cbfa176` | panel colours and 2.2em height, pill panel buttons, dash metrics, quick-settings, popover radius |

Both are LGPL-2.1-or-later. Only measured numbers and colour values are used; no stylesheet text, SVG or font is copied.

## System dependencies (not shipped)

* **Adwaita Sans / Adwaita Mono**: Arch `adwaita-fonts` (SIL OFL 1.1), the GNOME 48+ default faces. Fallbacks: Cantarell, Inter, Noto Sans.
* **Adwaita icon theme**: `adwaita-icon-theme` (LGPL-3.0-or-later / CC-BY-SA-3.0), `hicolor` as the fallback.

## Wallpaper candidates (for the central curated mapping)

The package sets no wallpaper image, scaling mode or background colour and downloads nothing, because the separate Wallpaper settings own
the image; this does not disable the user's follow-the-theme or match-wallpaper-colours choices. Wallhaven provides no per-image licence, so
no image is redistributed (licence unknown). Both candidates passed an independent acceptance (fresh API and original SHA/dimensions match,
SFW, no AI-art tag, 16:9, 3440×1440 and portrait crops viewed). They are **not merged yet**: entry in `data/wallhaven-curated.json` is what makes
"follow theme" select them, and the current loader ignores `recommended_mode`, so centre-fill is what actually happens. Provenance and hashes:
`tests/fixtures/gnome-theme-ports/wallhaven-fragment.json`.

| Mode | Wallhaven | Notes |
|---|---|---|
| dark | [yx3w57](https://wallhaven.cc/w/yx3w57) | Tasman Lake **sunrise** (not dusk), 3840×2160, blue/violet/pink sky. Uploader jrmnt; the linked SuperRare listing credits Paul Wilson (15-shot panorama claim, 2023-08-12; not independently certified). |
| light | [v9kw2m](https://wallhaven.cc/w/v9kw2m) | Picos de Europa lake and cloud reflection, 5280×2970. Uploader Petunai42; the source Reddit post is marked OC by u/Pchabs and its image is byte-identical. |

## Status

Requires Decebal 0.3.0 (`min_shell_version`): the GNOME quick-settings Control Center look it selects (`[control_center] style = "gnome"`) ships
with that release, and an older build refuses the package for that reason only. Not complete. Remaining gates: universal four-edge panel and applet schema, an Overview/launcher style, GNOME-shaped quick settings and
native fidelity checks (see the blockers in the docs). Do not treat the package as a finished port.

## Native behaviour declared

Light and dark variants of one package; mode switches select the variant.
Panel `[bar] style = "dux"`, top, 32 px, `left = launcherButton, workspaceSwitcher`, `center = clock, notificationButton`,
`right = network, audio, battery, controlCenterButton`; the `adwaita` rdwm titlebar (47 px, radius 15, centred title); no window border and no
layout policy of its own (Decebal's global window behaviour is untouched); GNOME's hot corner (top-left → overview); a persistent dock is **disabled** because stock GNOME only shows the
dash inside the Overview, but its Dash metrics (48 px icons, 12 px padding, round capsule) are declared so enabling the dock yields the
Dash look; no desktop widgets (GNOME has none).
