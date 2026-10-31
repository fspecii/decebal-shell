# openKylin port: integration contract for root and the other owners

Everything below is what the port **needs from** shared code; nothing in `src/`, rdwm or another owner's files was edited.

## 1. Registration (root)

* **Five** packages: `themes/openkylin` (v11), `openkylin-islands`, `openkylin-light-seeking`, `openkylin-heyin`, `openkylin-classic`. Each is `theme.toml` + `widgets/` (two small
  UKUI time-widget definitions) + README + LICENSE.md + ATTRIBUTION.md, `min_shell_version = "0.3.0"`. They need an engine that includes (a) the 0.3.0 panel framework
  (`[bar] applets`, `[branding]`), (b) the mode-aware window-style contract (`decoration.style_light/...`, lingmo lane, `decoration_compat.rs`) and (c) the generic `[taskbar]` keys
  (root `905c059`, `src/panel/plasma_style.rs`).
* **Registration:** see `registration-notes.md` (no patch is shipped: an earlier one had stale counts and a stub that listed only one family's styles). Root registers manually with its own multi-family schema proof.
* The only widget the packages carry is the UKUI time widget, as a **saved but switched-off** placement (`enabled = false`; UKUI has no default desktop widget), so applying a package
  never turns a widget on, and widget Ask/Keep/Replace is the engine's own: a package with no *active* widget hides, but keeps, the user's saved widgets exactly as every other widgetless
  theme does (`--keep-widgets` keeps them visible). They set **no** `wallpaper`, `icons`, cursor, `desktop.widgets`, or global tiling keys (`rdwm.layouts/mfact/nmaster/smart_gaps`);
  `tests/openkylin_port.py` and `tests/e2e/openkylin-preserve.py` assert this.

## 2. Shared asset installer (icons and cursors)

`assets/desktop-ports/openkylin/asset-packs.toml` is this port's companion catalogue in the **shared** format (`docs/desktop-asset-packs.md` of the asset-pack
lane). It replaces the lane-private recipe file, fetcher and cursor builder this port first shipped (all removed: they were not safe production code).

* 2 `[[source]]` (openkylin-theme @ `3c78d71`, dmz-cursor-theme @ `54eb2bd`, `archive_url` = the GitHub archive of that commit, `strip_components = 1`),
  6 `[[pack]]`: `openkylin-ukui-default`, `-classical`, `-fashion` (icons), `openkylin-dark-sense`, `-blue-crystal` (cursors with 57 `[[pack.xcursor]]` tables
  each) and `openkylin-dmz-white` (prebuilt Xcursor), and 5 `[[recommend]]` (one per package: v11/islands/light-seeking → default + dark-sense, heyin → fashion
  + dmz-white, classic → classical + dark-sense). `tree` is `git rev-parse <commit>:<path>`, equal to the trees verified in `sources.json`.
* `licence`: GPL-3.0-only for openkylin-theme (`debian/copyright`: "either version 3"), CC-BY-SA-3.0 for DMZ; `license_files = ["debian/copyright"]`.
  The icon index says "from moka": that provenance is **unresolved** (not researched, not assumed) and is recorded as such in `asset-packs.toml`, `sources.json` (`unresolved_provenance`) and the pack descriptions; GPL-3 is only what `debian/copyright` declares. `license_oids` pin the exact `debian/copyright` blobs (openkylin-theme `0558d3e…`, dmz-cursor-theme `84660fb…`).
* Cursor hotspot tables were read **as text** from the pinned `cursors.sh` (`tools/cursor_table.py`, strict regexes; nothing is run) and cross-checked
  against the asset lane's `xcursorgen2toml.py` (same 57 cursors; `hand1` is an alias of `hand2` upstream). 25 CSS cursor-spec names (`default`, `pointer`, `text`,
  `not-allowed`, `col-resize`, ...) are added as `aliases`: additive, never shadowing an upstream cursor. `DMZ-White` is prebuilt, so it gets none: it lacks
  the CSS names (gap for the installer: no alias hook for prebuilt cursor packs).
