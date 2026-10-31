# KDE Plasma theme ports — 2026-10-31

**Status: NOT READY.** Both packages now declare their panels through the committed shared contract (`[bar] position`/`applets`,
`[dock]`, `[branding]`, `min_shell_version = "0.3.0"`) and validate and render natively against it, but neither may be called ready:
the shared renderers are still being finished, no complete native four-edge, every-applet and launcher-fidelity acceptance exists, and
a genuine global application menu (Layan) is not implemented. Authoritative requirements: `docs/universal-panel-requirements.md`
(integration checkout) and the contract `docs/universal-panels.md`.

## State on root `7272017` (branch `feat/claude-plasma-packages-727`, 2026-10-31)

The packages were re-based onto root `7272017` (generic source is root's: `plasma_style.rs`, the tracker, the pager, the typed `[taskbar]` renderer). Only the
packages, these docs, `tests/plasma_theme_ports.rs`, my tools and the package native harnesses are mine. Both packages validate with the strict validator
and declare `[launcher] style = "kde"` (Kickoff: the anchored popup that opens from the launcher towards the screen).

**Native acceptance, `tests/e2e/plasma-packages-native.py`** (private Xvfb on a reserved high display from `tests/e2e/private_x11.py`, both listening
transports verified as the exact child's, a from-scratch environment, the pinned rdwm `ab1b588` `468de612…` and rdwmctl `8db572d` `2d3b846f…`, root's
real Qt menu exporter, debug binaries of `7272017`; every report records the binary and harness sha256):

| Gate | Result | Evidence |
| --- | --- | --- |
| Both packages, 4 edges x 100/125/200 % x dark/light (48 entries): strut = work area, Kickoff glyph differs from the grid / Windows / Apple controls, no search box (Breeze) / one search icon (Layan), every registry applet alone / all on / all off (Breeze), clock popup on the desktop side, `--focus` ring, Layan bar launcher | 45 ok, 3 failed: Kickoff's window mapped before its card was painted (the screenshot after the click was pixel-identical to the one before). The harness now polls up to 8 s. Layan dark top 200 % re-run: ok. The two Breeze entries (light left 200 %, light right 100 %) passed in the anchor gate below. The full 48 were **not** re-run end to end with the fixed harness | `native/727/matrix-7272017{.log,-report.json}`, `layan-top-200-rerun-report.json` |
| Start-click anchor, root fix `fbddaf87f10b404b6574ff916bba767acbaa42f2`, immutable `decdock` `ec0106d5…0557dd3`, `--require-anchor` mandatory, Breeze, 4 edges x 100/125/200 % x dark/light | **24 / 24 ok**: Kickoff opens on the desktop side of the strip, inside the output, 1 px from the strip, centred on Start or clamped to the output margin (card at 8 / 10 / 16 px from the screen edge at 100 / 125 / 200 %). The same package with the `7272017` `decdock`: bottom card 915 px off Start, top 420 px from the strip, left / right 595 px from the strip | `native/727/anchor-gate-fbddaf8{.log,-report.json}`, `anchored-*-kickoff-fbddaf8.png`, `finding-*` |
| Layan `app_menu` on, real exporter | with no window the bar says `Desktop Window Help` (neutral label plus the bar's own generic menus, never File / Edit / View); with root's real Qt window focused it shows `Qt-fixture File Edit Window Help`, the window's own exported titles. Top and left at 100 % and top at 200 % were captured | `native/727/layan-top-100-app-menu-*.png` |

**Remaining blockers (the packages stay NOT READY; the human gate in `tests/plasma_theme_ports.rs` is untouched):**

1. `launcher` and `app_menu` together draw the logo twice in the flat bar (R9), so Layan keeps `app_menu` off by default (the user can turn it on and it is live).
2. Layan's real Plasma layout (Kickoff, appmenu, spacer, tray, split clock, KRunner icon) is approximated: the bar also shows its own generic `Window` / `Help` menus beside the exported titles, there is no panel spacer, and `window_title` stands in when `app_menu` is off. KRunner is not implemented: the search applet opens `decspot`.
3. Kickoff itself: only the Applications tab, a Leave button that opens `decsession`, favourites as a grid (`docs/launcher-styles.md`, "Fidelity gaps"). Not compared against a running Plasma.
4. Window decoration: rdwm has no separate button-side key, so `decoration.buttons` is accepted but not applied; Aurorae is an approximation (`decoration-metadata.md`), no replica is claimed.
5. Applets that need system data (network, volume, battery, bluetooth, brightness) are checked as effective settings only in a private world; the strip's drawing is proven for launcher, tasks, clock, show desktop and workspaces.
6. Optional assets: the Breeze / Layan cursor catalogue validates (`decthemectl assets catalog check`: 2 sources, 3 packs) but no install was run here; there is no Tela icon pack in the catalogue and nothing was fetched or verified over the network, so no icon-pack rights are claimed.
7. X11 only; Wayland is untested. The launcher contract test assertion added in `f18d95a` was not run (heavy slot-2 work is held by request).

## Current state: merged on root `8b3f822`, with the Plasma taskbar style wired into the real renderer

This branch now contains the root's universal-panel 0.3, GNOME and review-fix integration (`8b3f822`, merge commit `94fc7b0`; no root file
was rewritten) and, on top, the engine requests that the root approved and asked to be wired, not just specified
(`docs/plasma-theme-ports/engine-requests.md`, commit `21f0793`):

- `src/panel/plasma_style.rs`: the typed `[taskbar]` section and the pure Plasma formulas. Theme-neutral: no theme id is tested anywhere, a
  package without `[taskbar]` keeps the Windows 11 and Windows 10 numbers exactly, and any later port can reuse the keys.
- The real taskbar model (`taskbar::Metrics::styled`, layout, pager width), painter (frame indicator, floating plate, desktop icon cell,
  pager cells, Start tint), dock-app strip geometry (floating gap through surface, strut, input region, popups, hit areas and Control Center
  placement), the schema, `needs_framework` (a package with `[taskbar]` requires 0.3.0) and the native preview read it.
- `themes/plasma-breeze` declares Plasma's numbers: thickness 44, icon 28, cell 50, gap 0, inset 0, floating gap 8, frame indicator, desktop
  icon, pager cells. `themes/plasma-layan` has no `[taskbar]`; its flat top bar and Mac dock are untouched.
- Tiling policy, global panel preferences, icons, cursors and widgets are untouched by a theme change (existing tests, plus the apply-plan test).

**Native proof (private Xvfb on a reserved high display, owned in both socket namespaces, nothing at `:0`):** `tests/e2e/plasma-panel-native.py`
applies the package with the user's edge, runs the real `decdock` under a private XRender compositor and measures the root window. All 24
combinations (4 edges × 100/125/200 % × dark/light) pass: the strip is exactly the declared box in physical pixels (thickness `round(44·s)`,
floating gap `round(8·s)`, both ends `round(8·s)`; for example bottom 100 % `(8, 1028, 1912, 1072)`, right 200 % `(1816, 16, 1904, 1064)`), the
published strut is exactly thickness + gap (52, 65, 104), the first and last cells (Start, show-desktop icon) carry ink, hovering the Start
cell tints exactly that 50 px cell and a point in the floating gap does not react. Evidence: `docs/plasma-theme-ports/native/` (24 captures and
`report.json` with the binary sha256s: `decdock` `8a10c026…`, `decthemectl` `a89ff33c…`). Native previews of all four edges and both modes:
`docs/plasma-theme-ports/plasma-breeze-*.png`. What the native run does **not** prove: task states (no window manager and no windows),
popups, the workspace pager with real workspaces, keyboard navigation.

**Verified here, with caveats:** `cargo test --lib -- taskbar panel:: dock_app:: theme::engine controlcenter::settings` passes 187 tests; two
unrelated root tests fail on the merged tree and are not touched by this branch: `theme::engine::apply::tests::a_side_taskbar_theme_does_not_inherit_an_old_top_position`
(its manifest appends a second `[dock]` table to a manifest that already has one: "duplicate key") and
`theme::engine::cli::tests::the_whole_workflow_from_new_to_apply_and_undo` (the validate output starts with a `no author` warning, not `ok:`).
`tests/plasma_theme_ports.rs` passes 13/13 with the four-edge gate un-ignored. The root dock-ink change (`Theme::dock_foreground`,
`Colors::from_theme` dock inks, `IconCache::get_for_surface`) is merged into this branch (`d8d03e1`, then `905c059`) with the `warn` field kept.

The window tracker now carries real per-window state (minimised, attention, X11 geometry in root coordinates and the desktop size); the taskbar
paints minimised and attention tasks and the pager's window outlines on the real desktop shape (see `engine-requests.md`, "Real window manager
proof": 24 / 24 native cases under the immutable rdwm on a private Xvfb, evidence in `docs/plasma-theme-ports/native/wm/`). Other ports can reuse the
generic `[taskbar]` hooks: `taskbar-hooks-for-other-ports.md`.

Still **NOT READY**: no every-applet, full keyboard and launcher acceptance exists; the global application menu needs the bridge owner; Wayland
sessions give no window geometry or urgency, so there the pager keeps its 16:9 shape and a centre plate.

**Shared source used, exactly:** commit `71e343413fdafcc6fde638d701b63c183512f1c8` ("Stage the 0.3.0 feature batch and pin the panel
framework to it", branch `feat/claude-universal-panel-applets`), read with `git show`/`git archive` from the committed tree only and
built from that archive in `~/.cache/decebal-plasma-ports/shared-71e3434` (private target dir). The universal-panel worktree's
uncommitted work was not read or used. (That statement was true before the merge: the branch now carries the contract engine, and the tests still detect an engine without it.) Version 0.3.0 was assigned by the root owner; no version is bumped here.

**Also validated against** `b28cca5fb2e81b84d2cfcd65e9fa978391041e50` ("Draw a neutral logo when a theme's own logo cannot be had", same branch, committed tree only, archive in `~/.cache/decebal-plasma-ports/shared-b28cca5`): both packages validate with it, the previews render, and `tests/plasma_theme_ports.rs` passes 9/9 in an overlay on that archive. The manifests needed no change.

**Later committed head `8042ff7ffcdb0574c4bfa32f447700788dd01f18`** ("Fix the Dux default spin and four panel review findings", archive overlay, clean-env wrapper): `tests/plasma_theme_ports.rs` passes 10/10 with 1 ignored; the ignored side-strip blocker test still fails there (bottom taskbar about 69 px thick, no full-height side strips).

| Package | Looks like | Panel declared | Licence |
| --- | --- | --- | --- |
| `themes/plasma-breeze` | Plasma 6 Breeze Light + Breeze Dark (one package, both variants) | `[bar] enabled = "auto"`, `position = "bottom"`, 12 stock applets; Windows-style taskbar `[dock]` bottom, left aligned; Kickoff logo | LGPL-2.0-or-later |
| `themes/plasma-layan` | Layan KDE (vinceliuice), Layan Dark + Layan Light | flat 32 px top bar with 9 applets (no `app_menu`, one logo) + companion bottom Mac dock; Kickoff logo; neutral "Desktop" label | GPL-3.0-only |

**Tiling policy is global, not the theme's.** Neither manifest sets `layouts`, `mfact`, `nmaster` or `smart_gaps`, so applying or switching
a theme never overwrites a user's custom layout proportions (a test applies each package over a config with `mfact = 0.73`,
`nmaster = 2`, `smart_gaps = true` and custom layouts and checks they survive byte for byte). Only visual gaps (`gap_inner = 8`,
`gap_outer = 4`, from KWin's tile padding of 4) and `border_px = 1` are the theme's. KWin floats by default; Decebal's global
tile-first behaviour (new windows start in the first tile, free drag with Shift for snap zones; `docs/getting-started.md`) is an
intentional difference. Two earlier revisions overrode this policy (floating-first, then tile-first); both are gone.

Owned files: `themes/plasma-*`, this document, `docs/plasma-theme-ports/`, `assets/desktop-ports/plasma/asset-packs.toml`,
`tests/plasma_theme_ports.rs`, `tools/plasma-*` (edge matrix, wallpaper review, clean-run wrapper, icon recipe, asset receipts). No engine, Settings, shared catalogue, builtin registry or other package was touched.

## Assets, saved-off widgets, decoration metadata (additions)

- **Cursor packs and icon recipe:** `assets/desktop-ports/plasma/asset-packs.toml` pins three cursor packs for the shared asset-pack contract
  (`docs/desktop-asset-packs.md`, asset-owner commit `76b9285`); the Breeze icon themes are built natively into a private prefix because
  they need a generation step. Commands, tree OIDs, archive hashes and byte-comparison against the installed package:
  [`plasma-theme-ports/asset-recipes.md`](plasma-theme-ports/asset-recipes.md), receipts in `plasma-theme-ports/asset-receipts.json`.
  The contract's installer was not committed, so receipts come from an emulation and are labelled as such.
- **Saved-off widgets:** both packages carry Plasma's own desktop clocks (`widgets/plasma-analog-clock.toml`, 15×15 grid units, no card,
  no numerals, second hand off; `widgets/plasma-digital-clock.toml`, 10×4 grid units, time over date) as `[[widgets]]` with
  `enabled = false`, because stock Breeze and Layan desktops place none. They can be enabled in Settings and change nothing otherwise.
  Native render evidence (`decwidgets render`, shared `b28cca5` build, fixed sample data, private config): `plasma-theme-ports/saved-off-widgets-render.png`
  (Breeze dark analog and digital, Layan light analog and digital).
  `[desktop] widgets = "none"` cannot coexist with them (the validator refuses), so it was dropped; with no enabled instance the desktop
  stays empty.
- **Decoration metadata:** [`plasma-theme-ports/decoration-metadata.md`](plasma-theme-ports/decoration-metadata.md) records the source values
  (Breeze kcfg and metrics, Layan's Aurorae rc) and which package key carries each.
- **Numeric references:** `tests/plasma_theme_ports.rs::plasma_numeric_references_hold_for_the_packages_and_the_pinned_sources` pins panel
  thickness (46 at grid unit 18, 44 at 17), the 8 px floating gap, panel inner padding, the icon size (**28 px in the 44 px Breeze panel**,
  measured in upstream's preview and equal to 44 − 2×8; an earlier revision declared 32), panel fill alpha .85, corner radius 5, pager
  aspect-ratio formula and spacing, task indicator opacities, and the widget sizes; with `PLASMA_REFS` or `~/.cache/decebal-plasma-refs`
  present it re-reads each number from the pinned source files.

## Primary sources (pinned, read locally; clones live in `~/.cache/decebal-plasma-refs`, outside the checkout)

| Repo | Commit | Date |
| --- | --- | --- |
| `plasma/breeze` | `75561b7f32f3121603a0f2c8ea929b20a0d6c311` | 2026-10-24 (6.8.80) |
| `plasma/plasma-workspace` | `dec158c08fbda8f1dbfd3a86449977dde6347146` | 2026-10-30 |
| `plasma/plasma-desktop` | `c68d33a9c7cd8e3d32c989a9eadf95e25cc6e79c` | 2026-10-30 |
| `plasma/libplasma` | `6a9ded3bab634d4dbea6683d727d956e278c582c` | 2026-10-30 |
| `plasma/kwin` | `ff26dc46b3c9e5aa0c2e52cbdb7e3a35b9b4f586` | 2026-10-31 |
| `frameworks/kirigami` | `1ea0abb59d20705832c121eeebd06e1954f15d0c` | 2026-10-30 |
| `frameworks/breeze-icons` | `e3c7154b4f243413b0cb9028484977f3c37de15c` | 2026-10-27 (cloned, icons not re-themed here) |
| `vinceliuice/Layan-kde` | `a0b6a4956022276aee33309b2e07d1d0ef3db30c` | 2025-11-27, 540 stars, GPL-3.0 per GitHub API |

Why Layan: popular (540 stars, maintained), a genuine Plasma global theme (colour schemes, Plasma theme, Aurorae, Kvantum,
look-and-feel layout) with a layout unlike Breeze (thin top panel + global menu), licence permits redistribution of derived
values. Rejected: `EliverLara/Sweet-kde` (255 stars, **no licence**), WhiteSur/Fluent (macOS/Windows looks already exist
here), `EliverLara/Sweet` (GTK, no Plasma content). Catppuccin/kde (MIT) remains a possible colour-only follow-up.

## What was learned and applied

Breeze (all from the pinned sources):

- Colours: `colors/BreezeLight.colors`, `BreezeDark.colors`. Panel/popup/dock = Window; selection `#3daee9`.
  Frame colour = `mix(Window, ForegroundNormal, KColorScheme::frameContrast() = 0.2)` (`kstyle/breezehelper.cpp`).
- Geometry: `Frame_FrameRadius = 5`, Kirigami `cornerRadius 5`, `smallSpacing 4`, `largeSpacing 8`, `gridUnit 18`.
- Panel: libplasma `desktoptheme/breeze/translucent/widgets/panel-background.svg` fill 0.85, margins 4 px; dialogs 0.85;
  floating panels use an 8 px gap (`views/Panel.qml` falls back to 8 when the theme has no `floating-*` prefix, as Breeze has none).
  Height: default panel script uses `2*ceil(gridUnit*2.5/2)`, 46 at gridUnit 18; the upstream preview image shows 44. Not set
  in the manifests because the Windows-style bar derives its own height.
- Layout script `layout-templates/org.kde.plasma.desktop.defaultPanel`: kickoff, pager, icontasks, marginsseparator, (kimpanel for
  some locales), systemtray, digitalclock, showdesktop. Breeze Light/Dark/Twilight share it.
- Fonts `kcms/fonts/fontssettings.kcfg`: Noto Sans 10, Hack 10. Decoration `kdecoration/breezesettingsdata.kcfg`: centred
  full-width caption, no extra border, rounded corners. KWin tile padding 4.
- Icons: `breeze` / `breeze-dark` themes (installed here). Monochrome tinting stands in for KIconLoader recolouring.
- Not modelled: adaptive panel opacity (opaque when a window touches), blur strength, Twilight (dark panel + light windows,
  `lookandfeel/org.kde.breezetwilight`).

Layan (Layan-kde at the pinned commit):

- Colours `color-schemes/Layan.colors`, `LayanLight.colors` (Window `#31313a` / `#f5f5f5`, Selection `#5657f5`). Plasma theme
  translucency: panel 0.6, dialog 0.8 (`plasma/desktoptheme/common/translucent`).
- Layout script: top panel, `height 1.78` grid units (about 32 px), `maximumLength = minimumLength = 75.9` grid units (about 1366 px: the full width of the screen it was exported on), applets kickoff, appmenu, panelspacer, systemtray, splitdigitalclock, milou (KRunner search).
- `defaults`: Aurorae decoration, `ButtonsOnRight=IAX`, cursors `Layan-white-cursors`, icons `Tela` (dark) / `Tela-circle` (light),
  widget style `kvantum-dark` / `kvantum`.
- `fullscreenpreview.jpg` (viewed): menu-style top panel with global menu left and tray/clock/search right, left Kickoff-like
  dashboard, round grey caption buttons, and a bottom dock that is **not** in the layout script (Latte-style companion).
- Corner radius ~8 px is read from the preview, not from source. Kvantum SVG radii were not parsed.

## Applet inventory against the committed shared registry

The registry keys of `docs/universal-panels.md` at 71e3434: `launcher, app_menu, workspaces, window_title, tasks, layout, media, clock,
weather, tray, clipboard, cpu, memory, notifications, stay_awake, agents, lock_keys, volume, network, bluetooth, battery, brightness,
search, dashboard, control_center, power, show_desktop`. The table maps each to Plasma and gives the stock default that the manifests
declare. Plasma applets from repositories that were not pinned (plasma-pa, plasma-nm, bluedevil, powerdevil, milou) are marked "not
inspected".

| Key | Plasma equivalent (source) | Breeze default | Layan default (preview) |
| --- | --- | --- | --- |
| `launcher` | `org.kde.plasma.kickoff` (`plasma-desktop/applets/kickoff`) | on | on |
| `app_menu` | `org.kde.plasma.appmenu` (`plasma-workspace/applets/appmenu`) | off | **off** (needs a genuine exporter service; see gaps) |
| `workspaces` | `org.kde.plasma.pager` (`applets/pager`) | on | off |
| `window_title` | none stock (`window-list` is a button list) | off | **on**, a labelled stand-in for the missing application menu: the focused window's real title and menu |
| `tasks` | `org.kde.plasma.icontasks` / `taskmanager` (`applets/taskmanager`) | on | off on the bar; the companion dock carries tasks |
| `layout` | none (KWin tiling has no panel applet; `keyboardlayout` is a different thing) | off | off |
| `media` | `org.kde.plasma.mediacontroller` (`applets/mediacontroller`) | off (hidden tray entry) | off |
| `clock` | `org.kde.plasma.digitalclock` (`applets/digital-clock`) | on | on (`splitdigitalclock` is third party) |
| `weather` | none stock in Plasma 6 | off | off |
| `tray` | `org.kde.plasma.systemtray` | on | on |
| `clipboard` | `org.kde.plasma.clipboard` | off (hidden tray entry) | on (preview) |
| `cpu`, `memory` | `org.kde.plasma.systemmonitor` sensors | off | off |
| `notifications` | `org.kde.plasma.notifications` | on (tray) | on (preview) |
| `stay_awake` | none stock panel applet | off | off |
| `agents` | none | off | off |
| `lock_keys` | `org.kde.plasma.keyboardindicator` | off | off |
| `volume`, `network`, `bluetooth`, `brightness`, `battery` | plasma-pa, plasma-nm, bluedevil, powerdevil: not inspected; stock tray entries | volume/network/bluetooth/battery on; brightness on only with a backlight | volume, network, battery on |
| `search` | milou/KRunner: not inspected | off (Breeze panel has no search box) | on |
| `dashboard`, `control_center` | none stock | off | off |
| `power` | `org.kde.plasma.lock_logout` (`applets/lock_logout`) | off | off |
| `show_desktop` | `org.kde.plasma.showdesktop` (`applets/showdesktop`) | on | off |

Every key must remain individually switchable for both themes whatever the default; defaults are only the stock look.

Declared in the manifests (see `[bar] applets`; `applets` is the full default set, so unlisted keys start off and the user can turn each
on in Settings → Panels):

```toml
# plasma-breeze: [bar] enabled = "auto" (hidden while the taskbar is up), position = "bottom"; [dock] windows, bottom, left aligned
applets = ["launcher","workspaces","tasks","tray","volume","bluetooth","brightness","network","battery","notifications","clock","show_desktop"]
# plasma-layan:  [bar] flat, top, height 32; [dock] mac, bottom
applets = ["launcher","window_title","tray","network","volume","clipboard","notifications","clock","search"]
# both:          [branding] logo = "icon:start-here-kde-symbolic"  logo_tint = true  desktop_label = "Desktop"
```

Removed foreign defaults: for Breeze the taskbar's search box, layout, weather and Control Center button (the contract's taskbar default
set) are not declared; for Layan workspace dots, layout, weather and Control Center are not. `launcher_label` is omitted because Kickoff's
`menuLabel` default is empty. `desktop_label = "Desktop"` is deliberately neutral: it is only what a menu bar names when no window is
focused, so it never says "Finder" and never labels the panel's application-menu applet as a global menu (Plasma's own placeholder text,
"Global Menu", would claim a feature Decebal does not have). Applet availability still follows the system (volume needs an output,
brightness a backlight, and so on); the preview worker has none of that hardware, so those applets are absent from the native
previews on purpose, not hidden by the packages.

## Plasma behaviour on every edge (what each edge must keep)

Read from the pinned QML. Controls stay upright on left and right panels:

- Task manager: `qml/Task.qml`, `LayoutMetrics.js`: vertical form factor stacks icons in one column, margins swap axis; `tasks.svg`
  ships `normal-*`, `north-*`, `east-*`, `west-*` indicator frame sets so the running/focus indicator follows the panel edge.
- Digital clock: `DigitalClock.qml` state `verticalPanel` stacks time over date, fits width, no rotation, no one-line mode.
- System tray: `ExpanderArrow.qml`, `CompactApplet.qml`: arrow flips; expander opens upward by default on vertical panels.
- Pager: `pager/qml/main.qml`: aspect-ratio-derived min/max swap with orientation, fills the panel width when vertical.
- Show desktop, Kickoff (`kickoff/main.qml`: icon only when vertical, no label), panelspacer, marginsseparator: orientation aware.
- Popups (group dialogs, context menus, tray expander) anchor away from the edge in the source (`GroupDialog.qml`, `ContextMenu.qml`).

The four-edge contract for these themes therefore requires: thickness = the same value on all edges (width on vertical panels),
floating 8 px gap (Breeze) on the screen side, translucent fill and corner radius identical on every edge, clock/tasks/tray in a
column without rotated text, accent task indicator on the screen-facing side, popups opening inward, work area reduced by
thickness (+ margin), and the Layan top-bar look (global menu, spacer, tray, clock, search) preserved as a vertical strip with
upright glyphs only. Each edge needs native evidence for light and dark.

## Remaining genuine gaps (verified at shared commit 71e3434; items 3 and 4 are resolved on the merged branch, see the section above)

Concrete requests for the panel owner (task indicator per edge, panel metrics, pager, side strips, Start tint) with source evidence are in
[`plasma-theme-ports/engine-requests.md`](plasma-theme-ports/engine-requests.md).

Applet icon names to match in the icon pack integration: [`plasma-theme-ports/icon-map.md`](plasma-theme-ports/icon-map.md).
Branding requirements with upstream source evidence, current behaviour and acceptance criteria:
[`plasma-theme-ports/branding-contract.md`](plasma-theme-ports/branding-contract.md).

Resolved by the contract (seen in native previews): all four edges render distinctly for both packages; the launcher glyph is Kickoff's
icon, not the Windows squares or Apple mark; the taskbar draws no Search box for Breeze; the menu bar says "Desktop", not "Finder".

**Logo fallback (contract at b28cca5).** Only an *absent* `[branding] logo` keeps a style's own glyph. Both packages set an explicit
`icon:start-here-kde-symbolic`, so wherever the user's icon theme lacks that name (Breeze not installed; Layan also tries Tela, then its
`breeze` fallback) every renderer draws the neutral `builtin:grid`, never the Windows squares or Apple, and `desktop_label` stays;
`launcher_label` is intentionally unset. Both packages therefore depend on the Breeze icon theme being installed for the real Kickoff
glyph; the packages do not bundle icons. Evidence (`plasma-theme-ports/logo-fallback-b28cca5.png`: real Kickoff glyph vs fallback, Breeze taskbar and Layan bar; private `HOME`, `decthemectl theme render`, b28cca5 binary): a private copy of Breeze
with `logo = "icon:not-installed-in-any-theme"` renders pixel-identical (≤ 0.05 px differing) to `logo = "builtin:grid"` on the taskbar's
Start and on Layan's bar, and differs from the style's own Windows squares (44 px differ). The system's `/usr/share/icons/breeze` could
not be hidden without root, so the real "Breeze not installed" case was exercised through a missing icon name, not an actual
uninstall. Observation for the panel owner: the neutral 2×2 grid on the taskbar is smaller and softer than the Windows squares but is
still a four-square mark at a glance; a visibly different neutral shape (for example dots in a 3×3 grid) would avoid the resemblance.

