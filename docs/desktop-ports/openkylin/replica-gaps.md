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

## 2. Matched (source number or behaviour -> proof)

| area | upstream | ours | proof |
|---|---|---|---|
| start menu size / columns / border | 766 x 688, 312 / 1 / 396 / 1 / 56, `kRadiusWindow` (Classic 0), 1 px `textActive` border at alpha 0.15 | `launcher.style = "ukui"` | unit tests; native bbox at 100 / 125 / 200 %, light/dark, four packages |
| sidebar | 36 px buttons, 4 px apart: expand, user, computer, settings, power | **all five are targets**: expand / restore (full-screen menu), user -> Settings Accounts pane, computer -> `peony computer:///` (else `gio open`, else `xdg-open /`; not drawn when none exists), settings, power -> session menu | unit tests (hit == draw rect), native xdotool clicks with recording stubs: the exact program line is recorded and the menu closes |
| full-screen menu | `FullScreenUI.qml` / `FullScreenAppList.qml`: search 372 x 36 centred, restore 36 at the top right, 112 px sidebar, grid `n = clamp(ceil(c / 188), 1, 9)` x 160 px cells, `c = (4 + 2560 / w) * w / 8`, vertical gap h / 27, radius 0 | same formulas, card = the area the panel leaves, the typed search survives | unit test of the formulas and of expand/restore on four edges, native: covers the whole free area, restore returns the start menu |
| composited material | `mainXcb.qml`: window colour at `transparencyBlur` (65 %, HeYin 75 %) over a compositor blur region, 90 % without; content opaque | `overlay` = window colour, `panel_alpha` = transparencyBlur; the launcher tags its window for the blur rule and is shaped to the card; the shadow no longer darkens the card | native material matrix: expected mean within 0.4 / 255, blurred stripes smooth, stripes at (1 - alpha) without blur, app-row text reaches the ink colour; backdrop is **synthetic** (this is not original-application proof) |
| rows / favourites | 40 px rows, letter headers; 88 x 100 cells | same | unit test + capture |
| keyboard | Tab between pages, arrows inside, Return activates | same zones; the sidebar buttons are in the focus order | native: Tab x3, Down x3, Return runs Settings |
| narrow / short bounds | n/a (fixed size upstream) | a card too narrow for one favourite cell drops the widget page; short cards compress the search/header block and scale favourite / plate / row icons and names to the actual cell; sidebar buttons shrink; in the full-screen menu the search bar narrows and shifts left of Restore (12 logical px apart wherever the card leaves room) and a pill too narrow for 24 px of text becomes a compact glyph-only pill; plates are centred on the real (narrowed) click cell | unit tests: 320 x 240, 400 x 300, 480 x 320 ... 1920 x 1080, scales 1 / 1.25 / 1.5 / 2, five anchors, normal and full-screen: the painted search pill (magnifier, query line), plates, icons, names, rows and controls lie inside their click cell and the card, rectangles do not overlap, every focusable item is hit at its centre; Settings and Power stay reachable. **Degenerate cards are in this matrix, not untested** (320 x 240 at 2x leaves a card of about 128 to 160 logical px: widget page dropped, one grid column, glyph-only search); below that the policy is unbounded (nothing is hidden to make room) |
| shortcut panel | 396 wide; 4 x (80 x 72) cells; `ProgressBar.qml` row 356 x 56: 12 px margins, a 24 px icon button, 8 px, a 16 px slider | `control_center.style = "ukui"`: the icon button is drawn (volume: **Mute**), the track has no icon inside | unit tests; native capture |
| tile groups | n/a | exactly the enabled groups (`connectivity focus display sound media toggles actions`): a disabled group never comes back (no fallback to all groups, also when every group is off); Settings only with `actions`; an enabled group without hardware on the machine shows nothing (volume / brightness rows read "Unavailable"); with nothing to show the panel keeps the header (and Settings if `actions` is on) plus one neutral, non-interactive line "No controls available". How an explicit empty list is parsed (`tiles = []` none, omitted = all, as in root main `0d23768`) is **integration contract pending**: this tree's parser is not changed and was not verified for it | unit + bar-level tests (source increments `UNRUN`, see below) |
| WLAN | the Wi-Fi shortcut toggles the radio | `Target::Wifi` (`nmcli radio wifi on/off`); the wired Network editor is a separate shortcut | bar-level test through the dry-run hook against recording stubs (`tests/e2e/openkylin-cc-actions.py`) |
| classic panel search | square icon button | generic `[taskbar] search = "button"` | unit test + capture |
| start button | `kylin-startmenu` of the pack | bundled per package, blob ids pinned | `tests/openkylin_port.py` |
| the real taskbar Start | the dock passes its edge and anchor | root's `fbddaf8` | native: a real click on the Start cell opens the card at the Start end, clear of the strip, on four edges, 100 / 200 %, light/dark, two packages (built with `fbddaf8` applied in the scratch tree only; qualify on the merged tree) |

