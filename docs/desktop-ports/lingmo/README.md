# Lingmo desktop port — inventory and implementation mapping

Status: **NOT READY.** Research and mapping, the `themes/lingmo` package, asset recipes, wallpaper records, tests and native captures exist; see
[native-evidence.md](native-evidence.md) for exactly what was rendered and what was not. Nothing here is called a replica. Every number below was read from the pinned source named in [sources.json](sources.json)
(machine-readable pins, licence evidence and tree hashes). Local clones: `~/.cache/decebal-upstreams/lingmo`.

## 1. Which Lingmo is "current"

`LingmoOS/lingmo-meta` (2026-10-28) `lingmo-shell-meta` depends on `lingmo-compositor`, `lingmo-core`, `lingmo-sdk`,
`lingmo-icon-theme`, `lingmo-wallpapers`, `lingmo-framework`, `lingmo-filemanager`, `lingmo-settings` and friends. That is a
**new Wayland desktop**: one process (`lingmo-compositor`, "ported from Chromium Exo and Ash") owns window management,
server-side decoration, the status bar, the dock, the launcher, the control centre, notifications and the OSD, drawn with
Skia by `lingmo-sdk` (the "Lingmo Toolkit"). Its code says repeatedly that its numbers were carried over from the previous
Cutefish/FishUI + KWin generation ("kwin-plugins decoration", "FishUI's window frame").

The older repositories (`LingmoOS/LingmoOS` monorepo, `dock`, `lingmo-statusbar`, `lingmo-launcher`, `lingmo-shell`,
`lingmo-appearance` = a Darkly KDE style fork, `lingmo-kwin`) and the whole `velora-*` family (a Deepin DDE-derived tree) are
**not** the design source; they are listed as `reference_only_previous_generation` / `not_used`.

## 2. What the real desktop is (measured)

All values are logical pixels unless noted. File references are in `lingmo-compositor` `1cdaf0a` unless noted.

### Palette (`lingmo-sdk` `752d2b2`, `src/ui/theme.cpp`, `toolkit_colors.cpp`)

| token | light | dark |
|---|---|---|
| background | `#F5F6FA` | `#1C1C1D` |
| secondBackground (cards, fields) | `#FFFFFF` | `#2C2C2D` |
| alternateBackground (buttons) | `#F2F4F5` | `#3C3C3D` |
| text | `#323238` | `#FFFFFF` |
| disabledText | `#64646E` | `#888888` |
| highlight (default accent) | `#3385FF` | `#3385FF` |
| destructive | `#D92D20` | `#FF453A` |
| window titlebar | `#FFFFFF` | `#2C2C2D` |
| caption active / inactive | `#383838` / `#7F7F7F` | `#CACBCE` / `#707070` |

Accent palette (selectable in the real Appearance service, index order): blue `#3385FF`, red `#FF5C6D`, green `#35BF56`,
purple `#8266FF`, pink `#CA64AC`, orange `#FEA042`, grey `#4F596B`. Shell glass surfaces: status panel surface
`#FAFAFC` / `#242529`; control-centre tiles are white 80% (light) / 10% (dark) over the glass.
Radii (`ui::Theme`): small 8, medium 10, big 12, huge/window 14; tile and panel radius 16; OSD 18.
Spacing 6 / 9 / 12. Font: the toolkit asks fontconfig for `sans-serif`; Lingmo packages Noto fonts
(`lingmo-settings-default` depends on `fonts-noto`) and its font collection carries Inter and Noto Sans CJK.

### Window decoration (`src/decoration/decoration_theme.{h,cpp}`, `window_visual_style.cpp`, `caption_button.cpp`, `decoration.cpp`)

* titlebar height **36**, frame radius **14** (all four corners, continuous "squircle" corners by default), border width 0
  (the outline overlays client pixels), resize margin 5, corner hit length 16.
* three caption buttons, **right aligned, order minimise, maximise, close from left to right (close outermost)**;
  hit/hover square **32**, spacing **1**, right margin **4**, glyph drawn on a 24 grid inside it.
