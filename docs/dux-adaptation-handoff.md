# Dux native adaptation and Theme Specialist handoff

Research date: 2026-10-30. This document describes upstream evidence and a bounded native adaptation, not a completed Dux runtime port. No QML, Go service, wallpaper, logo, font or screenshot asset is shipped by this change. The existing Theme Specialist remains the sole author of theme/package/art files.

## Verified primary sources and licensing

| Repository | Inspected commit | License at that commit | Scope |
| --- | --- | --- | --- |
| [dux-shell](https://github.com/duxlabs-dev/dux-shell/tree/46a12d915203a6bf296f26cab251c55efa6499ab) | `46a12d915203a6bf296f26cab251c55efa6499ab` | [MIT](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/LICENSE), Avenge Media LLC, 2025 | Settings/native desktop-widget design references below |
| [dux-plugins](https://github.com/duxlabs-dev/dux-plugins/tree/a55bb40da2f70640600a520ea05eac9c3540f1af) | `a55bb40da2f70640600a520ea05eac9c3540f1af` | [MIT](https://github.com/duxlabs-dev/dux-plugins/blob/a55bb40da2f70640600a520ea05eac9c3540f1af/LICENSE), Avenge Media LLC, 2025 | First-party Pomodoro and weather references |
| [dux-plugin-registry](https://github.com/duxlabs-dev/dux-plugin-registry/tree/f3079becbf35f4f48d46879fa46cb867f15771f9) | `f3079becbf35f4f48d46879fa46cb867f15771f9` | No root license file found; GitHub license metadata is null | Discovery metadata only; each external plugin needs its own license review |

The registry README explicitly distinguishes third-party plugins and their dependencies. Listing there is not a grant to copy their implementation or artwork. No external registry plugin is imported here. The shell also contains separately licensed bundled components (for example `core/pkg/ipp/LICENSE`, Plasma sounds); its root MIT must not be treated as universal asset clearance. Its [historical license-change record](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/LICENSE_CHANGE_12_11_2025.md) records GPL-to-MIT contributor consent; this research uses the current pinned MIT tree.

MIT permits adaptation and redistribution with copyright and permission notices retained in copies or substantial portions. The stock palette excerpt below is attributed to that MIT source; the full notice is retained at the end. Screenshots are viewed reference evidence, not redistributed assets. Do not assume rights to the pictured wallpaper, avatar, album art or logos.

## Actual visual evidence inspected

These images are linked directly by the pinned upstream README files and were downloaded and viewed during research:

- [Dashboard screenshot](https://github.com/user-attachments/assets/a937cf35-a43b-4558-8c39-5694ff5fcac4): broad charcoal panel, calm rounded nested cards; large stacked clock, compact weather card, calendar, narrow system meters and media card. Lavender is a restrained focus/active colour. It is a dashboard, not proof of the current Settings layout.
- [Control Center screenshot](https://github.com/user-attachments/assets/732c30de-5f4a-4a2b-a995-c8ab656cecd5): dark rounded host; paired compact tiles; accent leading icon containers; clear primary labels and smaller supporting state; an Add Widget list with readable label/description pairs.
- [First-party Pomodoro screenshot](https://github.com/user-attachments/assets/b51b5f78-5215-403c-850f-c7e137097438): large countdown and state in a progress ring; simple primary controls; Work / Short Break / Long Break buttons; separate low-contrast completion summary. This screenshot is older visual reference, not a claim that the current source renders identically.

Temporary local inspection copies: `/tmp/decebal-dux-research/{dashboard,control-center,pomodoro}.png`. No screenshot is used as an application surface.

## Settings hierarchy: concrete source-to-native mapping

Sources are the pinned shell tree under `quickshell/`:

- [SettingsMetrics.qml](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/Modules/Settings/Widgets/SettingsMetrics.qml) defines shared role/spacing metrics: pane and sidebar rows use `floatingWindowNestedSurface`; rows use `chipSurface`; controls use `chipSurfaceNested`; highlight blends primary at 0.2; selection uses `selectedContainer`.
- [SettingsSidebarItem.qml](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/Modules/Settings/Widgets/SettingsSidebarItem.qml) uses tonal selected rows, `onSelectedContainer` selected text, small supporting labels and theme-derived icon containers. Translate this to shared native navigation painting without altering routes or keyboard focus semantics.
- [SettingsCard.qml](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/Modules/Settings/Widgets/SettingsCard.qml) places section labels above grouped content. [SettingsGroup.qml](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/Modules/Settings/Widgets/SettingsGroup.qml) supplies shared outlines, outer rounding and continuous-group divider handling. Translate to native section typography and quiet group frames, preserving item order, row hit regions and saved drafts.
- [SettingsRow.qml](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/Modules/Settings/Widgets/SettingsRow.qml) distinguishes primary label, secondary description, trailing control and visible focus ring. Keep this hierarchy at 100% and 200% text size and in short windows; do not trade accessibility for denser imitation.

Upstream dimensions are evidence, not drop-in constants: sidebar 336, compact breakpoint 700, content maximum 920, default window 1100×940, minimum 500×400. Native Settings already has its own routing, scrolling, adaptive controls and drafts. Retain those behaviors and adapt the visual roles. Its selected row must remain identifiable without hue alone; avoid hardcoded rainbow icon badges and macOS traffic-light framing.

## Exact palette handoff to the existing specialist

Canonical source: [StockThemes.js](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/Common/StockThemes.js), `DARK.purple` / `LIGHT.purple`. These are explicit source values, not colours sampled from a screenshot. Dynamic matugen palettes and user role overrides can produce other colours.

| Dux role | Purple dark | Purple light | Suggested Decebal token / use |
| --- | --- | --- | --- |
| background / surface | `#141218` | `#fef7ff` | background, host surfaces |
| surfaceContainerLow | `#1d1b20` | `#f4eef6` | sidebar / quiet nested area |
| surfaceContainer | `#211f24` | `#f2ecf4` | surface / grouped cards |
| surfaceContainerHigh | `#2b292f` | `#ece6ee` | raised rows / controls |
| surfaceContainerHighest | `#36343a` | `#e6e0e9` | nested control surfaces |
| surfaceText | `#e6e0e9` | `#1d1b20` | text |
| surfaceVariantText | `#cac4cf` | `#49454e` | text_secondary |
| primary | `#D0BCFF` | `#6750A4` | accent |
| primaryText | `#381E72` | `#ffffff` | accent_text |
| primaryContainer | `#4F378B` | `#EADDFF` | tonal action / active region reference |
| outline | `#948f99` | `#7a757f` | border hue; attenuate appropriately |

Specialist coordination received during this task: its package uses stock Purple dark/light accents `D0BCFF` / `6750A4`, dark background `141218`, and proposed light background `fffbfe`; the latter is a specialist adaptation, not the exact pinned stock background (`fef7ff`). Confirm deliberately rather than relabeling it as an exact upstream token. Floating top bar, Materia captions, dock disabled, no default wallpaper are the agreed package direction. This task does not modify that package.

Use native theme tokens for shared Settings; do not bake Purple into all themes. A bounded adaptation can blend `accent` over `surface` for tonal selection and derive nested controls from existing `surface`, `background` and `border`. Respect light/dark palette contrast. Package-specific palette, radii, fonts, bar layout and artwork remain specialist-owned.

## Useful native widgets and placement handoff

1. `builtin:resource-overview`, nominal **320×248**: inspired by [SystemMonitorWidget.qml](https://github.com/duxlabs-dev/dux-shell/blob/46a12d915203a6bf296f26cab251c55efa6499ab/quickshell/Modules/BuiltinDesktopPlugins/SystemMonitorWidget.qml). Upstream groups CPU / memory / network / disk tiles, dominant utilization values, small used-memory details, paired receive/transmit rates and cached histories. Adapt only existing native CPU/RAM/network/disk paths. Missing data must say unavailable rather than display a fabricated zero. No GPU, temperature, process table or dgop dependency is implied.
2. `builtin:focus-hub`, nominal **320×292**: inspired by [DankPomodoroWidget.qml](https://github.com/duxlabs-dev/dux-plugins/blob/a55bb40da2f70640600a520ea05eac9c3540f1af/DankPomodoroTimer/DankPomodoroWidget.qml). Use the existing shared Timer state/countdown/title and Quick Note summary with allowlisted native app actions. Preserve a dominant countdown plus secondary task/status hierarchy. Do not fork the Timer state, claim completed-session statistics, or add unsupported break automation.
3. Existing clock/weather/media cards remain native companions. [DankDesktopWeather.qml](https://github.com/duxlabs-dev/dux-plugins/blob/a55bb40da2f70640600a520ea05eac9c3540f1af/DankDesktopWeather/DankDesktopWeather.qml) and `WeatherMetric.qml` support a useful visual principle: dominant current temperature, readable condition/location, compact labelled secondary values and an explicit missing-data state. No extra provider/network fetch is needed to borrow the hierarchy.

Provisional specialist arrangement: right column resource at y=72 and focus at y=336; left companion clock/media at y=72/252. These are design coordinates, not workarea repair logic. Validate output bounds/gaps and enlarged native auto-height before adopting; the parent owns the separate theme-change workarea-overlap repair. The new cards opt in to `auto_height` for enlarged fonts; fixed nominal dimensions are not permission to crop labels at 200%.

The native DSL/painter must consume cached snapshots. CPU sampling, disk/network reads, image decoding and network work remain outside paint. Use existing Theme trust parsing and allowlisted app actions; no `exec`, arbitrary script actions, shell interpolation, new plugin installation or QML runtime. In particular the upstream Dank Actions and Dank Hooks script features are outside this adaptation.

## Acceptance and limits

The implementation owner must attach actual native-render evidence at 100% / 200%, short-window Settings scrolling, unavailable-data cards and light/dark palettes. A static upstream screenshot does not satisfy that gate. Check navigation/search aliases, focus, drafts, text wrapping, clipping and click targets in the native build. Retain cached item construction and cheap paints. This handoff is research evidence; it does not itself claim the runtime implementation or QA passed.

No theme creation, source-root edits, live configuration changes, installation, restart, push or PR are authorized in this workstream. Parent reviews and integrates the worktree commits.

### Native implementation and reviewed evidence

Settings commit `1f4f30b` replaces fixed rainbow navigation badges with semantic
tonal glyphs, a selected-route edge marker and keyboard focus outline. Section
labels align with outlined content groups. Routing, item IDs, drafts, item caches
and adaptive control geometry are retained. The shared painter adds no file,
image or network work.

Widget-only base commit `de48c1c` registers both cards independently of Settings.
Include follow-up `37c115f` when integrating: full CPU/RAM
unavailable labels must remain readable at 200%. Actual native font measurements
after that correction are **320×248 / 320×324** for resource-overview at 100/200%
and **320×292 / 320×363** for focus-hub. These are card sizes; exported renderer
evidence adds a 16-pixel margin on each edge. Both modes and sample/unavailable
states have the same heights with the tested installed fonts. Other font choices
can change measured height. At 200%, resource y=72 ends at 396; focus y=336 would
overlap. The specialist has since chosen resource top-right (32,72) and focus bottom-right
(32,32). That still requires at least 807 pixels of output height at 200% for a
16-pixel gap; a 720-pixel output needs a different arrangement. Use actual card
extents and output bounds, rather
than fixed nominal-height stacking. Simultaneous card placement on short outputs
remains a package/layout responsibility; no workarea implementation is changed.

Reproduce native Settings QA with `SETTINGS_BIN=/path/to/decsettings python3
tests/e2e/settings-dux-native.py`. It creates a private Xvfb and HOME/XDG paths,
blocks the inherited desktop bus, selects explicit light/dark modes, captures
100/200% UI scale at normal/short heights and exercises actual content scrolling.
It checks background luminance so a dark capture cannot be mislabeled light.
The existing native geometry tests additionally cover enlarged text metrics.
Reviewed captures live under `/tmp/decebal-dux-settings-evidence`; widget painter
captures are under `/tmp/decebal-dux-widget-evidence`. These are actual native
pixels, not screenshot surfaces or browser mockups.

Final `37c115f` native host QA also passed four actual mapped/painted windows at
100/200% text size on a private 640×480 display, and eight real mouse actions
captured by private sibling app fixtures. Reproduce with `python3
tests/e2e/dux-widgets-native.py /path/to/decwidgets /tmp/evidence`. Evidence is in
`/tmp/decebal-dux-native-widget-host-final`, including `checks.json`, screenshots
and the host log. Parent visually reviewed the corrected unavailable-state card
and final 200% focus host capture. All private test processes were cleaned up.

The X11 library suite passed **1595 tests, zero failed, 10 ignored** before the
small final resource-label correction; its affected native renderer test passed
separately. Strict X11 all-target Clippy with warnings denied and formatting passed. This is not a Wayland host, screen-reader, mixed-DPI or
physical-monitor acceptance claim. No QML runtime, new collectors, note bodies,
arbitrary exec actions or theme files are introduced.

### Second pass: structural Settings refinement

The first pass only recoloured navigation. This pass changes structure, still in `src/settings/{app,ui,icons,appearance}.rs`
with no new data, I/O in paint, theme file or hardcoded palette (every colour is a blend of existing theme tokens):

- **Header and search.** Search is now the top of the sidebar (as in Dux's `SettingsSidebar`); the redundant in-window
  "Settings" caption is gone and the header grows with enlarged text (previously it collided with the field at 200% text).
  A clear button joins Esc. Every page has its title plus one wrapped context line (`pane_context`, `Tab::blurb`).
- **Flat navigation.** No icon badges or nested boxes: glyphs sit directly on the row, the selected route is one tonal row
  with an edge marker, and the keyboard ring is the shared `ui::focus_ring` (shown only after keyboard use). Search hits
  read as setting name over pane name instead of "Pane > Setting" ellipses.
- **Primary/secondary page navigation.** `Item::Nav` replaces the card-wrapped segmented controls: Appearance tabs are an
  underlined strip, Customise sections are chips. Each is still one `Kind::Seg` control (`ap:tab`, `ap:sec`, one part per
  option) so IDs, mouse hits and Left/Right keys are unchanged, but it wraps onto more lines instead of truncating
  ("Custom…", "Window …" before).
- **Rows.** Stacking is decided per row (a segmented choice that does not fit moves under its label, Dux's compact
  button-group row) and long details wrap and grow their row instead of ending in an ellipsis. Segmented controls use the
  same tonal selection as navigation.

Evidence: `tests/e2e/settings-capture.py` renders the same private-Xvfb matrix (panes x light/dark x UI 100/125/200% x text
100/125/200% x normal/short). Focused tests: `ui::tests::{navigation_wraps_keeps_every_label_and_stays_one_segmented_control,
long_details_wrap_and_grow_their_row_instead_of_ending_in_an_ellipsis, page_header_context_wraps_below_the_title}` and
`icons::tests::flat_glyphs_draw_ink_without_a_badge`, `ui::tests::long_titles_wrap_and_never_lose_characters` and
`app::key_tests::arrow_keys_keep_the_page_navigation_focused_while_switching_tabs`; real keyboard/draft flows remain
`tests/e2e/dux-flow-qa.py`. Known limits (unchanged from the baseline): leaving a text field by pointer or Ctrl+F commits
it (only IPC route changes suspend a draft); at about 560 px wide with 200% text the sidebar keeps its full labels and
leaves the page roughly 250 px, where row labels and popup values can still ellipsise; theme-card metadata lines ellipsise.

## Upstream MIT notice

The stock palette excerpt above is from dux-shell; first-party widget references are from dux-plugins. Both pinned repositories carry this notice:

```text
MIT License

Copyright (c) 2025 Avenge Media LLC

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### Verified second-pass integration (2026-10-30)

Integrated as `d7fc8b4` and `21e6df0` onto the Wayland Displays branch; `38837ba` further isolates the capture harness. Integrated Settings tests: 355 passed, 0 failed, 1 ignored; formatting and strict lib/Settings clippy passed. The exact release binary passed private native keyboard/search/draft/scroll flows and light/dark, short-window and enlarged-text captures.

Installed Settings SHA-256: `7163d095de3d4b2e28260b28eb3e593d1dfba1ba06a234344bc6741d28e76db5` in both session bindirs. Guarded deployment journal: `~/.local/state/decebal/deployments/dux2-settings-20261030-174121/journal.json`; process, configuration and WM-state invariants matched. No process restart or live settings change. The redesign loads on the next Settings launch. Durable build/gate records: `~/.local/state/decebal/builds/settings-dux2-38837ba`. The narrow-window limitations above remain.