## 3. Exact gap list (what is NOT yet a replica)

| area | gap | kind |
|---|---|---|
| original captures | no running UKUI desktop is available, so there are **no original-vs-native pixel comparisons**; `native/replica-compare-*.png` set the official `ukui-globaltheme` preview mock-ups (identified by commit and hash, in `replica-compare.json`) next to native captures of the same theme and mode: composition and mood only | evidence |
| start menu | tablet mode, drag and drop, labels / category pages, the full-screen label list and list-mode buttons, plugins other than Favorites and Recent, the user menu's other accounts and Log Out (Power's session menu logs out) | interaction |
| start menu | blur is the compositor's: the shell requests it (`_DECEBAL_BLUR`), picom proves it in the fixture, KWin was not run; not proven on a photographic backdrop; UKUI's Qt menus use `menu-transparency` 72 (not our 0.82 `popup_alpha`) | material |
| start menu | typography is `Noto Sans CJK SC` / `Noto Sans` at the QML sizes (14 px), not Qt's text stack: hinting differs | typography |
| start menu | 150 ms open / hover transitions, the animated move into full screen, the 14 px scroll bar | animation |
| start menu | app and favourite icons come from the user's icon theme (intended: the choice stays the user's) | icons |
| shortcut panel | UKUI's is the shortcut part of a 400 px full-height sidebar with a 384 px notification column; Decebal opens the shortcut part as a flyout | geometry |
| shortcut panel | edit mode and drag re-ordering, lottie icon animations, the 24 px sub-page arrow of rows whose plugin has a page (needs plugin pages), tablet mode; the original's default list also has auto-rotation, clock, notebook, multi-screen, services and split-view shortcuts the shell has no control for; Quiet / Awake / Notices / the `decutil` toggles / Now Playing are **disclosed extensions** (not in the original's default list) | content / interaction |
| shortcut panel | glyphs are the shell's symbolic set, not UKUI's `ukui-*-symbolic` | icons |
| classic taskbar | task-view, separator and AI-assistant widgets have no applet; Classic's plate corner clamps at 2 px (UKUI 0); the search glyph is the shell's magnifier, not the pack's `kylin-search`; no UKUI panel shadow | content / icons / material |
| island taskbar | contents and order follow `panel-next.cpp` except IDM / task view (no applet); the original is **bottom-only**: top, left and right are a Decebal **extension** | content / disclosure |
| icons / cursors | installed through the shared catalogue (private-install proof in `icons-cursors.md`); cursors are not visible in screenshots | proof |
| decorations | the frame on a mapped client window is the decorations owner's capture | proof |
| licences | root owns the licence-text and generator clean-up of the bundled logos after handoff; this lane does not call licence handling complete | scope |
| greeter, lock screen, IDM, task-view overview | not replicated | scope |
| tests | six unrelated lib tests fail in the shared private environment (`settings::navigation` socket path length, `settings::wallhaven::store` equal XDG dirs, `settings::themepane` palette count, `dock_app` focus walk, `settings::schema` vs the host `rdwmctl`); five pass serially with a short private HOME, `settings::schema::snapshot_matches_installed_rdwmctl_if_any` fails against the installed `rdwmctl` (host state, unchanged) | preserved |

See also [toolkit-contract.md](toolkit-contract.md): materials, Qt vs GTK, decoration states, icons (routed items marked).

## 4. Extensions that are disclosed, not source-matched

* Island panel on the top, left and right edges (UKUI islands are bottom-only).
* `[taskbar] search = "button"`, `bar.cluster` etc. are generic Decebal keys any port can use; no theme id appears in the engine.
* `launcher.style = "ukui"` and `control_center.style = "ukui"` are selectable by any theme or by the user in Settings; the packages only suggest them.

## Status of the latest source increment

The search/Restore, plate / icon / name / search-glyph geometry, neutral empty state, tile-group and clippy-lint changes after `b9d01f2` (commits `47f6544` and the follow-up) were written and checked with `rustfmt` and `py_compile` only: **unrun** (no cargo, no native run), by instruction. Proposed gates for root, one job at a time: `cargo clippy --all-targets -- -D warnings`; `cargo test --lib -- launcher::styled::ukui controlcenter::ukui menubar::ukui_cc_tests` (the painted-geometry test name starts `painted_geometry_of_search_plates`); `python3 tests/e2e/openkylin-cc-actions.py` (action driver); `python3 tests/openkylin_port.py`; then the native matrices `launcher-expand`, `launcher-edges`, `start-click` (needs `fbddaf8`) and `material` of `tests/e2e/openkylin-native.py`. The evidence under `native/replica/` was produced on `b9d01f2`-level binaries and does not cover this increment.

