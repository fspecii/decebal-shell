# Attribution: openKylin (UKUI islands)

This package contains `theme.toml` (colour values, metrics and panel choices), two small widget definitions, this note, the licence and **one upstream image**:
`icons/kylin-startmenu.png` (128 x 128), the UKUI start button's artwork (`kylin-startmenu`, the icon the `ukui-menu-starter` widget draws), copied unchanged from
`icons/ukui-icon-theme-default/128x128/apps/kylin-startmenu.png` of <https://github.com/openkylin/openkylin-theme> at commit `3c78d714aa8fac723c9a5360bd54484789a647ab`
(git blob `adbe8bd46bcb314dd8eaff1e1ec198409b27b856`, verified by `tests/openkylin_port.py`). Declared licence of that repository: GPL-3 (`debian/copyright`); the icon index says part of the artwork is "from moka":
that provenance is **unresolved** (not researched, not assumed). The openKylin mark is also a brand of its owners: it is shown only as the identity of the replicated desktop.
It is bundled so the start button is the real mark without an icon pack; the pack itself is never bundled. No other upstream code, icon, cursor, wallpaper, font or asset is copied.

## Where the design comes from

* UKUI global theme `v11` from <https://github.com/openkylin/ukui-globaltheme>, commit
  `83cb8ff8f113e4002681401ac9da97997f440968` (branch `openkylin/huanghe`, 2025-04-08): `theme.conf` and the light/dark CSS
  token tables. The declared licence of that repository is GPL-3.0+ (`debian/copyright`). The colour *values* and
  radii are design facts measured from those tables; the mapping tool and this manifest are independent MIT work, no GPL
  implementation code is used.
* Panel geometry and widget order: <https://github.com/openkylin/ukui-panel>, commit
  `3429618a48b7049e87b1a1857599534cc0ae76ab` (GPL-3.0+ / Expat). Measured, not copied.
* Desktop time widget (`widgets/ukui-time-*.toml`): <https://github.com/openkylin/ukui-system-appwidget> (GPL, `time.qml`), commit
  `059a328736edd8556bd4b1d6b3f21b9394dcdda9`: sizes and layout measured; the files here are independent MIT definitions.
* Window decoration measurements: <https://github.com/openkylin/kwin> (`src/plugins/kdecorations/ukui`, LGPL-3), commit
  `01bf91445b61470fce0c7ac6b0f47daa2f8dc9d0`; the rdwm styles themselves come from the decorations owner.

Exact pins, declared licences and verified tree object ids: `assets/desktop-ports/openkylin/sources.json` and
`asset-packs.toml` (the shared asset-pack catalogue) in the Decebal repository.

## Suggested but separate

* Icon pack `ukui-icon-theme-default` and cursor theme `dark-sense` are the upstream suggestion of this global theme. They are **installed by the shared decebal asset installer from the pinned
  upstream commit** (`decthemectl assets install`, tree-verified) (GPL-3 for the icons; `dark-sense` GPL-3; `DMZ-White` CC-BY-SA-3.0, Copyright 2007-2010
  Novell, Inc.) and are never bundled. They stay independently selectable in Settings.
* Wallpapers: photographs recommended in `assets/desktop-ports/openkylin/wallpapers.json` (Wallhaven). Authorship and licence of
  those photographs are **unknown**; nothing is redistributed. This package sets no wallpaper.

"openKylin", "UKUI" and "Kylin" belong to their respective owners. This package is an independent adaptation and does not
imply endorsement.