* Generated from the local clones by `tools/make_asset_catalog.py` (read-only `git rev-parse`), validated by `decthemectl assets catalog check` (the shared
  validator) and by `tests/openkylin_port.py` (trees equal `sources.json`, 57 tables each, hotspots inside the image, every package recommended).
* **Installer evidence** (private root, cleared environment, the asset lane's `decthemectl`, sha256 `ca4646c2e5dc…`): all six packs installed from the GitHub
  archives (137 MB + 0.9 MB) with `--archive … --strip 1`; `tree` verified `yes`, `commit` verified `yes` for every pack; `verify` reports all six `ok`;
  dark-sense = 57 built cursors + 61 links (118 names); `preview` resolved every sample cursor from the pack itself. Findings: (a) licence files lie outside the pack path: without a pin the installer reported "NOT pinned; complete: NO"; with
  `license_oids` (the `debian/copyright` blobs) it reports "licences verified (blob); complete: yes" (re-run for DMZ-White); no `archive_sha256` is pinned because a GitHub archive is not byte-stable, the tree id still pins every byte of the pack; (b) `ukui-icon-theme-default` lists 128 icon directories but 56 are empty upstream (kept as `.gitxxx` placeholders), a warning; (c)
  Classical/Fashion inherit `ukui-icon-theme-default` and every pack inherits `gnome`: installing only one pack falls back to hicolor/Adwaita for missing names (the
  catalogue has no `requires`; Settings should offer the default pack with them).
* **Selection stays independent**: the catalogue only *recommends*; `decthemectl assets use` / the Icons pane / a hand edit make the choice explicit and theme apply
  never changes it again. `tests/e2e/openkylin-preserve.py` still asserts that applying a package writes no icon/cursor key.
* The evidence helpers left in this lane (`tools/xcursor_read.py`, `tools/cursor_sheet.py`) are read-only previews of *installed* files and say so; there is no
  fetch, extract, build, link or install code in this namespace (a test asserts that).
* Root registration: list the family in `src/desktop_assets/builtin.rs` (one `include_str!` line for `assets/desktop-ports/openkylin/asset-packs.toml`).

## 3. Wallpapers (root)

`assets/desktop-ports/openkylin/wallpapers.json` is a **drop-in fragment** of `data/wallhaven-curated.json` (identical record schema: `images` with thumbnail/full-resolution hashes, AI review,
provenance limits; `themes` with placement and crop policy). `tests/curated_theme.rs` requires every shipped non-legacy theme to have a catalogue entry and every image to be used and distinct,
so registering the packages needs this merge: `python3 -I assets/desktop-ports/openkylin/tools/merge_catalogue.py data/wallhaven-curated.json OUT` adds the 5 themes and 10 images (refuses a clash;
a lane test verifies the merged result keeps both invariants and the HeYin light image was swapped when the root catalogue grew and collided). Ten images: SFW/general, ≥ 2560×1440, native 16:9,
sha256/bytes/dimensions/tags/uploader recorded, authorship and licence **null/unknown**, nothing downloaded by apply. The full-resolution review files live outside the repository
(`~/.cache/decebal-upstreams/openkylin/_wallhaven`).

## 4. Window decoration (decorations owner; contract adopted)

Source of truth: `openkylin/kwin` @ nile `01bf914`, `src/plugins/kdecorations/ukui` (38 px header, 30 px buttons, 4 px margin/spacing, radius 12 from the global theme; Classic 0).
The packages carry the **mode-aware pair** with fallbacks (keys defined by the lingmo lane's generic engine contract, `docs/desktop-ports/lingmo/decoration-style-modes.md`):
`style = "adwaita"`, `style_light = "openkylin"`, `style_dark = "openkylin-dark"`, `fallback_style_light = "adwaita-light"`, `fallback_style_dark = "adwaita"`; no `height`
(native 38); `radius = 0` only in `openkylin-classic`. No theme id appears in the engine. Proven against real new and old rdwm: `native/decoration-proof.json`. These keys exist only in
an engine that includes that contract (root must merge it before or with these packages: the 0.3.0 panel-branch binaries reject them as unknown keys, which is why `min_shell_version` is 0.3.0).

## 5. Renderer/panel findings (measured natively; owner: panel framework)

From the private-Xvfb captures (`native/native-checks.json`, real `decdock`/`decbar` 0.3.0 panel build):

1. **Taskbar thickness (resolved with root `905c059`).** The generic typed `[taskbar]` keys (`thickness`, `icon_size`, `cell_width`, `gap`, `inset`; `src/panel/plasma_style.rs`) now carry UKUI's classic
   panel: `thickness = 48`, `icon_size = 32`, `cell_width = 48`, `gap = 0`, `inset = 4`. Measured natively on a merged build: the strip is **48 / 60 / 96 px** at 100 / 125 / 200 % on every edge
   (asserted in all 84 taskbar cases of `native/native-checks.json`; it was a fixed 44 px before). The icon size is declared, not pixel-measured. Plate corner is `theme radius / 2` clamped to 2..8 px, so
   v11's 12 gives UKUI's 6 px `kRadiusNormal`, while Classic (radius 0) still gets 2 px (UKUI: 0): a one-line clamp question for the taskbar owner. No theme id is used anywhere.
2. **Dux lists reject registry keys.** `bar.left/center/right` are validated against the legacy `DUX_BAR_ITEMS` ids only, so the island
   package expresses its layout as `bar.applets` (registry keys) plus legacy-id lists. UKUI's island panel is three
   islands (data = start, task view, IDM; app = tasks, search; settings = tray, calendar) **centred as one content-sized cluster**;
   Dux spreads start/centre/end across the band, and tasks join the start group. Need: accept registry keys in the lists and an
   option to centre the cluster.
3. **No task-view / IDM applet** in the registry. Task view is the multitask overview (`ukui-kwin-effects` windowsview);
   IDM is Kylin-specific. Left out, not faked.
4. **Search** on the UKUI panel is a button/box that opens `ukui-search`; the Windows-10-look taskbar draws a 236 px box, close but wider.
5. **Plate radius:** `Metrics::themed` clamps the plate corner to `radius/2` in 2..8 px, so Classic (radius 0) still gets 2 px.
6. (Earlier, unconfirmed observation: a 44 px taskbar at 200 % when `apply` had run without `DECEBAL_SCALE`. Not reproduced since the generic `[taskbar]` keys; every case now sets the scale for every process and the thickness is asserted.)
7. Hardware applets (battery, bluetooth, volume) are correctly absent on the fixture (no devices); the island pills for them are
   drawn empty in the Dux renderer (cosmetic: an empty rounded pill is shown for an applet that has nothing to show).

8. **Desktop widgets are physical pixels.** `decwidgets` ignored `DECEBAL_SCALE` (the 448 × 162 time widget stays 448 × 162 at 200 %), unlike
   the taskbar. Also, a package widget with `enabled = false` is saved with `source`/`from_theme` but a hand-written item without `source`
   cannot be found (`widget not found`), so the packages ship the UKUI time widget as a **saved, switched-off** placement (UKUI has no
   default desktop widget); enabling it in Settings → Widgets works. The host clears the root background when it starts (a real desktop's
   wallpaper host repaints it), so the harness repaints the private backdrop.

## 5b. Next phase (not claimed): launcher, Control Center, island cluster

* **Island cluster:** ask the panel owner for a generic Dux option to centre the three groups as one content-sized cluster (and to accept registry keys in `bar.left/center/right`). Until then the
  island package is documented as an approximation.
* **Launcher:** `ukui-menu` is empty on the mirror, so there is no source to measure; the Decebal launcher keeps its own design with the package's palette and logo. No replica is claimed.
* **Control Center / sidebar:** `ukui-sidebar` is a 400 px full-height panel (notification list 374 px items); the package selects the theme-radius `fluent` flyout. A proper look needs real
  geometry (full-height side panel) and interaction work, planned as a later phase together with the launcher; nothing in this lane claims it.

## 6. Not done / out of scope

* UKUI start menu (`ukui-menu` is empty on the GitHub mirror), greeter (`ukui-greeter` empty), IDM, task-view overview.
* Fonts are the system's `Noto Sans CJK SC` / `Noto Sans`; none bundled.
* Wayland and hardware compositors were not exercised (X11, picom xrender, private Xvfb only).