1. **No real global menu (Layan), so `app_menu` is not shipped by default.** Plasma's `org.kde.plasma.appmenu` shows the focused
   application's own menu titles (`File(F) Edit(E) ...` in the upstream preview). Decebal's menu-bar `app_menu` draws fixed placeholder
   titles `File Edit View Window Help`, which would be a fake shown as if it were the application's menu, so Layan's default set omits
   it (this also removes the duplicated logo: `launcher` and the system-menu slot of `app_menu` both drew the logo). The focused window's
   real title (`window_title`) stands in, labelled as a stand-in here and in the manifest; the user can still enable `app_menu`. A
   genuine implementation needs a **menu exporter service**, exact API from the pinned Plasma source
   (`plasma-workspace/appmenu/menuimporter.{h,cpp}`, `com.canonical.AppMenu.Registrar.xml`, `appmenu.cpp`, commit `dec158c0`):
   own the session-bus name `com.canonical.AppMenu.Registrar` at `/com/canonical/AppMenu/Registrar` with
   `RegisterWindow(u windowId, o menuObjectPath)` (no reply), `UnregisterWindow(u)`, `GetMenuForWindow(u) -> (s service, o menuObjectPath)`
   and the `WindowRegistered(u, s, o)` / `WindowUnregistered(u)` signals; apps that export a menu also set the X11 window properties
   `_KDE_NET_WM_APPMENU_SERVICE_NAME` and `_KDE_NET_WM_APPMENU_OBJECT_PATH`; the consumer then reads each menu through
   `com.canonical.dbusmenu` (`GetLayout`, `AboutToShow`, `Event`, `LayoutUpdated`/`ItemsPropertiesUpdated` signals; the existing
   `src/sni.rs` client already speaks part of it for tray menus). Until that exists the applet must say it is unavailable (naming the
   missing service), not draw fixed titles. This section is **source evidence and a requirement statement only**. The menu API and bridge are coordinated by the separate app-menu bridge