* hover and press are round washes: diameter `32 - 2*round(32*0.1)` = **26**. Light: hover `#E0E0E1`, pressed `#CDCDCE`,
  glyph `#282828`; dark: hover `#38383A`, pressed `#2A2A2B`, glyph `#FFFFFF`. The close button has no red state (colour is the
  same wash as the others) — a real difference from Adwaita/Breeze-style close buttons.
* caption centred in the **window** (not between the buttons) and pushed aside only when it would touch them; margin 20;
  size = session font size (9 pt = 12 px default). The unfocused caption is dimmed, not hidden.
* outline 1 physical px, light `rgba(0,0,0,.05)` active and inactive, dark `rgba(0,0,0,.7)`; an inner highlight `rgba(255,255,255,.15)`
  (light) / `.20` (dark). Shell surfaces (menus/panels): outline `.14` light / `.7` dark. Dock outline `.10` / `.30`, dock highlight `.20`.
* shadow, three lobes (wide ambient, directional, 1.5 px contact): active `lobes(24, 9, 14, 1.0)`, inactive strength 0.55,
  popup `lobes(8,4,3,.7)`, menu/tooltip `lobes(14,6,8,.85)`, dock `lobes(10,4,3,.25)` with **no** contact line. Lobe opacities
  are `0.22 / 0.15 / 0.16` times the strength.
* modal dim 15% (light) / 25% (dark); snap preview = accent at 25% with a dark 8% hairline and a light inner line.

### Status bar (`src/shell/statusbar/*`, `menubar/*`)

* a **32 px** strip along the top of the primary output, reserved from the work area. Background = the user's Gaussian blur
  from `shell.conf [Statusbar]`: sigma **20** (max 64), opacity **0.55**; contents are light or dark **by the wallpaper behind
  the bar**, not by the system mode (text `#FFFFFF` on a dark backdrop, black 85% otherwise; menu text pure white/black).
* items sit on rounded plates (radius 8) of white 20% (light contents) / black 10% (dark contents); when a plate shows was not verified; icon 16, font 10 pt, edge margin 12,
  item gap 12, plate gap 6.
* left to right: **menu bar** (first slot: the system logo entry `systemlogo`, then the focused application's DBus/AppMenu
  menus, with a global-menu registrar `com.canonical.AppMenu.Registrar`), recording/agent stop buttons, application-drawn
  items, **tray** (StatusNotifier), **volume**, **network**, **battery**, **control centre** icon (with a camera/microphone/
  system-audio privacy dot), **clock** (weekday, month, day, time; clicking opens the notification centre).
  There is no workspace switcher, weather, search, CPU/RAM or media widget on the real bar.

### Dock (`src/shell/dock/*`)

* floating glass panel, **bottom by default; also left and right; never top**. Icon **54**, gap **16**, length padding **16**,
  outer padding **16**, running dot 4 with 8 gap and 4 edge padding; thickness 86 at 54. Everything scales linearly with the
  icon size (`DockMetrics::forIconSize`). Corner radius = thickness × 28/86 (**28 px at the default size**).
* settings defaults: magnification **on, factor 1.5** (reach 2.5 slots, follow τ 45 ms, return τ 90 ms), bounce on launch
  (amplitude 0.18, 300 ms × 3), glass tint 0.5, edge margin 10, minimised-window area on, visibility always/hide/intelligent.
* contents: a fixed **"Launcher" item (`kLauncherId`) that toggles the Launchpad**, pinned applications (default pins:
  file manager, terminal, chromium, settings, calculator, deb installer, screenshot, video player), running applications,
  minimised windows, a divider (white/black), and a trash item (`trash_monitor`). Order of the trash/launcher items was not
  verified.
* glass: blur sigma 0.6 – 8 with tint, displacement 36 (clear) → 0, saturation 1.4 → 1.0, rim 1; tint alpha dark .12–.50, light .16–.53.

### Launcher (`launcher_metrics.cpp`)

Full-screen Launchpad: search field at the top (width max(240, min(22% w, 42% h)), height 2.3 × text size), paged icon grid,
page dots at the bottom. Columns 4–9 by grid width (<720 → 4 … ≥2800 → 9), rows 2–6 by height, icon = 48% of the cell width
(32–192), selection plate 1.44 × icon. Its backdrop is the wallpaper shrunk to an eighth and blurred by three box passes (`launcher_wallpaper.h`). Controls: arrows move, edges turn the page.

