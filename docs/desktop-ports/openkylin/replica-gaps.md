# openKylin / UKUI replica: references, what is matched, exact gap list

Status of this document: **first replica increment** (start menu, shortcut panel, taskbar search, start-button mark). It is a gap list, not a completeness claim.

## 1. Reference material (primary sources only)

GitHub `openkylin/*` is a read-only April-2025 mirror; `ukui-menu` is empty there. UKUI's active development is on **gitee.com/openkylin**, branch `openkylin/huanghe`. Sparse checkouts (QML, resources, settings, data) were pinned
on 2026-10-31; the exact commits, trees and declared licences are in `assets/desktop-ports/openkylin/sources.json` → `gitee_huanghe`:

| repo | commit | declared licence | used for |
|---|---|---|---|
| ukui-menu | `6330250eb56b` (2026-10-26) | GPL-3.0+ | start menu: 766 × 688, margin 8, columns 312 / 1 / 396 / 1 / 56, search 40, header 48, rows 40 (32 px icon), tabs 32, favourite cells 88 × 100, 36 px sidebar buttons, card radius = `kRadiusWindow` (`qml/mainXcb.qml`) |
| ukui-sidebar | `59abed062553` (2026-10-29) | GPL-3+ | shortcut panel: 396 wide, 20 px sides, 4 columns of 80 × 72 cells, 12 / 16 px gaps, 356 × 56 progress rows, radius `kRadiusWindow` / `kRadiusMax` 8 |
| ukui-panel | `7d8829d462e6` (2026-10-26) | GPL-3.0+ | classic default widget order (`panel/src/view/panel.cpp`), island contents (`panel-next.cpp`), search widget = a square `kylin-search` icon button (`widgets/ukui-panel-search`), start button (`widgets/ukui-menu-starter`, icon `kylin-startmenu`) |
| ukui-globaltheme | `5274c41a196d` (2026-09-18) | GPL-3.0+ | `windowRadius` (v11 12, Classic 0, all `--kradius-*` 0 in Classic), `preview.png` mock-ups |
| ukui-quick, ukui-search, ukui-greeter, ukui-control-center | pinned | GPL-2+/3+ | pinned for later phases; the Control Center's `logoukui.svg` is the wordmark and is **not** used |

Nothing is copied: the numbers are measurements and the Decebal code is independent MIT work. The only upstream *file* in the packages is the start-button PNG (`themes/*/icons/kylin-startmenu.png`,
pinned git blob, see each `ATTRIBUTION.md`; provenance of the Moka part of the icon art stays unresolved).

**No runnable UKUI desktop exists in this environment** (the shell needs `ukui-quick`, KWin and the session). Therefore there are **no original-vs-ours pixel captures**; what is compared is
(a) the numbers in the QML, asserted by unit tests (`src/launcher/styled/ukui.rs`, `src/controlcenter/ukui.rs`), (b) the upstream theme `preview.png` mock-ups (not captures of a running desktop: material and mood cues only),
(c) our native captures on a private owned Xvfb (`tests/e2e/openkylin-native.py --matrix launcher|cc`, evidence under `native/`).

## 2. Matched (source number → native proof)

| area | upstream | ours | proof |
|---|---|---|---|
| start menu size / columns | 766 × 688, 312 / 1 / 396 / 1 / 56 | `launcher.style = "ukui"` | unit test + native bbox at 100 / 125 / 200 %, light/dark, four packages |
| sidebar | 36 px buttons, 4 px apart, 12 px margins; settings and power at the bottom | same, settings and power are real targets | unit test |
| rows / favourites | 40 px rows, letter headers with a divider; 88 × 100 cells | same | unit test + capture |
| card radius | `kRadiusWindow` (12; Classic 0) | `theme.radius` | capture (Classic is square) |
| shortcut panel | 396 wide; 4 × (80 × 72) cells; 356 × 56 bars; header with settings button | `control_center.style = "ukui"` | unit test + native capture |
| classic panel search | square icon button, not a box | generic `[taskbar] search = "button"` | unit test (key parse) + capture |
| start button | `kylin-startmenu` mark of the pack | bundled per package (default / classical / fashion) | blob ids verified by `tests/openkylin_port.py` |

## 3. Exact gap list (what is NOT yet a replica)

| area | gap | kind |
|---|---|---|
| start menu | full-screen mode, tablet mode, drag and drop, labels / category pages ("All" is the only list), more than the Favorites and Recent plugin tabs | interaction |
| start menu | user / computer / expand sidebar buttons are drawn but do nothing | interaction |
| start menu | blur: UKUI blurs the card through KWin's blur region at the theme's 65 % alpha; here the card is the theme's popup colour at ≥ 78 % and relies on the compositor for blur (not proven on a photographic backdrop) | material |
| start menu | typography is the system's `Noto Sans CJK SC` / `Noto Sans` at the sizes read from the QML (14 px), not rendered by Qt's text stack: hinting differs | typography |
| start menu | UKUI's 150 ms hover/press fades, the list scroll bar (14 px) and the open/close animation are not implemented | animation |
| start menu | app and favourite icons come from the user's icon theme: with another pack the grid differs from the pack's own look (intended: icon choice stays the user's) | icons |
| shortcut panel | UKUI's panel is part of the sidebar (a 400 px full-height side panel with the notification column at 384 px); Decebal opens only the shortcut part as a flyout above the panel | geometry |
| shortcut panel | edit mode, drag re-ordering, lottie icon animations, tablet mode | interaction / animation |
| shortcut panel | the shortcut list is limited to what the shell can control; hardware rows (volume, brightness) read "Unavailable" on a machine without the device (as in the fixture) | content |
| shortcut panel | Classic's tile radius is 0 (all `--kradius-*` are 0px); the ukui panel follows it, but the glyph set is the shell's symbolic icons, not UKUI's `ukui-*-symbolic` | icons |
| classic taskbar | task-view, separator and AI-assistant widgets have no applet; the system tray is one applet group instead of `org.ukui.systemTray`; Classic's plate corner clamps at 2 px (UKUI 0); the search glyph is the shell's magnifier instead of the pack's `kylin-search` | content / icons |
| classic taskbar | background and shadow of the strip: UKUI draws a `DtThemeBackground` with `WidgetContainer.radius` and margin; the Decebal strip is flush with a hairline (no UKUI shadow) | material |
| island taskbar | contents and order follow `panel-next.cpp` (data: menu, IDM, task view; app: tasks, search; settings: tray, calendar) except IDM / task view (no applet); the original is **bottom-only**: top, left and right are a Decebal **extension** (kept for the four-edge ecosystem) and not part of the matched source | content / disclosure |
| icons / cursors | UKUI default / classical / fashion icon packs and Dark Sense / Blue Crystal / DMZ-White cursors are installed through the shared catalogue (private-install proof in `icons-cursors.md`); the new native captures use the default icon pack prefix; cursors are not visible in screenshots | proof |
| decorations | native frame on a mapped client window is the decorations owner's capture | proof |
| greeter, lock screen, IDM, task-view overview | not replicated | scope |

## 4. Extensions that are disclosed, not source-matched

* Island panel on the top, left and right edges (UKUI islands are bottom-only).
* `[taskbar] search = "button"`, `bar.cluster` etc. are generic Decebal keys any port can use; no theme id appears in the engine.
* `launcher.style = "ukui"` and `control_center.style = "ukui"` are selectable by any theme or by the user in Settings; the packages only suggest them.