owner; nothing in these packages or this document claims that exported application menus exist or proposes the design.
2. **Duplicate logo on Layan's top bar: resolved in the package** by omitting `app_menu` (above). The renderer should still not repeat the
   logo when `launcher` and `app_menu` are both enabled; relayed.
3. **Vertical Breeze taskbar preview is collapsed.** In the 71e3434 preview of `plasma-breeze-{left,right}-*`, the taskbar is a small
   cluster at the bottom of the strip (Kickoff glyph overlapping the stacked clock) instead of a full-height column. That is the
   renderer or the preview painter, not the package. Layan's left/right rails are laid out correctly (logo, menu glyph, stacked clock at the
   foot, upright icons).
4. **Taskbar vs Plasma panel.** No floating 8 px gap, no accent task indicator on the screen-facing side, no pager cells, no margins
   separator, Windows-style show-desktop sliver instead of Plasma's square icon (`showdesktop` uses `user-desktop-symbolic`).
5. **Per-mode icon themes.** Layan's light and dark use `Tela-circle` and `Tela`; one package has one `icons.theme`. Tela is not
   installed here or bundled. Breeze relies on `monochrome_tint` instead of `breeze-dark`.
6. **Decoration.** `decoration.buttons` is `Pending` for side/order (the style decides). The native caption helper was unavailable in
   the preview worker, so caption fidelity is not shown and Layan's `adwaita` choice is an approximation without a native capture.
