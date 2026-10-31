# GNOME Catppuccin Dock

A distinct, researched, popular **GNOME rice**, ported natively: **Catppuccin Mocha / Latte** (mauve accent) with the extension trio
most rices are built on, a translucent **Blur my Shell** top panel, a **Dash to Dock** bottom dock and **Just Perfection** panel
trimming, plus **Vitals**-style CPU/memory readouts, Inter, a Nerd-Font JetBrains Mono and Papirus icons. Unaffiliated with GNOME or
Catppuccin. The research record, per-key status and framework blockers are in
[`docs/gnome-theme-ports.md`](../../docs/gnome-theme-ports.md).

## Why this rice

The Catppuccin palette repository has ~19.9k stars (MIT, 2026-10-31 API read), Dash to Dock ~4.4k, Blur my Shell ~2.4k, WhiteSur ~9.4k
(already covered by the macOS themes), Nordic ~2.7k and Colloid ~1.5k (both GPL, and Nord already exists). Mocha + Dash to Dock + Blur my
Shell + Just Perfection is the recurring combination in public GNOME dotfiles and is MIT-compatible at the palette level, so it was
chosen as the second, visually distinct port.

## Pinned references (nothing copied)

| Source | Revision | Used for |
|---|---|---|
| [catppuccin/palette](https://github.com/catppuccin/palette) (MIT) | `07d02aa110ef9eb7e7427afca5c73ba9cf7f8ebd`, palette 1.8.0 | Mocha and Latte hex values |
| [ubuntu/dash-to-dock](https://github.com/ubuntu/dash-to-dock) (GPL-2.0+) | `2dfae3474d6984b566ef0c3dab1524a675bbba48` | bottom position, `dash-max-icon-size` 48, `background-opacity` 0.8, dash radius 16+12 |
| [aunetx/blur-my-shell](https://github.com/aunetx/blur-my-shell) (GPL-3.0) | `f13bdcf36be5e1d8b62203c77face3303f0860af` | default blur brightness 0.6, sigma 30 |
| [jrahmatzadeh/just-perfection](https://github.com/jrahmatzadeh/just-perfection) (GPL-3.0) | `a46f18957422210411f5675e462c73d47e79266e` | panel trimming (Activities hidden, Shell default panel size kept) |
| libadwaita 1.10.0 / GNOME Shell 51.1 | `c9ab118…` / `33da068…` | window radius 15, headerbar 47, panel 32 |

## System dependencies (not shipped)

Inter (SIL OFL), a JetBrains Mono Nerd Font, Papirus (`Papirus-Dark`, GPL-3.0; falls back to Adwaita when absent), Adwaita Sans/Mono as font fallbacks.

## Wallpaper candidates (for the central curated mapping)

No wallpaper, scaling or colour is set and nothing is downloaded: the separate Wallpaper settings own the image, and the user's
follow-the-theme / match-wallpaper-colours choices are not disabled. Wallhaven publishes no per-image licence (unknown); nothing is
redistributed. The current curated loader ignores `recommended_mode`, so centre-fill is what happens. Evidence and hashes:
`tests/fixtures/gnome-theme-ports/wallhaven-fragment.json`.

| Mode | Wallhaven | Status |
|---|---|---|
| light | [d8prrm](https://wallhaven.cc/w/d8prrm) | Mount Fuji over a calm lake at pastel sunset, 5120×2880. Independently accepted for 16:9, 3440×1440 and portrait; not merged yet. Uploader Pc7; no source link. |
| dark | **pending** | [833qd2](https://wallhaven.cc/w/833qd2) (Guangzhou skyline) is **rejected** as a universal default: a centre-fill crop to 3440×1440 cuts the upper sections of the two tallest towers. A bounded search (4 queries, 8 originals) found no replacement that passes every crop, so no dark photo is mapped and none is substituted. |

## Status

Requires Decebal 0.3.0 (`min_shell_version`): the GNOME quick-settings Control Center look it selects (`[control_center] style = "gnome"`) ships
with that release, and an older build refuses the package for that reason only. Not complete. Remaining gates: universal four-edge panel and applet schema, an Overview/launcher style, GNOME-shaped quick settings and
native fidelity checks (see the blockers in the docs). Do not treat the package as a finished port.

## Native behaviour declared

Light = Latte, dark = Mocha. Panel `[bar] style = "dux"`, top, 32 px, translucent (`bar_alpha` 0.6, blur): `left = workspaceSwitcher,
focusedWindow`, `center = clock, notificationButton`, `right = systemTray, cpuUsage, memUsage, network, audio, battery,
controlCenterButton`. Dock enabled, bottom, floating capsule, 48 px icons, alpha 0.8, no magnification. `adwaita` titlebar (47 px,
radius 15), 8 px gaps and 2 px accent-coloured focus borders (window arrangement stays the user's global policy: no layout, mfact or nmaster is declared), hot corner top-left → overview, no desktop widgets.
