# Deepin desktop port: inventory and implementation map

Status 2026-10-31, on root base `8b3f822`. Family `deepin`, entry point <https://github.com/linuxdeepin>. Everything below was read from
shallow clones under `~/.cache/decebal-upstreams/deepin` at the revisions in [`sources.json`](sources.json); no upstream
script was run and no upstream C++/QML was copied. Package: [`themes/deepin`](../../../themes/deepin). Assets:
[`assets/desktop-ports/deepin`](../../../assets/desktop-ports/deepin) ([README](../../../assets/desktop-ports/deepin/README.md)).
Decoration spec: [DECORATION-CONTRACT.md](DECORATION-CONTRACT.md). Wallpapers:
[`wallhaven-deepin.json`](../../../assets/desktop-ports/deepin/wallpapers/wallhaven-deepin.json). Icons and cursors: [ICONS.md](ICONS.md), [DCI-CONVERSION.md](DCI-CONVERSION.md).

## 1. Which Deepin is ported

The current desktop is **DDE 25** (the `dde-shell` generation, Qt 6, X11 and the `treeland` Wayland compositor): one panel,
the dock (`dde-shell` `panels/dock`), a launcher (`dde-launchpad`), a control center (`dde-control-center`), tray plugins
(`dde-tray-loader`), appearance service (`dde-appearance`), the DTK toolkit (`dtkgui`, `dtkdeclarative`) and the
`deepin-kwin` window manager with the Chameleon decoration. Defaults (`dde-appearance` `org.deepin.dde.appearance.json`):
global theme `nirvana.light`, icon theme `nirvana`, cursor `bloom`, font Source Han Sans SC 10.5 pt, mono Noto Mono,
cursor size 24, opacity 0.4. The older V20/V23 `dde-dock` / `dde-launcher` (`dde-dock` last commit 2025-01-14) are history,
not the target.

## 2. Source inventory (what each repository gave)