### Self-contained native launcher gate (new, `UNRUN`)

`tests/e2e/openkylin-launcher-native.py --bin <declaunch> --out <new dir> --fixture-dir <dir with the current private_x11.py> [--quick]` drives the real `declaunch` on an owned private Xvfb with no panel and no theme apply: package `openkylin` and `launcher.style = "ukui"` come only from `decebal.json`; both D-Bus buses are dead, HOME/XDG are private, PATH holds only fake action tools, the exact binary is copied so installed siblings cannot run, the applications are synthetic coloured fixtures. Per run (320x240, 400x300, 1600x900; scale 1, 1.25, 2; light and dark; four panel edges): Shift+Tab from the initial list focuses the expand button (only the UKUI style has it, so an unknown-style fallback fails), Return expands, Return restores, with the same PID and X window id and real pixel changes; the card stays inside the output and clear of the strip on every edge; the full-screen card covers exactly the free area; typing `Green` and Return launches the green fixture app. On 1600x900 it also clicks the expand and restore buttons and a coloured icon, and checks the opaque card colour against the package. Cleanup uses a run token and pidfds; any SIGKILL, leftover token process or retry fails the run. It is not an upstream pixel comparison and says nothing about compositor blur. The older `openkylin-native.py` is evidence tooling with weaker cleanup and must not be executed as a gate.

### Root validation checkpoint: native panel-reservation regression

The `e5e3de4` production sources passed 17 focused tests, strict all-target Clippy and development builds of `declaunch`, `decbar` and `decthemectl` (`/var/tmp/decebal-ukui-corrected-source-gate-v2/result.json`). The immutable development launcher passed the self-contained 1600×900 light/bottom smoke test, including real pointer expand/restore and app launch (`/var/tmp/decebal-ukui-launcher-smoke-evidence/native-launcher.json`). These are not optimized-release or full-replica acceptance.

The expanded native matrix then passed 16 cases (320×240, 100/125%, light/dark, all four edges), but failed at 320×240, 200%, light/bottom. A 96-pixel explicit panel reservation was clamped by `resolve_anchor` to 80 pixels, so the expanded launcher painted 16 pixels over the reserved strip. Evidence: `/var/tmp/decebal-ukui-launcher-matrix-v2-evidence/`; the failure had clean process cleanup and no resource abort. The matrix is **failed**, not accepted.

The source repair now preserves explicit thickness up to the output extent minus one pixel; pointer-inferred thickness retains its conservative one-third limit. A focused regression covers all four edges with a 96-pixel reservation at 320×240. This repair is **not compiled or tested yet**; a new launcher build and native matrix are required. Earlier artifacts do not contain it.


### Root native launcher matrix accepted (development binary)

The corrected self-contained fixture at `42b2283` passed all 72 combinations: 320×240, 400×300 and 1600×900;
100/125/200%; light/dark; top/bottom/left/right. Keyboard expand/restore and search-to-launch passed with the
same PID/XID, and the desktop-size cases exercised pointer expand/restore and a coloured fixture-app click.
Evidence: `/var/tmp/decebal-ukui-launcher-matrix-v5-evidence/native-launcher.json`; guarded controller
`/var/tmp/decebal-ukui-launcher-matrix-v5-control/result.json`. Binary SHA256
`905fde5824358d8b7e511379f2f6154a2cafc2ca6bc31ffaae6cd5dd973c35c1` is the immutable development launcher
built from production sources `555c37a`. No forced cleanup, rescued/leftover token processes or pressure abort.

This qualifies the isolated launcher interactions and bounds. It does not qualify the main-branch integration,
Start-button anchoring, compositor material/blur, upstream pixel fidelity, Control Center parser semantics,
optimized release or physical hardware. Earlier failed/pressure-aborted runs remain recorded.


### Main-branch integration checkpoint

Merge `3997411` preserves the current Wayland repairs, anchored launcher calls and explicit-disabled applet
semantics. UKUI is now included in the common inert-empty-state regression. The merged snapshot passed 135
focused tests (1 ignored), formatting, strict all-target Clippy and five development binary builds:
`/var/tmp/decebal-ukui-main-integration-source-gate/result.json`. The integrated test artifact also passed the
owned-Xvfb glass input/bounds check and eight private Control Center action checks:
`/var/tmp/decebal-ukui-integrated-native-actions-gate/result.json`. These do not replace the pending integrated
launcher matrix or optimized-release acceptance. The preceding 72-case launcher proof belongs to its explicitly
recorded candidate binary.
