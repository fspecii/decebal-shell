# Plasma Breeze for Decebal

A port of KDE Plasma's stock **Breeze Light** and **Breeze Dark** look. One package holds both variants
(`decthemectl theme apply plasma-breeze` with the normal light/dark mode switch).

**Status: NOT READY.** Light/dark colours, metrics, fonts, icons, Kickoff's logo, the stock applet set and Plasma's panel geometry are
ported through the shared panel contract (`[bar] applets`, `[branding]`, `[taskbar]`, `min_shell_version = "0.3.0"`). The strip is verified
on a private Xvfb on all four edges at 100/125/200 % in both modes, but a full every-applet, popup and launcher acceptance does not exist yet. See
[`docs/plasma-theme-ports.md`](../../docs/plasma-theme-ports.md); do not describe this package as a complete Plasma panel.

## Upstream (all inspected from pinned local clones, 2026-10-31)

| Project | Commit | Used for |
| --- | --- | --- |
| [KDE/breeze](https://invent.kde.org/plasma/breeze) | `75561b7f32f3121603a0f2c8ea929b20a0d6c311` | `colors/BreezeLight.colors`, `BreezeDark.colors`, `kstyle/breezemetrics.h`, `kstyle/breezehelper.cpp`, `kdecoration/breezesettingsdata.kcfg`, wallpaper `Next` |
| [KDE/plasma-workspace](https://invent.kde.org/plasma/plasma-workspace) | `dec158c08fbda8f1dbfd3a86449977dde6347146` | `lookandfeel/org.kde.breeze{,dark,twilight}`, `kcms/fonts/fontssettings.kcfg`, tray/clock applets |
| [KDE/plasma-desktop](https://invent.kde.org/plasma/plasma-desktop) | `c68d33a9c7cd8e3d32c989a9eadf95e25cc6e79c` | `layout-templates/org.kde.plasma.desktop.defaultPanel`, Kickoff, task manager, pager, show-desktop |
| [KDE/libplasma](https://invent.kde.org/plasma/libplasma) | `6a9ded3bab634d4dbea6683d727d956e278c582c` | `src/desktoptheme/breeze*` panel and dialog SVGs (translucency 0.85 / 0.8) |
| [KDE/kirigami](https://invent.kde.org/frameworks/kirigami) | `1ea0abb59d20705832c121eeebd06e1954f15d0c` | units: smallSpacing 4, largeSpacing 8, cornerRadius 5, gridUnit 18 |
| [KDE/kwin](https://invent.kde.org/plasma/kwin) | `ff26dc46b3c9e5aa0c2e52cbdb7e3a35b9b4f586` | tile padding 4 |

Plasma 6.8.80 development snapshot. Nothing from these projects is executed or vendored except numeric colour and metric
values and the licence text.

## What is ported

- Every colour token from `BreezeLight.colors` / `BreezeDark.colors`: Window → background/bar/popup/dock, Button → surface,
  Tooltip → overlay, Selection → accent, WM → titlebars, Foreground\* → text/status. The border is Breeze's own
  `mix(Window, ForegroundNormal, frameContrast = 0.2)`. `tests/plasma_theme_ports.rs` recomputes all of it.
- Metrics: corner radius 5, translucent panel 0.85 and popups 0.85 (libplasma SVGs), Noto Sans 10 / Hack from Plasma's font KCM
  defaults, `breeze` caption buttons, `breeze`/`breeze-dark` icon theme with symbolic tinting.
- Panel: default panel script order Kickoff → pager → icon-only tasks → separator → tray → clock → show-desktop. Declared as the
  `launcher, workspaces, tasks, tray, volume, bluetooth, brightness, network, battery, notifications, clock, show_desktop` applets on the
  left-aligned Windows-style bottom taskbar, Kickoff's `start-here-kde-symbolic` logo, no search box, no menu bar (`[bar] enabled = "auto"`),
  no hot corners; Plasma's analog and digital desktop clocks are saved off (`widgets/`, `enabled = false`).

Tiling policy is the user's **global** behaviour (layouts, master fraction, master count, smart gaps): this package deliberately sets none of
it, so applying or switching the theme never changes custom layout proportions. Only visual gaps (8 inner / 4 outer, KWin's tile
padding of 4) and the 1 px border are the theme's. KWin floats windows by default; Decebal's global tile-first behaviour is an
intentional difference.

## Known differences

The panel is Plasma's own geometry through the typed `[taskbar]` section (44 px floating strip, 8 px gap, 28 px icons, 50 px cells,
focus/hover task frames with the strong line towards the desktop, pager cells, a square show-desktop icon, Kickoff's glyph in the text
colour), on all four edges. Minimised and attention task states and the pager's window outlines come from real window state (X11; on Wayland the window
manager reports no urgency or geometry, so the pager keeps a 16:9 shape and a centre plate). Not drawn: Kickoff's popup, the margins separator and Plasma's adaptive
opacity. Breeze's white-on-`#3daee9` selection text is 2.5:1 and is kept as upstream defines it. Breeze Twilight is not modelled.

## Licence

Colour values and layout facts derive from KDE Breeze, `LGPL-2.0-or-later` (`SPDX-License-Identifier` of the colour scheme
files); the full licence text is in `LICENSE.txt`. Original authors include Andrew Lake, Marco Martin, Nate Graham, Noah Davis,
Neal Gompa and David Redondo (copyright headers of `BreezeLight.colors`). KDE and Breeze are KDE e.V. names; no endorsement is
implied. The Breeze default wallpaper "Next" (Krystian Zajdel, CC-BY-SA-4.0) is not bundled.