| repository (revision) | read | became |
|---|---|---|
| `dde-shell` b2a4ea2 | `panels/dock/constants.h`, `dconfig/org.deepin.ds.dock.json`, `package/main.qml`, `tray/`, `taskmanager/` | dock geometry, edges, hide modes, plugin list, blur colours (below) |
| `dde-launchpad` b6a8b63 | `qml/Helper.qml`, `FullscreenFrame.qml`, `windowed/*`, `shell-launcher-applet` | fullscreen 8 x 4 grid, windowed popup 610 x 480 (`launcheritem.qml`), list row 36 |
| `dde-tray-loader` 0253c9f | `plugins/dde-dock/*` | tray applet inventory: airplane-mode, bluetooth, brightness, datetime, dnd-mode, eye-comfort-mode, keyboard-layout, media, notification, onboard, power, shutdown, sound; plus `application-tray` (StatusNotifier) |
| `dde-appearance` 7190e59 | dconfig defaults | font, icon, cursor, opacity defaults |
| `dtkgui` fa3d6fd | `dguiapplicationhelper.cpp` `standardPalette` | all colour tokens |
| `dtkdeclarative` 0159003 | `qt6/src/qml/FlowStyle.qml` | radii 8 / 12 / 14 / 18, titlebar 50, button 36, window button 50, scrollbar 4, tool button 30, icon sizes |
| `deepin-kwin` 6648082 | `src/plugins/kdecorations/deepin-chameleon` | decoration metrics (DECORATION-CONTRACT.md); nothing copied: rdwm draws its own |
| `deepin-icon-theme` 5d11ba4 | `bloom`, `bloom-dark`, prebuilt `cursors/` | `asset-packs.toml` pins (not copied; GPL-3.0-or-later, installed by the shell's asset installer) |
| `deepin-desktop-theme` a0c79e3 | `*/deepin-themes/*/index.theme`, `nirvana/` | global-theme values (ActiveColor, WindowOpacity 0.4, DockBackground) and the default icon theme nirvana, DCI-only, converted by `tools/build_nirvana.py` (ICONS.md) |
| `deepin-wallpapers` caae39a | `.reuse/dep5`, `deepin/*.jpg` | licence facts and the colour direction only (blue silk). No file copied |
| `deepin-gtk-theme`, `qt6platform-plugins`, `dde-widgets`, `dde-dock`, `dde-kwin`, `dde-wallpapers` | listed | not used in this checkpoint |
| `dde-control-center` (not cloned: 438 MB), `treeland`, `dde-file-manager`, `deepin-terminal`, `dde-session-shell` | repository metadata only | see gaps |

## 3. Measured design and where it landed

### Dock (`dde-shell` `panels/dock`)

* Size: `Dock_Size` 48 in dconfig, `DEFAULT_DOCK_SIZE` 56 in `constants.h`, range 37 to 100. Icon size = dock size x 9 / 14 (36 at 56).
  Indicator style switches to the "Efficient" look at the minimum size 37. **Edges: Top, Right, Bottom, Left**, default Bottom.
  Hide modes keep-showing / keep-hidden / smart-hide; item alignment centre / left / fashion.
* Two looks: efficient (default; a full-width strip with the blur) and fashion (`enableFashionMode`; centred floating pill,
  floating margin 8, corner radius `(dock + 2 x max(6, 0.16 x dock)) / 4`, bottom/top only).
* Material: blur with blend colour rgb(235,235,235) light / rgb(10,10,10) dark, alpha = the appearance Opacity (0.4), 1 px
  border rgba(0,0,0,0.15) light / rgba(0,0,0,alpha + 20/255) dark, shadow rgba(0,0,0,0.1) radius 40.
* Content: launcher button, multitasking-view button, task icons (centred), tray area (application tray and quick-panel
  plugins), clock in the tray, show-desktop at the end. Quick-panel plugins default (dconfig `quickPlugins`): network, bluetooth,
  wireless-casting, grand-search, eye-comfort-mode, airplane-mode, dnd-mode, screenshot, screen-record, clipboard,
  system-monitor, media, brightness, sound ...

**Mapping.** The shared Windows-style taskbar renderer (`[dock] style = "windows"`) has the same structure and all four
edges, so the package uses it: `position = "bottom"`, `align = "center"`, `size = 36`, `roundness = 0` (efficient), `[bar] enabled = "auto"`
so no second process runs. Applet set (`docs/universal-panels.md` keys): `launcher`, `workspaces` (multitasking view),
`tasks`, `search` (grand search), `tray`, `bluetooth`, `network`, `volume` (sound), `battery` (power), `notifications`,
`control_center` (the quick panel), `clock`, `power` (shutdown), `show_desktop`. `clipboard`, `agents` (system monitor), `media`,
`brightness`, `lock_keys`, `weather` and the rest remain user-selectable in Settings → Panels on every edge.
The fashion look (floating, rounded, centred) is *not* a separate package: it is the Mac dock's geometry plus app-only content
and cannot hold the tray, so it is listed as a gap, not faked.

### Launcher (`dde-launchpad`)

Fullscreen (default): search field, 8 x 4 icon grid with pages, dock stays visible. Windowed: 610 x 480 popup at the
launcher button with a sidebar, frequently used row, list rows of 36 px, search. Decebal's launcher styles (`launcher.style`
default | dux on this base; seven on the launcher worker's branch, `docs/launcher-styles.md`) have no Deepin one.
The package sets no `[launcher]` key (default), because the closest existing styles (`gnome` full-screen pages, `windows` anchored popup)
are other desktops' compositions; the `--edge/--anchor/--thickness` contract already places a launcher on every dock edge.
A Deepin style is a launcher-owner item (section 7).

### Palette and shapes (`dtkgui`, `dtkdeclarative`)

Light: Window #f8f8f8, Base #fff, text black 0.85 / 0.7 / 0.6, Highlight #0081ff, warning #ff5736, FrameBorder black 0.05.
Dark: Window #252525, Base #282828, text white 0.85 / 0.7 / 0.6, Highlight #024cca on #f1f6ff, warning #e43f2e, FrameBorder
white 0.1. The manifest flattens the alpha tokens on their window colour (documented in `theme.toml`). Deviation: the
accent is #0081ff in both modes (reason in the manifest); the default global theme's ActiveColor (#1f6ee7 light, #024cca dark)
is recorded, not applied.
Metrics: control radius 8, popup 18, floating panel 14, menu / default window 12, buttons 36, title bar 50.

### Window decoration

Chameleon: 50 px titlebar, 50 x 50 flat buttons, centred 14 px title; full spec and the rdwm requirements in
[DECORATION-CONTRACT.md](DECORATION-CONTRACT.md). rdwm's measured `deepin` / `deepin-dark` styles (candidate 952457b) are selected by mode through the shared contract, with `adwaita-light` / `adwaita` as the proven fallback.

### Icons, cursors, sounds, fonts

* Icons: Bloom (freedesktop SVG, GPL-3.0-or-later) is a pinned catalogue entry (`asset-packs.toml`, `deepin-bloom` / `deepin-bloom-dark`): the
  shell's installer fetches or imports it from the pinned upstream and verifies the git tree; **no icon file is in this repository**. The global
  theme's `nirvana` icons are DCI containers the freedesktop lookup cannot read: DCI-CONVERSION.md (maintainer recipe, nothing committed).
* Cursors: Bloom's prebuilt Xcursor files come from the same `deepin-bloom` / `deepin-bloom-dark` packs (`provides = ["icons", "cursors"]`).
* Sounds (`deepin-sound-theme`, GPL-3.0): not packaged; an event-sound theme is the notification-sound owner's business.
* Fonts: nothing bundled. Source Han Sans SC (OFL) is requested first; Noto Sans CJK SC / Noto Sans stand in.

### Widgets (`dde-widgets` 090c043, last commit 2024-09-27)

The widget host ships Calendar (a stock month grid), Notification Center and Memory Monitor. `themes/deepin/widgets/deepin-memory.toml` ports the
Memory Monitor at its small size (170 x 170): legend dots 6 px, rings 6 px wide, memory `#00C5C0`, swap `#FEDF19`, track at 0.1 of the text
colour ([dark](evidence/widget-deepin-memory-dark.png), [light](evidence/widget-deepin-memory-light.png), native `decwidgets render`, fixed sample data).
Differences: full rings instead of 270-degree arcs; the legend shows swap as a percentage (the shared data has no swap totals). The widget
is *offered*, not placed: DDE shows no desktop widgets by default and `[desktop] widgets = "none"` keeps the user's layout and the
Ask / Keep / Replace flow untouched. The Calendar widget needs a month-grid node the widget DSL does not have; the Notification Center is the
`notifications` applet (`decnotify history`).

### Wallpapers

None in the package, and no image anywhere in the repository. The fragment `assets/desktop-ports/deepin/wallpapers/wallhaven-deepin.json` follows the root catalogue's
schema and policy (SFW photographs, no digital-art / 3D / CGI / AI-related tag, no other desktop's or vendor's name, unknown author and licence kept unknown):

| mode | id | photograph | original | looks like |
|---|---|---|---|---|
| dark | [`ex5m3w`](https://wallhaven.cc/w/ex5m3w) | one lit grid window in a near-black facade at night (low-light architecture) | 6016 x 3384 JPEG, 3,744,072 bytes, sha256 `e6f7b335...06d1` | graphite: upper third rgb(27,27,27); the lit window is warm sepia |
| light | [`r2k6km`](https://wallhaven.cc/w/r2k6km) | white and pale-blue ice floes under a pale sky, Jokulsarlon, Iceland | 3840 x 2160 JPEG, 5,936,095 bytes, sha256 `dc17ed88...513f` | pale blue-white: upper third rgb(215,224,233); lower half busier and darker |

Each record carries the detail-response and thumbnail hashes, the full-resolution review and a `crop_review` of centre crops at 16:9, 3440x1440 and 1080x1920 (hashes and
a verdict each; the 9:16 slice of the dark photograph keeps only the left part of the window). Compared with Deepin's own look these are *photographic stand-ins* in the same
graphite and pale-blue colours, not matches of its renders (smoked glass, blue silk), and the dark one is warmer than Deepin's blue-rimmed glass. The first picks (`7j2wl9`, `3zg3xd`) were CGI
renders tagged `3D Abstract` / `digital art`, which the root policy rejects: they are recorded in `rejected_recommendations` (with the replacements) and kept in full under
`historical_candidates`. **Every capture in this directory was taken before these two were chosen: none of them shows either photograph, and nothing here claims how
the desktop looks with them** (the captures use a flat backdrop and a private compositor, see section 5). Deepin's own wallpapers are CC-BY-4.0 (UnionTech) and could be shipped
with attribution; the brief asked for Wallhaven photographs, so they were only a colour reference.

## 4. Integration contracts (nothing outside this namespace was edited)

**Panel/shared engine.** Uses only documented 0.3.0 keys: `[bar] enabled/style/height/applets`, `[dock] style/position/size/align/
roundness`, `[branding] logo = "icon:deepin-launcher"` (the neutral grid unless the user's icon theme has it),
`min_shell_version = "0.3.0"`. Global tiling keys (`layouts`, `mfact`, `nmaster`, `smart_gaps`) are not written; the wallpaper
keys are absent so user wallpaper choices cannot be overwritten; `[desktop] widgets = "none"` keeps the Ask / Keep / Replace flow
untouched. Validated with the panel branch's `decthemectl theme validate` (build of 8042ff7).

**Icons and cursors stay independent.** The manifest has **no** `[icons]` section and no cursor key. Measured on the panel base: `theme apply`
copies a package's `[icons]` over `decebal.json` `icons`, so a `[icons] theme = "bloom"` turned a user's `Adwaita` into `bloom` (the first draft did
exactly that). The link between the theme and its packs is `[[recommend]]` in `assets/desktop-ports/deepin/asset-packs.toml`
(`theme = "deepin"`, icons and cursors `deepin-bloom`); the installer's Icons pane offers it, `decthemectl assets use` applies it on request,
and applying or switching the theme never selects anything.

**Asset catalogue (installer owner, `docs/desktop-asset-packs.md` at 8022bd7).** `assets/desktop-ports/deepin/asset-packs.toml` is the family's
companion file: source `deepin-icon-theme` pinned to commit `5d11ba4e...` (GPL-3.0-or-later, licence files `LICENSE`,
`LICENSES/GPL-3.0-or-later.txt`, `.reuse/dep5`, GitHub archive URL, `strip_components = 1`), packs `deepin-bloom` (`bloom`, tree `9baca758...`) and
`deepin-bloom-dark` (`bloom-dark`, tree `738f4c4a...`), one `[[recommend]]`. The three licence files sit outside the pack trees and a GitHub archive has no stable hash, so `license_oids` pins each by its Git blob id (`git rev-parse 5d11ba4:<file>`: `LICENSE` `94a9ed02...`, `LICENSES/GPL-3.0-or-later.txt` `d41c0bd9...`, `.reuse/dep5` `acc35823...`, cross-checked with `git hash-object` on the clone's bytes). Validated with the installer's own `decthemectl assets catalog check`
(ok: 1 source, 2 packs, 1 recommendation) and installed from a clone with `assets install deepin-bloom --source-dir` into a private root: tree
verified, 6359 files and 7501 links installed, 22 dangling upstream links dropped with a warning, `assets verify` ok, `assets preview` draws the real
icons and cursors ([evidence/installer-bloom-preview.png](evidence/installer-bloom-preview.png)). The registration line for
`src/desktop_assets/builtin.rs` is the root's. `nirvana` is not expressible there; see DCI-CONVERSION.md for the recipe and a proposed typed
conversion for the installer owner.

**Wallpapers.** `wallpapers/wallhaven-deepin.json` follows the schema of `data/wallhaven-curated.json` (`images`, `themes.deepin`, `rejected_recommendations`); the root merges it. Proof on root `38dafed` (job `bz5wcs412`, output in [evidence/root-38dafed-curated-and-catalogue-tests.txt](evidence/root-38dafed-curated-and-catalogue-tests.txt)): with the fragment merged into a scratch copy of the root catalogue (56 -> 58 images, 29 -> 30 themes) root's own `curated_theme::catalogue_covers_shipped_designs_with_reviewed_sfw_provenance` passes, `tests/deepin_port.rs` passes 12 of 12, `deepin_decoration_modes` 1 of 1, and `desktop_assets::builtin::every_port_catalogue_in_the_repository_is_compiled_in_and_valid` passes with the registration patch. The scratch tree was removed; no root file was changed.

## 5. Evidence (all private HOME/XDG, owned private Xvfb on :700-:999, invalid session and system buses, no live config or display)

**Build under test.** Root integration `8b3f822` (universal panels, taskbar fixes and the GNOME port included) plus this branch (theme, docs, tests and
docs and tests only; no `src/`, `Makefile` or `docs/themes.md` of the root is touched). Binaries from a private target, sha256 `decthemectl b8c69d66...`,
`decdock 489025a7...`, `decbar 55047ed8...`; `Cargo.lock` is the root's. The asset installer is the separate branch `8022bd7` (not yet in root),
built in its own worktree and target for the pack checks. Nothing was installed outside private roots, nothing was pushed.

* **Tests**: `cargo test --test deepin_port` 12 of 12 on `8b3f822` (the mode-style assertions that need the engine are skipped there) and, on a scratch tree of the Lingmo engine commit `ca418f2` carrying this package, 12 of 12 plus `deepin_decoration_modes` 1 of 1 and the shared `theme::engine::shipped` 4 of 4 (palette recomputed from the pinned DTK values, panel and applet keys, no global or user-owned key
  written, the catalogue's pins and recommendation, no artwork in the asset folder, the preference record, the wallpaper fragment's provenance, the
  nirvana manifest, the stdlib self-test of the DCI reader, contrast in both modes, and the window-style pair); `cargo test --lib theme::engine::shipped`: the generic "every folder under `themes/` is a valid package whose id is its folder" check covers `themes/deepin`;
  `deepin` is deliberately *not* in that file's showcase list, whose sandbox apply cannot satisfy the mode-specific window-style contract (an apply stops when no rdwm answers), so the
  contrast checks that list would run are in `tests/deepin_port.rs` instead. All run with `flock --close` on the shared slot and a private HOME/XDG/bus environment.
* **Catalogue and installer** (`8022bd7`): `decthemectl assets catalog check` ok (1 source, 2 packs, 1 recommendation); `assets install deepin-bloom
  --source-dir <clone at the pin>` verified tree `9baca758...`; `assets verify` ok; `assets preview`: [evidence/installer-bloom-preview.png](evidence/installer-bloom-preview.png).
  The converted nirvana imports with `assets import` (1,441 files, ok). Details: ICONS.md, DCI-CONVERSION.md.
* **Native preview**: `decthemectl theme render deepin --mode dark|light` ([dark](evidence/native-dark.png), [light](evidence/native-light.png); taken on the earlier
  panel base 8042ff7 and not redone; the preview worker has its own private HOME, so it shows the neutral grid instead of the Bloom logo).
* **Live dock captures, 48 of 48** (`tools/capture_matrix.sh`): the real `decdock` on a private Xvfb with a private `picom` (xrender, no blur) over a flat
  `#6f93c4` backdrop, the Bloom pack installed by the installer as the *user's* icon choice (`icons set bloom`), dark and light x top, bottom, left, right at
  100 % on 1280x720, 125 % on 1600x900, 200 % on 2560x1440 (1280x720 logical) and short windows with every applet on (1280x480 and 800x480 at 100 %, 640x360
  logical at 200 %): [evidence/panels/](evidence/panels/). Measured from the PNGs: the taskbar is 48 / 60 / 96 px thick at 100 / 125 / 200 % on every edge.
  With the compositor the dock is the specified blend (`#ebebeb` x 0.4 over `#6f93c4` = rgb(161,182,212), exact); without one the renderer blends against
  black and the light dock turns grey ([evidence/panels/no-compositor-light-bottom-100.png](evidence/panels/no-compositor-light-bottom-100.png), older base).
* **Hover, popup and keyboard on all four edges, both modes** (`tools/capture_interaction.sh`, xdotool on the proven owned display; 8 of 8 cases):
  per case the Start button and a pinned task hovered (plate drawn), a click on the clock opens the calendar popup on the screen side of the strip (above a
  bottom dock, below a top dock, beside a left or right one), and `decdock --focus` draws the keyboard ring on Start, then moves it with Right, Right to the
  first task; Escape closes: [evidence/interaction/](evidence/interaction/) (`dark-bottom.png` ... `light-right.png`, six frames each).
* **User choices survive** (private config, panel base 8042ff7; not redone on 8b3f822): with `panel dock left`, `panel applet weather on / clock off / tasks off` and
  `icons set Adwaita`, `theme apply nord`, `theme apply deepin` and a mode change left the edge, the applet map and the icon theme untouched. The first draft with
  an `[icons]` section replaced `Adwaita` by `bloom`, hence no such section. No wallpaper, widget or tiling key is written.
* **Widget**: [dark](evidence/widget-deepin-memory-dark.png), [light](evidence/widget-deepin-memory-light.png), native `decwidgets render`, fixed sample data
  (panel base).
* Assets: the Xcursor writer experiment and cursor sheet ([evidence/cursors-bloom-48.png](evidence/cursors-bloom-48.png)) and the Chameleon glyph sheet
  ([evidence/chameleon-glyphs-light-dark.png](evidence/chameleon-glyphs-light-dark.png)) are renderings of upstream files, kept as measurement evidence.

What the captures do **not** show: tasks of real windows, workspaces, volume / network / Bluetooth / battery / brightness (no WM, hardware or bus in the
fixture, so those applets are absent, as the contract says), the notifications popup (no `decnotify`), the Control Center, the launcher, drag, touch, Wayland, or
a pointer cursor on screen. Hover is the dock's own highlight; keyboard is `decdock --focus`.

Observed with every applet on (shared taskbar, not changed here): on short horizontal screens the task icons are cut hard where the tray starts and the
terminal pin hides behind an overflow ("...") entry; on short vertical screens at 800x480 and 640x360 the task icons beyond the first pin are reduced to
slivers (a red sliver of the Chrome icon) above the overflow ("⋮"). Not a Deepin-specific defect; for the panel owner.

## 6. Remaining gaps (honest list)

1. **Window decoration**: rdwm's `deepin` / `deepin-dark` styles are a candidate (rdwm `952457b`, in root's review) and the manifest selects them by colour
   mode through the mode-specific window-style contract, falling back to `adwaita-light` / `adwaita` with a persistent warning when the running rdwm
   does not list them. No rdwm built from the candidate was run for this port, so the Chameleon look itself is not captured, only the title-bar
   numbers are cross-checked (DECORATION-CONTRACT.md).
2. Deepin launcher compositions (fullscreen paged grid with the dock visible; windowed 610 x 480) are not launcher styles; the launcher owner's call.
3. The fashion (floating) dock and the multitasking-view overview have no renderer; `workspaces` is the closest applet. The shared floating bar was tried
   for the fashion look and rejected (a full-width pill about 22 px high that ignores `[bar] height`; [evidence](evidence/rejected-fashion-floating-bar-dark-light-100.png)).
4. The global theme's own icons (nirvana, DCI) have only a maintainer recipe; a typed native conversion in the installer is requested in DCI-CONVERSION.md.
   Palette layers, other DCI states and animations are not handled (nirvana has none). Bloom (what the toolkit selects) is a normal catalogue pack.
5. Blur: alpha 0.4 over a non-blurred backdrop looks lighter than DDE; the compositor-blur work is the engine's.
6. Light-only shadow/border tokens (rgba black 0.15 vs 0.478) cannot be per-mode in the shared metrics.
7. Not exercised: real windows (task buttons, workspaces), the Control Center and launcher under this theme, notifications, volume / network popups (no service in
   the fixture), the settings gallery card, Wayland. Thickness is the taskbar's fixed 48 px; Deepin's dock is 56 (dconfig default 48), so `dock.size = 36`
   only sets the icon size.
8. `dde-control-center`, `treeland`, `dde-file-manager`, `deepin-terminal` and GTK/Qt application theming are not inspected or ported.
9. The wallpaper fragment awaits the root's merge into `data/wallhaven-curated.json`; the committed catalogue has no image and unknown licences.

## 7. Requests to other owners

* **Root**: register `src/desktop_assets/builtin.rs` for `assets/desktop-ports/deepin/asset-packs.toml` (installer branch); merge the wallpaper fragment
  `assets/desktop-ports/deepin/wallpapers/wallhaven-deepin.json` after review; apply (or amend) `root-registration.patch` (three hunks: the `deepin` line in `src/desktop_assets/builtin.rs`, the Makefile uninstall list, the `docs/themes.md` gallery entry), which this branch deliberately does not apply to root files; it was regenerated against root `38dafed` (the `CATALOGS` list there has `gnome` and `openkylin`; the patch inserts `deepin` before them).
  Measured earlier on `7680c65` with the first version of the patch (clean HOME, job `brmxs2f1m`, output kept in [evidence/root-lib-tests-with-deepin-registered.txt](evidence/root-lib-tests-with-deepin-registered.txt)): 67 of 68 `desktop_assets` and shipped-theme lib tests pass, including
  `every_port_catalogue_in_the_repository_is_compiled_in_and_valid` (the Deepin catalogue parses under the root's validator) and the four shipped-package tests. The one failure is
  `desktop_assets::pane::tests::cursor_choice_and_following_the_theme_go_through_the_pane` (pane.rs:265): that root test's fixture claims the pack id `deepin-bloom-dark`
  is *not downloadable* (it expects the "has no download" note), but once the real catalogue is compiled in, `deepin-bloom-dark` is a real pack with an `archive_url`, so the pane shows an
  Install card instead. The fixture, not the catalogue, needs another pack id (e.g. `fixture-bloom-dark`, pane.rs:260 and :265); I did not change root files and did not re-run to confirm that edit.
  Without the registration line the same run fails only `every_port_catalogue_...` (as designed: the list must name every catalogue).
* **Decorations owner / root**: merge the engine with the mode-specific contract (`ca418f2`) before this branch; then `tests/deepin_decoration_modes.rs` compiles and runs.
* **Asset-installer owner (b179eb...)**: DCI-CONVERSION.md (optional typed conversion), and the installer notes in section 4.
* **Panel owner**: the short-screen overflow observation in section 5.

## 8. Licence pins, measured with an installer binary that reads them

A catalogue copy with renamed ids (so the compiled-in older copy in that binary did not shadow it) installed from the local clone: `outside the pack folder: licences verified (blob); complete: yes`. The same copy with the first oid replaced by zeros refused to install: `licence file LICENSE: pinned blob 0000..., read 94a9ed02...` (nothing was installed). Before the pins the same install said `licences NOT pinned; complete: NO`. Test: `deepin_port::asset_catalogue_pins_the_upstream_and_recommends_without_selecting` (shape, order and the three blob ids).