### Control centre / OSD / notifications

Glass popover under the bar (margin 6, radius 16, blur sigma 20, saturation 2.4, tint `0.13/0.13/0.14 @ .86` dark,
`0.94/0.94/0.95 @ .86` light); tiles raised off it. OSD: 280 × 54 at the top centre, radius 18, hides after 1.5 s. Notification
banner surface `#4D4D4D @ .65` dark / `#F0F0F0 @ .82` light.

## 3. Mapping onto the Decebal shared framework

Contract: `docs/universal-panels.md` (shared panel contract, `feat/claude-universal-panel-applets`); the validated engine build is
`~/.cache/decebal-plasma-ports/target-shared` (pinned at `71e3434`, see the Plasma port document).

| Lingmo | Decebal key | Value | Honest gap |
|---|---|---|---|
| status bar | `[bar] style = "flat"`, `position = "top"`, `height = 32` | menu-bar renderer | the engine's flat bar uses the theme's `bar` colour/alpha; wallpaper-luminance-driven light/dark contents and the blur settings (sigma 20, 0.55) are not modelled by the engine |
| applets | `[bar] applets = [app_menu, tray, volume, network, battery, control_center, clock]` | the real bar's set; every other applet stays one toggle away | the real bar has no `notifications`/`workspaces`/`weather`/`search` item; they are **off by default**, not removed |
| logo | `[branding] logo = "icon:systemlogo"`, `logo_tint = false` | the multicolour blue Lingmo mark from the icon pack | without the Lingmo icon pack installed the neutral `builtin:grid` is drawn (by contract), never Apple |
| desktop label | `[branding] desktop_label = "Desktop"` | upstream has no fixed string: with no application focused the bar shows the **desktop process's own exported menu** (`MenuBarModel::updateDesktopImport`) | the engine default would be "Finder"; "Desktop" is a neutral stand-in, not an upstream string |
| dock | `[dock] style = "mac"`, `position = "bottom"`, `size = 54`, `magnification = 1.5`, `roundness` | `dock_padding = 16` | 4 sides: Lingmo has no top dock but the shared dock supports it |
| dock radius | `dock_roundness = 0.65` | keeps upstream's radius/thickness ratio (28/86); engine radius = min(w,h)/2 × roundness | measured in a real capture the engine's panel is 73 px tall at size 54 (upstream 86 includes the running-dot area), so the radius is ~24 px not 28 |
| dock gap 16 | not expressible | the engine's `app_spacing` is a user dock setting (default 6), not a theme key | requirement for the shared dock owner: `[dock] spacing` |
| dock launcher | Launchpad item first in the dock | the mac dock holds applications only (contract) | the Launchpad is reachable through the `launcher` applet on the bar or the dock's own launchpad icon |
| decoration | `[decoration] style_light = "lingmo"`, `style_dark = "lingmo-dark"`, fallbacks `adwaita-light` / `adwaita`, `title_align = "center"`; no height or radius | the native measured port pair when the RUNNING rdwm lists it, else the proven same-mode fallback with a persistent warning (`decoration-style-modes.md`) | applied: the stable native Lingmo geometry (30 px header, 11 px radius). The new compositor's own decoration measures 36 px / 14 px (reference only, `native/reference-numbers.json`, not applied) |
| colours | `[colors.light]` / `[colors.dark]` from section 2 | | the status bar's dynamic ink is replaced by the mode's `bar_text` |
| icons | none declared on purpose (an `[icons]` section would replace the user's chosen pack when the theme is applied) | `Lingmo` is recommended through `asset-packs.toml` `[[recommend]]` (the shared contract applies a recommendation only when the pack is installed and the user has not chosen explicitly); the manifest itself never selects it | until the user installs the pack the bar logo is the neutral grid |
| cursors | `docs/desktop-ports/lingmo/reference-recipes.toml` (not in the contract catalogue: no upstream licence file) | `lingmo-light` / `lingmo-dark` | the manifest has no cursor key; selection stays in Settings (section 5) |
| wallpaper | none in the package | Wallhaven records (separate) | |

Tiling policy (layouts, mfact, nmaster, smart_gaps) is **not** set by the package. Only visual gaps and the border are.

## 4. Window style: native pair, with the original contract kept for the record

The decorations owner has since added the native `lingmo` / `lingmo-dark` styles; the package uses them through the mode-specific keys in
[decoration-style-modes.md](decoration-style-modes.md). What follows is the measurement a native style was asked to meet (upstream `1cdaf0a`), kept so the numbers can be compared
with the stable native Lingmo record that the package applies (30 px header, 11 px radius, 30 px plates). The 36 / 14 numbers belong to the new compositor, are a reference only and are not applied.

A faithful Lingmo style needs, beyond `adwaita`:

1. metrics: titlebar 36, corner radius 14, **continuous** (superellipse-like) corners, button hit square 32, spacing 1, right margin 4;
2. button order from the outside in: close, maximise, minimise; a 26 px round wash on hover/press only; **no coloured close state**;
3. glyphs on a 24 grid inside the square: close = two 10×10 diagonal strokes (7..17), minimise = a 10×1 bar at y=12, maximise =
   a 10×10 rounded square with 2.5 radius, restore = two overlapped squares (do not copy upstream's SVG path data: draw the
   measured geometry independently);
4. colours from section 2 (hover/pressed/glyph per mode), caption centred in the window with a 20 px margin from the buttons;
5. outline 1 physical px (`rgba(0,0,0,.05)` light, `.7` dark) plus an inner 1 px highlight (`.15` / `.20`);
6. three-lobe shadow with the active/inactive strengths in section 2.

## 5. Contract for the shared asset installer owner

`assets/desktop-ports/lingmo/asset-packs.toml` is the catalogue for the shared installer (contract: `docs/desktop-asset-packs.md`, commit `76b9285`): one icon
pack, with a pinned GitHub archive (sha256 `c5cbd4fa…4043`) and tree OID, plus a `[[recommend]]` for the `lingmo` theme.
`reference-recipes.toml` (this directory) keeps both packs, with pinned repo, commit, tree hash, licence evidence and the exact
files to take. Rules the installer must follow: fetch by commit over HTTPS into an empty temporary directory, verify the git tree
hash, never run upstream `cmake`/install scripts; install the single `Lingmo` icon theme exactly as `reference-recipes.toml` restates
upstream's script (`src/` then `links/`, `@2x`/`@3x` symlinks; upstream never installs `dark/`). **`links/` is 14,137 regular files
(git mode 100644, not symlinks) that each hold a relative target path as text; 14,104 resolve and 33 dangle.** The installer of commit `69d6107` (asset-pack branch) supports exactly this through
declared, tree-pinned `[[pack.text_links]]` and `[[pack.dir_alias]]` tables, so the catalogue now declares upstream's own build step as data: `links/` copied over `src/` (15 names collide, hence `replace = true`)
and the 16 `<ctx>@2x` / `<ctx>@3x` folder links. Nothing is run. Real result with that installer (private root): 14,137 aliases applied, 96 of 96 icon directories (no longer DEGRADED), 14,118 links
installed, `verify: ok`, 16 of 16 sample icons resolved, `complete: yes`. Still disclosed: the 33 aliases that dangle upstream are dropped, and 2 alias names without an icon extension
(`actions/24/media-album-cover`, `apps/scalable/cinnamon-virtual-keyboard`) are left out by the installer's allowlist. **Dependency:** a validator without that installer rejects these keys, so this catalogue
must land after it. `docs/desktop-ports/lingmo/reference-tools/verify-icon-recipe.py` is a verification script, not an installer. Requires `breeze` icons for inheritance; install the
cursor themes under `~/.local/share/icons/<name>/` and set them only when the user chooses (`decthemectl system set cursor-theme NAME`; icons: `decthemectl icons set Lingmo`). Both verification scripts in `docs/desktop-ports/lingmo/reference-tools/` pass against the pinned upstream commits. Icons and cursors stay
independently selectable in Settings even when a theme suggests them. The cursor pack's licence is **not granted in the
repository** (see `sources.json`), so it is user-initiated and shows that uncertainty.

## 6. Files and what each is

| Path | Purpose |
|---|---|
| `themes/lingmo/` | the package: `theme.toml` (light and dark, validates under the 0.3.0 shared contract), `README.md`, `NOTICE.md`; no art, no icons, no `[icons]` section |
| `docs/desktop-ports/lingmo/sources.json` | pinned upstream repositories, commits, tree hashes and licence evidence |
| `assets/desktop-ports/lingmo/asset-packs.toml` | the icon pack in the shared installer's catalogue format (data only) |
| `docs/desktop-ports/lingmo/reference-recipes.toml` | icon and cursor recipes with the install semantics, link findings and licence evidence (data only) |
| `docs/desktop-ports/lingmo/wallhaven-lingmo-records.json` | merge-ready fragment in the integration catalogue's schema (`data/wallhaven-curated.json`): two disjoint photographs, each with full-resolution review, three hashed crops, parent verification against a fresh API response and a fresh download, and readability metrics; authorship and licence unknown |
| `docs/desktop-ports/lingmo/native-evidence.md`, `native/` | tools, captures, hashes and the measured gaps |
| `assets/desktop-ports/lingmo/tools/` | scripts (edge matrix, real-GUI capture, record builder); no images |
| `tests/lingmo_desktop_port.rs` | offline tests: colours recomputed from upstream, dock-tint formula, metrics, panel contract, no tiling keys, no shipped art, licence honesty, apply keeps user choices and icon pack, wallpaper records |

## 7. Requirements this port places on shared work (do not edit another owner's files)

* **Panel/dock owner:** scale the mac dock with `DECEBAL_SCALE` (it is 82 × 401 px at every scale); a theme key for the mac dock's icon gap (16 upstream); per-surface glass parameters (dock lens 36, popup 20, bar blur-only sigma 20); a way for the bar's ink to follow the wallpaper; a real AppMenu consumer for `app_menu`. (The flat bar's declared `[bar] height` is now consumed: see native-evidence.md.)
  bar's ink to follow the wallpaper; a real AppMenu consumer for `app_menu`.
* **Decorations owner:** section 4. A native `lingmo` style with continuous corners, the 26 px wash, close-outermost order and no coloured close state.
* **Asset installer owner:** `asset-packs.toml` is validated by the shared catalogue validator of `69d6107` (`decthemectl assets catalog check`: ok) and installs completely with it (see section 5). It needs that installer: root should land `69d6107` before this catalogue, or the catalogue test of the current validator rejects the `text_links` / `dir_alias` keys. Cursors stay out of the catalogue (the repository has no licence file); never run upstream scripts; keep icon and cursor choices separate from theme apply.
  never bundle the cursors (licence not granted upstream), never run upstream scripts, keep icon and cursor choices separate from theme apply.
* **Integration owner:** merge `wallhaven-lingmo-records.json` (`images{}` plus a `themes{}` entry light `mldor9` / dark `l8pe5r`) into `data/wallhaven-curated.json` if accepted.

## 8. Not done yet / risks

* **This is not a full replica.** Lingmo's full-screen paged Launchpad, its control centre, the wallpaper-adaptive bar ink and the blurred glass bar/dock are not reproduced (the engine's launcher is a floating card, its control centre has its own tiles, glass parameters are global, and no GLX or hardware compositor run exists). The package matches the measured palette, the 32 px bar, the 54 px dock with 1.5x magnification, the mode-specific window styles and the panel applet set; nothing more is claimed.

* No claim of visual parity. Scale: the flat bar is evidenced at 100/125/200% (`aa0580a`); the mac dock does not scale in that build, so no dock claim. Compositor: a private picom run exists, no GLX/hardware run. No decoration or Launchpad capture; the control centre is the engine's, not Lingmo's.
* The real status bar's wallpaper-adaptive contents, per-window continuous corners, dock glass refraction and Launchpad paging are engine capabilities, not theme data.
* The compositor's and SDK's licences are internally inconsistent (BSD-3-Clause file versus GPL-3.0-or-later README/SPDX);
  only measured numbers are used and no upstream source or SVG path data is copied.