7. **Wallpaper catalogue.** `tests/curated_theme.rs::catalogue_covers_shipped_designs_with_reviewed_sfw_provenance` fails (24 package
   ids vs 22 catalogue keys) until the merge-ready records below are merged by their owner.
8. Not modelled at all: Plasma adaptive opacity, blur strength parity, cursor theme, Kvantum/Qt widget style, Kickoff, tray, calendar and
   notification popups, sounds, splash, lock screen, SDDM.

## Wallpapers (merge-ready, not merged)

Packages declare no wallpaper (repository policy); the user's explicit **Wallpaper → follow theme = yes** reads
`data/wallhaven-curated.json` through `src/settings/wallhaven/curated.rs`, which resolves `themes.<id>.dark/light` to reviewed images
and verifies downloads against `full_resolution_review.sha256`. Separate Wallpaper Settings do not mean follow-theme is off. Until the
catalogue has the two ids the choice list is empty, so the records are written in the **catalogue's own schema** for a verbatim merge:
`docs/plasma-theme-ports/wallhaven-plasma-records.json` (`images{}` + `themes{}`, plus a `legibility_review` section that is not part of
the merge). No shared data file is edited here.

| Package / mode | Wallhaven | Original (private cache) | sha256 (first 16) | Notes |
| --- | --- | --- | --- | --- |
| Breeze dark | `yx3w57` | 3840×2160 PNG, 9.5 MB | `a3bac37436b91649` | Tasman Lake sunrise, blue/violet/pink sky (the image record lives in the shared catalogue, reviewed there; this port's own copy was dropped when the catalogue gained it) |
| Breeze light | `gwwre3` | 5120×2880 PNG, 19.9 MB | `d3e5a3240c5ce641` | pastel lilac dusk, snowy Lofoten; bottom fifth near-black spruce |
| Layan dark | `d61kgg` | 3840×2160 JPEG q99, 10.3 MB | `fe9b048f86620851` | Aoraki / Mount Cook under the Milky Way, indigo-violet; peak centred; near-black foreground |
| Layan light | `qz13pl` | 5120×2880 JPEG, 8.8 MB | `ccaca62b7f1f1ee5` | misty valley and hilltop chapels at sunrise, aerial |

A brighter daylight alternative for Breeze light (`v9kw2m`, Picos de Europa aerial, 5280×2970 JPEG q75) was fully reviewed and **not
chosen**: its portrait 9:16 centre-fill cuts the main peak in half and the 1.4 MB file looks smoothed at 1:1. Its record and legibility
numbers (≥ 10.1:1 on every edge and viewport) are in `alternates_not_for_merge` of the records file.

Three earlier picks were replaced: Breeze light `57o619` (already curated for `nextkde`), Layan light `p8kyvm` (its ultrawide crop clipped
the tree crown, bottom edge 4.47:1) and Layan dark `gpjk37` (portrait side edges only 4.01:1 and 4.43:1, strongly tone-mapped sky). The
replacements keep their subject in **ultrawide 21:9 (2560×1080) and portrait 9:16 (1080×1920) centre-fill**, viewed for every image. Layan
dark `d61kgg` has a minimum bar-text contrast of **7.12:1 on every edge at all four viewports**; Layan light `qz13pl` reaches 6.06–9.46 on its
own top edge. Layan light's other edges still dip to 4.44:1 because a light panel over the near-black foreground cannot do better with
this photograph; a second bright candidate (`p2eg9e`, Matterhorn) was rejected because its portrait crop cuts the mountain. Rejected and
replaced picks are kept in `alternates_not_for_merge`.

How it was reviewed (2026-10-31): fresh detail API responses at download time (all SFW, tag 133451 absent, no tag changed since the
first look), originals downloaded into `~/.cache/decebal-plasma-ports/wall/full/img` (persistent root disk, never `/tmp`), byte size equal
to the API's `file_size`, decoded with ImageMagick, 16:9, 21:9 and 9:16 views and 1:1 crops inspected, sha256 recorded.
`tools/plasma-wallpaper-review.py` composites each package's own panel fill and alpha over the centre-filled original under all four
edges at 1920×1080, 2560×1080, 1920×1200 and portrait 1080×1920 and records WCAG contrast of the package's bar text. Each package's **own
default edge** (Breeze bottom, Layan top) reaches ≥ 9.46 / ≥ 6.06 respectively. The weakest measurements are on edges the user may move a
panel to: Layan light bottom 4.44 and portrait left 4.44; everything else ≥ 4.5 (Layan dark ≥ 7.12). This judges
legibility only. Authorship, licence and capture method are **not** verified: Wallhaven provides no licence, uploaders are not authors,
photographer names that appear as tags (Sven Roden, Marek Tomasik) are unverified `reported_candidate` values, and
photographic appearance is not proof against AI generation. `d61kgg` is a long-exposure star field with slight trailing and noise; `gwwre3` and `qz13pl` are tagged
drone/aerial or photograph by the provider and show natural grain and detail at 1:1. No Wallhaven image, thumbnail or derivative is
committed. The merge also needs `docs/wallhaven-curated.md` and the 22→24 counts in `tests/curated_theme.rs` updated by their owner; a
test here proves the merged catalogue covers both ids and passes the same invariants. Authentic upstream wallpapers (Breeze "Next" by
Krystian Zajdel, CC-BY-SA-4.0; Layan's own) are referenced by pinned URL and not bundled.

## Evidence

Shared engine for contract validation and renders: `decthemectl` built with `cargo build -j2 --offline --bin decthemectl` from the archive
of commit `71e343413fdafcc6fde638d701b63c183512f1c8` (private `CARGO_TARGET_DIR=~/.cache/decebal-plasma-ports/target-shared`), binary
`~/.cache/decebal-plasma-ports/target-shared/debug/decthemectl`. Validation against it (`decthemectl theme validate themes/<id>`) passes
with one expected warning each (`decoration.buttons` is accepted but the style decides the side). Package content hashes at this revision:
`plasma-breeze` `sha256:cb36216158f37e0ff2a4ee2e437405825a92a7c67d285437090608cb00a84d21`, `plasma-layan`
`sha256:1919c5d20a7d6ea6efffaa0c6425de0dae6ef2e4971957b2118ec24bd5fbb947` (they change with any file edit).

Results: `tests/plasma_theme_ports.rs` passes 9/9 both on this branch's 0.2.0 engine and in a private overlay of this branch's new files on
the 71e3434 archive (where `theme::engine::shipped`, 4 tests, also passes with both packages). On this branch alone
`theme::engine::shipped::every_theme_folder_is_a_valid_package_whose_id_is_its_folder` **fails** because the 0.2.0 validator rejects
`[bar] applets`, `[branding]` and the 0.3.0 floor; that is the expected dependency on the framework, not a package defect.

Build slots: heavy cargo jobs ran as `flock ~/.cache/decebal-build-slots/slot2.lock nice -n10 cargo ... -j2 --offline` with private target
directories, after checking the UTC time against the 11:36:32 UTC cutoff.

Commands (no live config, display, process or install; private `HOME` under `~/.cache`):

```sh
# this branch's own engine (0.2.0): contract-dependent assertions are skipped, manifest assertions run
CARGO_TARGET_DIR=$HOME/.cache/decebal-plasma-ports/target cargo test -j2 --offline --test plasma_theme_ports
# the same tests against the committed shared engine: archive 71e3434, overlay this branch's new files, run
#   (themes/plasma-*, tests/plasma_theme_ports.rs, docs/plasma-theme-ports*, tools/plasma-*, data/ is the shared one)
tools/plasma-edge-matrix.sh $HOME/.cache/decebal-plasma-ports/target-shared/debug/decthemectl $HOME/.cache/decebal-plasma-ports/evidence/matrix-71e3434
python3 -I tools/plasma-wallpaper-review.py themes <image-dir> <out.json> <id>=<theme>:<mode>:<file> ...
```

Native previews at 71e3434 (`decthemectl theme render ... --png`, production preview painter, 1920×1200): all 16 renders (2 packages × 4
edges × 2 modes) are pairwise distinct (first 12 hex of sha256: Breeze dark top `393562aa8f6b`, bottom `983abc57e7e9`, left
`430ffb063541`, right `18227fa86bf6`; Breeze light `9c0609af892e`, `05b0b8a8d717`, `92b3fb6a4c43`, `b5ff5e5bd19e`; Layan dark `403a854fd427`,
`5721b73253a9`, `9482e89497b4`, `17dcfa12148d`; Layan light `0242c4929b33`, `5c214fd69cf6`, `ab4cfaabb71c`, `251ecbda0ad6`). The previous
(0.2.0) matrix had left/right byte-identical to bottom. Committed subset: `docs/plasma-theme-ports/*.png`.

What the previews prove: package-driven tokens, branding, applet sets and the four edges render natively through the shared painters with
fixed sample data. What they do not: any real desktop, applet toggling in Settings, popups, captions (helper unavailable), compositor
translucency, keyboard/pointer behaviour, scale factors, work-area struts, applets that need hardware the worker lacks. Native
four-edge completion is **not** claimed: the shared owner is still implementing renderers, and gap 3 above is visible already.

Tests (`tests/plasma_theme_ports.rs`): colour tokens recomputed from the pinned scheme numbers; contrast; package validity (verbatim
with the contract, only the new keys may be rejected without it), reproducible pack, `min_shell_version = "0.3.0"`, attribution, upstream
pins, no art assets; panel/branding/applet declarations (Kickoff logo, neutral "Desktop" label, no foreign defaults); tiling policy absent from the
manifests and preserved by a real apply plan; edge capability (while an engine drops left/right it must say so; with the contract all
four edges must plan without notes and render four distinct previews); a human gate that keeps `NOT READY`; and the wallpaper records
merged into the shipped catalogue in memory (catalogue invariants, runtime resolver URL/tag rules, coverage, 21:9 and 9:16 reviewed,
default-edge legibility ≥ 4.5, other edges ≥ 4.0, cached originals' sha256 when present).

## Integration gates for the parent

1. Integrate the universal-panel framework first (this branch does not carry it). The contract-dependent assertions then switch on by
   themselves; `theme::engine::shipped` and `tests/theme_ports.rs` iterate every package and need the contract. Validated so far on shared
   `71e3434`, `b28cca5` and `8042ff7` (overlays).
2. Relay `plasma-theme-ports/engine-requests.md` to the universal-panel owner: per-edge task indicator, panel metrics (thickness 44–46, 8 px
   floating gap, 28 px icons, thickness regression 44 → ~70 px between `71e3434` and `b28cca5`), pager cells, Start tint, collapsed
   Breeze side strips (blocker test `breeze_side_strips_are_full_height_native_panels`, run with `--ignored`), plus the application-menu
   exporter facts for the app-menu bridge owner. The proposed `src/panel/plasma_style.rs` helper API is **not started**; it needs your
   agreement on file ownership first.
3. Asset packs: the installer and `catalog check` from the asset owner are not committed; re-run `assets/desktop-ports/plasma/asset-packs.toml`
   and the receipts (`plasma-theme-ports/asset-receipts.json`) through them, register the `plasma` catalogue in
   `src/desktop_assets/builtin.rs`, and decide how to carry icon themes that need a generation step (Breeze: recipe and `import` route
   documented, Tela: no route). Cursor packs are contract-compatible.
4. Wallpapers: merge `images{}` and `themes{}` of `plasma-theme-ports/wallhaven-plasma-records.json` into `data/wallhaven-curated.json`, update
   `docs/wallhaven-curated.md` and the 22→24 counts in `tests/curated_theme.rs` (central wiring is root-only). Originals are in the private cache
   with recorded hashes; licences and authorship stay unverified.
5. Decide whether to register the ids in `src/theme/builtin.rs`; `make install` copies every file in `themes/` (now including `widgets/`).
6. Remove `NOT READY` only after real native four-edge, every-applet and launcher acceptance on the final engine.

## Fidelity summary

Faithful (source-derived and tested): all colours of Breeze Light/Dark and Layan Dark/Light, translucency numbers, Breeze radius, fonts,
Breeze icon theme, Kickoff's logo and empty label, a neutral desktop label, stock applet sets, KWin tile padding as visual gaps.
Approximated: panel shape (Windows-style taskbar, flat menu bar), decoration renderer (rdwm `breeze`, `adwaita`), Layan radius (from a
screenshot), Layan icons (fallback), panel height. Missing: everything in the gap list. Do not describe either theme as a finished
Plasma port.
