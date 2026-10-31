# GNOME theme ports

Two new native theme packages, researched from primary upstream sources on 2026-10-31:

| Package | What it reproduces | Dark / light |
|---|---|---|
| [`themes/gnome-adwaita`](../themes/gnome-adwaita) | **GNOME 51 / libadwaita 1.10** stock Adwaita: colours, 32 px top panel, 47 px headerbars, 15 px window corners, Adwaita Sans, quick-settings card | libadwaita light / dark |
| [`themes/gnome-catppuccin-dock`](../themes/gnome-catppuccin-dock) | A popular GNOME **rice**: Catppuccin Mocha/Latte + Blur my Shell panel + Dash to Dock bottom dock + Just Perfection trimming + Vitals-style readouts, Inter, Nerd-Font mono, Papirus | Mocha / Latte |

The older `gnome-adwaita-like` package (owned elsewhere) is untouched; it is a loose "inspired by" palette. These are measured ports.
Unaffiliated with the GNOME Project or Catppuccin. This work owns only the new `themes/gnome-*` packages, this document,
`tests/gnome_theme_ports.rs`, `tests/e2e/gnome-theme-ports-native.py`, `tests/fixtures/gnome-theme-ports/` and
`docs/gnome-theme-ports/`; the parent later extended the scope to the genuine GNOME quick-settings Control Center, adding `src/controlcenter/gnome.rs`
(+ `gnome/tests.rs`), `src/menubar/gnome_cc_tests.rs`, `tests/e2e/gnome-cc-native.py` and these narrow edits: the `Style` enum and `STYLE_NAMES`
(`controlcenter/settings.rs`, which feeds the theme validator's allowlist), routing in `controlcenter/mod.rs` (module, `panel_width`, `layout_styled`, `look`,
`draw`, `Draw::Toggle`), the `control_center.style` help text in `theme/engine/schema.rs`, a test-only `theme::live::thread_theme`, and one
`#[cfg(test)] mod gnome_cc_tests;` line in `menubar/mod.rs`. Nothing here registers the packages in `src/theme/builtin.rs`, the Makefile, `docs/themes.md`,
`data/wallhaven-curated.json` or the universal panel code; those are listed under [Parent relay](#parent-relay).

## Status

**Not complete, and not claimed to be.** The native GNOME quick-settings Control Center look exists (`control_center.style = "gnome"`, below) and both packages use it, behind the
`min_shell_version = "0.3.0"` floor. What exists: two validated packages, pinned research, native previews and private native captures.
What gates completion (all outside this change): the universal four-edge panel/applet schema and renderer, an Overview/launcher style and
GNOME-shaped quick settings (blockers 1, 2, 3, 4, 8, 9, 10), rdwm decoration captures, the parent's wallpaper verification and merge, and the
native-fidelity review. Until then these are honest partial ports.

## Research record and pinned revisions

All values were read from the upstream sources at the exact revisions below (GitLab/GitHub API, raw files). Pinned revisions, not
branches, so a later upstream restyle cannot silently change what the package claims.

| Project | Revision | Licence | Files read |
|---|---|---|---|
| [libadwaita](https://gitlab.gnome.org/GNOME/libadwaita) | tag `1.10.0` (2026-09-14) `c9ab118612eee92324143783c4641c98eaded3f0` | LGPL-2.1+ | `src/stylesheet/_colors.scss`, `_palette.scss`, `_common.scss`, `widgets/_header-bar.scss`, `_buttons.scss`, `_window.scss`, `_popovers.scss`, `_menus.scss`, `_switch.scss`, `_sidebars.scss`, `_toolbars.scss` |
| [GNOME Shell](https://gitlab.gnome.org/GNOME/gnome-shell) | tag `51.1` (2026-10-14) `33da06854e229c256afd5872fedd83c28cbfa176` | GPL-2.0+ / `gnome-shell-sass` LGPL-2.1+ | `data/theme/gnome-shell-sass/_colors.scss`, `_default-colors.scss`, `_common.scss`, `_drawing.scss`, `widgets/_panel.scss`, `_quick-settings.scss`, `_dash.scss`, `_popovers.scss`, `_overview.scss`, `_calendar.scss`, `_message-list.scss`, `_osd.scss`; `gnome-shell-{dark,light}.scss`; `js/ui/main.js` (variant selection) |
| [catppuccin/palette](https://github.com/catppuccin/palette) | `07d02aa110ef9eb7e7427afca5c73ba9cf7f8ebd` (palette 1.8.0) | MIT | `palette.json` |
| [ubuntu/dash-to-dock](https://github.com/ubuntu/dash-to-dock) | `2dfae3474d6984b566ef0c3dab1524a675bbba48` | GPL-2.0+ | `schemas/org.gnome.shell.extensions.dash-to-dock.gschema.xml`, `_stylesheet.scss` |
| [aunetx/blur-my-shell](https://github.com/aunetx/blur-my-shell) | `f13bdcf36be5e1d8b62203c77face3303f0860af` | GPL-3.0 | schemas (brightness 0.6 / sigma 30 defaults) |
| [jrahmatzadeh/just-perfection](https://github.com/jrahmatzadeh/just-perfection) | `a46f18957422210411f5675e462c73d47e79266e` | GPL-3.0 | repo metadata and options (panel/Activities) |

Installed system versions on the build host agree: `libadwaita 1.10.0`, `adwaita-fonts 51.0`, `adwaita-icon-theme 51.0`.

### Why this rice

Popularity from the GitHub API on 2026-10-31: `catppuccin/catppuccin` 19.9k stars (MIT), `ubuntu/dash-to-dock` 4.4k, `aunetx/blur-my-shell`
2.4k; competing GNOME themes: WhiteSur 9.4k (macOS look, already covered by Decebal's macOS packages), Nordic 2.7k (GPL; Decebal has Nord),
Colloid 1.5k (GPL). A web search for r/unixporn did not surface a ranked "most popular GNOME rice", so this is stated as the recurring
public-dotfiles combination (Catppuccin Mocha + Blur my Shell + Dash to Dock + Just Perfection), **not** as a measured #1. Catppuccin's
palette is MIT, so its values are usable with attribution; the GPL extensions are only measured, never copied.

### Licensing posture

* No upstream stylesheet text, SVG, icon, font or wallpaper is copied. Only measured numbers and colour values, which are facts, are
  used; each manifest line cites its source in a comment and `README.md`/`ATTRIBUTION.md` carry the pins.
* The manifests, tests and docs are MIT (this repository). `LICENSE.txt` is in each package.
* Catppuccin's MIT notice is reproduced in `gnome-catppuccin-dock/ATTRIBUTION.md`.
* The only bundled third-party file is `fonts/MaterialSymbolsRounded.ttf` (Apache-2.0, Google; byte-identical to the copy already shipped by
  `dux-lavender`, test-enforced), because the only item-list bar paints its glyphs from it. Git stores the identical blob once.
* System dependencies, not shipped: Adwaita Sans/Mono (`adwaita-fonts`, SIL OFL), Adwaita icons (LGPL-3/CC-BY-SA), Inter (OFL),
  JetBrains Mono Nerd Font, Papirus (GPL-3).

## Token derivation (gnome-adwaita)

A package token is one opaque sRGB colour; libadwaita/Shell values are often alpha or computed, so they are pre-blended over the surface they
sit on. The exact numbers are pinned by `adwaita_tokens_and_geometry_match_the_pinned_upstream_values`.

| Token | Light | Dark | Upstream source |
|---|---|---|---|
| `background` | `#fafafb` | `#222226` | `window_bg_color` |
| `sidebar` | `#ebebed` | `#2e2e32` | `sidebar_bg_color` |
| `surface` | `#ffffff` | `#343437` | `card_bg_color` (dark: `rgb(255 255 255 / 8%)` over window) |
| `titlebar` / `titlebar_inactive` | `#ffffff` / `#fafafb` | `#2e2e32` / `#222226` | `headerbar_bg_color` / `headerbar_backdrop_color` |
| `popup` | `#ffffff` | `#36363a` | `popover_bg_color` |
| `bar` / `bar_text` | `#fafafb` / `#222226` | `#000000` / `#ffffff` | Shell `$panel_bg_color` / `$panel_fg_color` (Shell 51.1 ships a *light* panel in light style) |
| `dock` | `#38383b` | `#38383b` | Shell `$system_overlay_bg_color`: the dash is always dark |
| `overlay` | `#f3f3f5` | `#28282c` | libadwaita `--overview-bg-color` |
| `border` | `#dcdcde` | `#434347` | `currentColor` at `--border-opacity` 15 % |
| `text` | `#323237` | `#ffffff` | `window_fg_color` (`rgb(0 0 6 / 80%)` over window in light) |
| `text_secondary` | `#78787b` | `#9b9b9d` | Shell `$insensitive_fg_color` (60 % / 50 % foreground mix) |
| `text_faint` | `#969699` | `#909092` | `--disabled-opacity` 50 % |
| `accent` / `accent_text` | `#3584e4` / `#ffffff` | same | `accent_bg_color` (blue, `blue_3`) as a fill / `accent_fg_color` |
| `danger` | `#c30000` | `#ff938c` | `destructive_color`: `red_3`/`red_4` through the standalone-colour oklab clamp (`L ≤ 0.5` light, `L ≥ 0.85` dark) |
| `ok` | `#007c3d` | `#78e9ab` | `success_color`: `green_4`/`green_5` clamped the same way |
| `warn` | `#905400` | `#ffc252` | `warning_color`: `yellow_5` / `#cd9309` clamped the same way |

`danger/ok/warn` use the text-safe standalone colours because Decebal reads those tokens mostly as text and glyph colour. The accent
stays the fill colour (it carries `accent_text`), so on dark the focus ring is `#3584e4` rather than libadwaita's lightened `#81d0ff`.
Single-valued tokens cannot express both; see blocker 11.

## Geometry (measured)

| Quantity | Value | Source | Where declared |
|---|---|---|---|
| Panel height | 32 px (`2.2em` at 11 pt = 14.67 px/em) | Shell `_panel.scss` `$panel_height` | `[bar] height = 32` |
| Panel buttons | fully round (`999 px`), 3 px transparent border, hover fill 17 % fg | `_drawing.scss` `panel_button` | `[bar] radius = 16` (clamped to half the face) |
| Headerbar | 47 px (46 + 1 px shade), 6/7/7/7 px padding | libadwaita `_header-bar.scss` | `[decoration] height = 47` |
| Window radius | 15 px (`$button_radius` 9 + 6) | libadwaita `_common.scss` | `[decoration] radius = 15` |
| Button radius / min size | 9 px / 24 px (+ 5 px padding) | libadwaita `_buttons.scss` | not a manifest key |
| Popover radius | 20 px (`$modal_radius` 16 × 1.25) | Shell `_popovers.scss` | `[metrics] radius = 20` |
| Quick-settings menu | padding 18, radius `16 × 2.25` = 36 px, toggle `12em` × 3 icon heights, 12 px grid gaps | Shell `_quick-settings.scss` | `[control_center] style = "gnome"` (floor 0.3.0) |
| Dash | radius 16 + 12 = 28, padding 12, item spacing 2, icon 48 | Shell `_dash.scss`; Dash to Dock `_stylesheet.scss` | `[dock] size = 48`, `dock_padding = 12`, `dock_roundness = 0.8` |
| Base font | 11 pt Adwaita Sans (GNOME 48+), mono Adwaita Mono | Shell `$base_font_size`; `adwaita-fonts` | `[font]` chains |
| Window shadow | `0 0 14px 5px rgb(0 0 0 / 15 %)` | libadwaita `_window.scss` | `[metrics] shadow = 0.9` (approximate scale) |

## What each manifest declares (all keys valid today)

Both packages validate clean with the current engine (`decthemectl theme validate` prints `ok` and zero warnings; the test asserts an
empty diagnostic list, so no pending/ignored key hides). The deliberate declarations:

| Area | gnome-adwaita | gnome-catppuccin-dock |
|---|---|---|
| Bar | `style="dux"`, `position="top"`, 32 px, flat band (`margin 0`, `band_radius 0`) | same geometry, translucent (`bar_alpha 0.6`, blur) |
| Items (`left` / `center` / `right`) | `launcherButton, workspaceSwitcher` / `clock, notificationButton` / `network, audio, battery, controlCenterButton` | `workspaceSwitcher, focusedWindow` / `clock, notificationButton` / `systemTray, cpuUsage, memUsage, network, audio, battery, controlCenterButton` |
| Dock | **disabled** (stock GNOME shows the dash only in the Overview); Dash metrics declared for when the user enables it | enabled, bottom, floating capsule, 48 px, `dock_alpha 0.8`, no magnification |
| Titlebar | rdwm `adwaita`, 47 px, radius 15, centred title | same |
| Window appearance | `border_px 0` (GNOME has no border), gaps 12 | gaps 8, `border_px 2`, focus ring = accent (default) |
| Window arrangement | **not declared** (see [Window policy](#window-policy)) | not declared |
| Control Centre | enabled, `position="auto"`, `style="gnome"` (needs 0.3.0), all seven tile groups | same |
| Hot corner | `top_left = "overview"` (GNOME's default) | same |
| Icons | `Adwaita`, fallback `hicolor`, `monochrome_tint` (symbolic follows foreground) | `Papirus-Dark`, fallback `Adwaita`, tint |
| Fonts | Adwaita Sans → Cantarell → Inter → Noto Sans; mono Adwaita Mono | Inter → Adwaita Sans → Noto Sans; mono JetBrainsMono Nerd Font → Adwaita Mono |
| Desktop widgets | `desktop.widgets = "none"` (GNOME has none) | none (see blocker 15) |
| Wallpaper | none in the package (the separate Wallpaper settings own the image; follow-theme and match-colours stay the user's choice); candidates below feed the central curated mapping | same |
| Previews | none baked in: gallery and `decthemectl theme render` draw them natively from the package | same |

## Window policy

Stock GNOME floats windows, and the rice usually adds a tiling extension. The user's explicit global Decebal behaviour wins over both: new
apps tile first (not fullscreen), normal drag stays free, Shift snaps, auto-formation stays on for new apps. A theme may set decoration,
gaps and border (appearance) but must not silently override that preference, so neither manifest declares `[rdwm] layouts`, `mfact`,
`nmaster` or `smart_gaps`. This is a **deliberate window-behaviour difference from stock GNOME** (not a fidelity claim), enforced by
`themes_never_override_the_global_tiling_preference`, which applies both packages over a config with custom layouts/mfact/nmaster and
asserts they survive.

## Proposed universal-panel defaults (not committed in the manifests)

The universal-panel owner's draft contract (`[bar] position` default edge, `[bar] applets` optional enabled list, `[dock] position`, user
overrides winning across palette/theme changes, applet keys `launcher, app_menu, workspaces, window_title, tasks, layout, media, clock,
weather, tray, clipboard, cpu, memory, notifications, stay_awake, agents, lock_keys, volume, network, bluetooth, battery, brightness,
search, dashboard, control_center, power, show_desktop`) is **not merged**, so no such field is in any manifest here. The current
`left/center/right` Dux lists are retained as the preserved aliases. The intended defaults, to be adopted once the schema lands, are:

| Applet | gnome-adwaita | gnome-catppuccin-dock | GNOME basis |
|---|---|---|---|
| `launcher` | on | off | Activities button (opens the Overview); Just Perfection hides it in the rice |
| `workspaces` | on | on | Shell 51 Activities workspace dots |
| `window_title` | off | on | removed from stock Shell in 45 (app menu); the rice adds a title extension |
| `clock` | on | on | date menu, centred |
| `notifications` | on | on | the clock's message indicator / do-not-disturb |
| `tray` | off | on | stock GNOME has no tray; AppIndicator extension in the rice |
| `cpu`, `memory` | off | on | Vitals-style readouts, rice only |
| `network`, `volume`, `battery` | on | on | the status capsule of the quick-settings button |
| `control_center` | on | on | Quick Settings |
| `bluetooth`, `brightness`, `power` | off (inside Control Centre) | off | they live in the quick-settings menu, not as panel buttons |
| `weather`, `media` | off (inside the date menu / notification list) | off | |
| `search` | off | off | Overview search entry (hot corner / super key) |
| `dashboard` | off | off | the date menu already opens the native dashboard |
| `tasks`, `layout`, `clipboard`, `stay_awake`, `agents`, `lock_keys`, `app_menu`, `show_desktop` | off | `stay_awake` off | optional; each remains independently togglable in Settings |

Everything the registry offers must stay togglable for both packages; "off" here is only the package default, never a removal.

### Four edges (design intent, not implemented)

GNOME has no vertical panel; the placements below are the researched look to hand to the universal renderer, flagged as derived rather than
measured:

* **Top (native, measured):** the panel above.
* **Bottom:** same 32 px thickness and colours; the clock stays centred; popups anchor upward. Dash to Dock already sits bottom in the rice,
  so the dock and the bottom panel must stack rather than overlap (dock offset by the panel thickness).
* **Left / right:** keep text and glyphs **upright** (never rotated). Thickness must grow beyond 32 px for upright `Sat 31 / 01:52`
  stacks (suggest the dock-item pitch, 48 px, i.e. `dash-max-icon-size`); order top→bottom follows left→centre→right. Round buttons stay
  round (`999 px`). Dash to Dock's side docks (left is the Ubuntu default) show the dock-on-the-side look, with the same 28 px capsule
  radius.
* In every edge the Control Centre opens beside its button (`position = "auto"`), which is already how the manifests are written.

## GNOME quick-settings Control Center (`control_center.style = "gnome"`)

Implemented as `src/controlcenter/gnome.rs`: an independent native renderer of GNOME Shell 51.1's quick-settings menu, selectable as the Control
Center **Look under any theme** (Settings → Control Center → Look, `decebal.json` `control_center.style`, or a package's `[control_center] style`).
Other looks (`auto`, `card`, `fluent`, `dux`) are unchanged. It reuses the common controller entirely: the same `Snapshot`, the seven tile groups
and the same `Target` actions, so Wi-Fi, Bluetooth, volume, brightness, Focus/DND, dark mode, Night Light, media, Stay Awake, Screenshot, Record,
the decutil toggles, Notifications and Settings all stay functional. A missing provider is hidden or marked "Unavailable" and is never clickable;
nothing is faked.

### Measured geometry (gnome-shell 51.1 `33da068`, px at 100 % text scale, `1em` = 11 pt = 14.67 px)

| Quantity | Value | Source |
|---|---|---|
| Panel padding / radius | 18 px / 36 px (`$modal_radius` 16 × 2.25) | `_quick-settings.scss` `.quick-settings` |
| Grid | 2 columns (`N_QUICK_SETTINGS_COLUMNS = 2`), 12 px row and column gaps | `js/ui/panel.js`, `.quick-settings-grid` |
| Toggle | `12em` wide (176) × 3 icons high (48), fully round; icon 16, left padding 15, icon-to-text 9, right padding 12 | `.quick-toggle` |
| Split toggle (Wi-Fi) | one 176 px pill: toggle + 37 px menu button (16 px icon, 10.5 px side padding) with a 1 px separator | `.quick-toggle-has-menu` |
| Title / subtitle | bold 11 pt (`%heading`) / regular 9 pt (`%caption`); a toggle without a subtitle centres its title | `_common.scss` |
| Panel width | 2 × 176 + 12 + 2 × 18 = **400 px**; follows the text scale (`352·k + 48`) | derived |
| System row | battery readout then icon buttons Screenshot, Settings; button = 16 px icon + 2 × 10.5 px padding (37 px) | `status/system.js`, `.icon-button` |
| Slider row | icon + 6 px + track; row = 16 px icon + 2 × 6 px padding (28 px); trough 4 px, handle 16 px, accent fill | `_slider.scss`, `.quick-slider` |
| Order | system row → output volume → brightness → grid (network, bluetooth, night light, dark style, DND …) | `js/ui/panel.js` `_addItemsBefore` |
| Colours | button = `mix(fg, bg, 12 % light / 9 % dark)`, hover/active `lighten`/`darken` 4 % / 9 %, checked = accent, menu button base `lighten(bg, 8 %)` / `darken(bg, 5 %)`, insensitive foreground 60 % / 50 % | `_drawing.scss` `button()` |
| Panel border | `1 px lighten(bg, 5 %)` dark / `darken(bg, 7 %)` light | `.popup-menu-content` |

The Shell's `$bg_color`/`$fg_color` map to the theme's `popup` and `text` tokens, the accent to `accent`/`accent_text`, so the look follows any palette.

### Common groups → GNOME presentation

| Common group | Rendered as |
|---|---|
| `connectivity` | Wi-Fi split tile (arrow opens the network editor), Bluetooth tile, a Wired tile only while a wired link is connected (as GNOME does) |
| `focus` | Night Light, Dark Style, Do Not Disturb |
| `display` / `sound` | brightness / output-volume slider rows (the sink name is a small caption: a Decebal addition, GNOME shows it in the volume menu) |
| `media` | the Shell's message card: art placeholder, title, artist, previous / play-pause / next |
| `toggles` (decutil) | Record, Inhibit Idle, Game Mode, DND pills |
| `actions` | battery readout + Screenshot, Settings, Lock and Power icon buttons, then Stay Awake and Notifications (or Record without decutil) pills; Lock and Power buttons when their programs exist |

Every tile can be switched off independently; a selection with no available control never produces an empty box (the common behaviour).

### Icons

Adwaita symbolic icons are looked up **at run time in the installed icon theme** (`XDG_DATA_HOME` and every `XDG_DATA_DIRS` entry,
`Adwaita/symbolic/{status,devices,actions,legacy,ui,categories}`), recoloured to the foreground, with no icon copied into this repository. Where the
installed Adwaita 51 lacks the name (`screenshot-recorder`, `dark-mode`, `caffeine`) or the theme is absent, the built-in glyph set stands in; battery
levels have no fallback (the percentage text remains). Traversal-looking names are refused. Tests cover recolouring, absence and fallbacks.

### Verification

* **Metrics/painter evidence, not a pixel replica.** No GNOME Shell runtime exists on this host (`gnome-shell` is not installed), so there is no
  source capture to diff against. The values above are measured from the pinned stylesheet and JS; the painter tests assert the resulting rectangles and
  the exact pixels of the derived colours. Do not read the captures as a pixel-identical GNOME replica.
* `cargo test --lib controlcenter::gnome`: 16 painter/layout tests (pinned metrics; every `Target` reachable; each group off removes exactly its controls;
  unavailable providers never clickable; state text/icons/order; 0.8-3.0× text scale with no overlap or overflow and touch areas ≥ 28 px; slider hit rows
  with 6 px extra; reading order; colour formulas; authentic icons + fallbacks; pixels in light and dark; hover; ellipsis at 200 %).
* `cargo test --lib menubar::gnome_cc_tests`: 8 tests through the real bar (open at 400 px, input region, hit-testing, drags, wheel, groups, scales, and every
  common action). The ones that click a setter, or run a status probe, only run inside `tests/e2e/gnome-cc-native.py`: private `PATH` of recording stub
  services (plus read-only `fc-list`/`fc-match` for fonts), private HOME/XDG, no display or bus, one test thread, and the script **fails if any setter reaches a
  stub**. Result: 24 passed; the stubs saw only `decnotify dnd status` once and `brightnessctl -m -c backlight` twice (probes), never a setter.
* Strict clippy on `--lib --tests` is clean for the new files; `rustfmt` clean.

Test isolation: tests that need a GNOME look use the new test-only `theme::live::thread_theme`, which overrides the theme for the calling test thread only,
because publishing a global theme from a parallel test disturbed unrelated tests (observed and fixed). Under heavy parallel load a few unrelated existing
tests (`theme::engine::apply::tests::preview_reverts_by_itself`, two `theme::wallpaper_follow` tests) failed intermittently in one of two full runs and
passed alone and on the rerun; they do not touch this code. Scratch directories must not contain "gnome" in their path: the omarchy apply test asserts that no
command line mentions it.

### Evidence (debug build `~/.cache/gnome-ports/target`, library test binary SHA-256 `31c96593510c4c4b1886afe8dc2fdec02806b1dadd3bfd0459132f3f31be990b`)

Painter output over a neutral backdrop (`nord` dark, `macos` light), 100/125/200 % text scale, all groups / sliders only / grid only, Wi-Fi hovered;
`decbar-gnome-control-center.png` is the real bar's open popup. Source hashes (SHA-256 prefixes): `gnome.rs` `58779d4e3cc873e9…`, `gnome/tests.rs` `b301328e8500b0e0…`,
`gnome_cc_tests.rs` `e09af95ec98908f5…`, `gnome-cc-native.py` `110b066bdae450d7…`, `gnome-cc-acceptance.py` `61e3258b12e58933…`.

| Capture | SHA-256 (prefix) |
|---|---|
| [`decbar-gnome-control-center.png`](gnome-theme-ports/control-center/decbar-gnome-control-center.png) | `87fff5632df61bb9…` |
| [`macos-light-100-all.png`](gnome-theme-ports/control-center/macos-light-100-all.png) | `ad49b9421c81ae38…` |
| [`macos-light-100-grid.png`](gnome-theme-ports/control-center/macos-light-100-grid.png) | `53402555d4280fb8…` |
| [`macos-light-100-sliders.png`](gnome-theme-ports/control-center/macos-light-100-sliders.png) | `5bc85300bc148f16…` |
| [`macos-light-100.png`](gnome-theme-ports/control-center/macos-light-100.png) | `a9712a2447a0ddbc…` |
| [`macos-light-125-all.png`](gnome-theme-ports/control-center/macos-light-125-all.png) | `c12f56e412528f3d…` |
| [`macos-light-125-grid.png`](gnome-theme-ports/control-center/macos-light-125-grid.png) | `26f0995b744f264e…` |
| [`macos-light-125-sliders.png`](gnome-theme-ports/control-center/macos-light-125-sliders.png) | `00ed557b6e344deb…` |
| [`macos-light-200-all.png`](gnome-theme-ports/control-center/macos-light-200-all.png) | `8016d85465e71299…` |
| [`macos-light-200-grid.png`](gnome-theme-ports/control-center/macos-light-200-grid.png) | `eed3a72b24aa6a2b…` |
| [`macos-light-200-sliders.png`](gnome-theme-ports/control-center/macos-light-200-sliders.png) | `9b8ec72d158ff398…` |
| [`nord-dark-100-all.png`](gnome-theme-ports/control-center/nord-dark-100-all.png) | `e90063d1409840d1…` |
| [`nord-dark-100-grid.png`](gnome-theme-ports/control-center/nord-dark-100-grid.png) | `fe5dedb0be07e26d…` |
| [`nord-dark-100-sliders.png`](gnome-theme-ports/control-center/nord-dark-100-sliders.png) | `ea1c5584650b7adb…` |
| [`nord-dark-100.png`](gnome-theme-ports/control-center/nord-dark-100.png) | `520258825f98edd8…` |
| [`nord-dark-125-all.png`](gnome-theme-ports/control-center/nord-dark-125-all.png) | `e7b8ea01cc9fafb1…` |
| [`nord-dark-125-grid.png`](gnome-theme-ports/control-center/nord-dark-125-grid.png) | `50d38866b25b459a…` |
| [`nord-dark-125-sliders.png`](gnome-theme-ports/control-center/nord-dark-125-sliders.png) | `cb7b36e53c05acb7…` |
| [`nord-dark-200-all.png`](gnome-theme-ports/control-center/nord-dark-200-all.png) | `d8b533bf7875befa…` |
| [`nord-dark-200-grid.png`](gnome-theme-ports/control-center/nord-dark-200-grid.png) | `bc62dddcded04376…` |
| [`nord-dark-200-sliders.png`](gnome-theme-ports/control-center/nord-dark-200-sliders.png) | `55ea6d69dd1d5fc7…` |

Raw driver output: [`test-output.txt`](gnome-theme-ports/control-center/test-output.txt), [`stub-service-calls.log`](gnome-theme-ports/control-center/stub-service-calls.log),
[`report.json`](gnome-theme-ports/control-center/report.json). **Real-window acceptance** (`gnome-cc-acceptance.py`, `decbar` SHA-256 `de240e115f0e05e0ad7696e83058efc1db382319cb078e87013adc07e9e9cbcf`): captures, the per-control
recorder lines and every measured rectangle are in [`control-center-acceptance/`](gnome-theme-ports/control-center-acceptance/) (`report.json`, 41 PNGs).

### Functional gaps closed (2026-10-31, after `38a8503`)

| Gap | Now |
|---|---|
| No Lock / Power / mute | `Target::Mute`, `Target::Lock`, `Target::Power` are common controller targets. Mute toggles the default output (`sys::toggle_mute`, optimistic state like every control); Lock runs the existing session helper (`session::command(Lock)`: `rdwm-lock`, else `loginctl lock-session`); Power opens the existing session menu (`decsession`). Both go through the dry-run aware `sys::spawn_application`. `Snapshot` gained `lock` / `power`, set by `sys::lock_command()` / `sys::session_menu()`: the GNOME system row draws a Lock or Power button **only when its program exists** and never reports success for a refused spawn. The mute button is the volume slider's icon, as in GNOME. |
| Screenshot ignored dry-run | `menubar::activate` called `launch::spawn_argv` directly; it now uses `sys::spawn_application`, so the recorder sees it and no test ever starts a real capture. |
| Keyboard, scrolling and ring were Dux-only | `Style::scrolls()` (Dux and GNOME) now keys the clipped viewport, wheel scrolling and Up/Down/Enter navigation in `menubar/mod.rs` (`cc_clipped`, `nav_key`), `overlay.rs` (every former `dux()` viewport decision; the material-font hooks stay Dux) and `ccviewport.rs` (draws with the real style and its corner radius). The overlay (`decbar --control-center`) sizes its surface to the content for GNOME instead of the 560 px card ceiling, so a short window scrolls instead of cutting the panel. |
| No distinct focus indication | `controlcenter::draw_focused` / `draw_focus`: GNOME's ring is 2 px of accent standing 3 px off the control (visible on accent-filled toggles, never confused with the hover fill); the other looks keep their inset ring. All hosts that paint the panel (menu bar, overlay, pill bar, embedded) call `draw_focused`, so a control scrolled out of the viewport gets no ring. |
| Settings label / help | "GNOME" (was "Gnome") and help text that describes the look (`settings/ccpane.rs`). |
| Packages | both packages declare `style = "gnome"` and `min_shell_version = "0.3.0"`; see the gate section below. |

### Native acceptance: the real windows, real pointer and keyboard (`tests/e2e/gnome-cc-acceptance.py`)

The driver starts a private Xvfb (a fresh display, never `:0`), a private XRender compositor and backdrop, a private HOME/XDG/runtime and no
session or system bus. It runs a **copy** of the built `decbar` next to **recording stand-ins** for every program the panel can start (nmcli,
bluetoothctl, wpctl, playerctl, brightnessctl, decnotify, omarchy, decutil, decsettings, decsession, decthemectl, rdwm-lock, loginctl,
nm-connection-editor, desktop-style) and a `PATH` that holds only those (plus read-only `fc-list`/`fc-match`/`sh`/`sleep`). Setters are not put in dry-run:
they genuinely run, but only the stand-ins, so the recorder proves which command each control starts. Input is XTEST (`xdotool`) on the private display only;
`DECBAR_DEBUG_PANEL` (a test hook in both the overlay and the bar) reports the panel rectangle, scroll, content height, every control's rectangle and the focus.
Probe lines (status reads) are filtered out of the proof: only commands that controls start count, so the earlier "status probes only" evidence is no longer the
acceptance claim.

| Scenario | What the real window did |
|---|---|
| `pointer-1080-100-dark` | opened `decbar --control-center` (400 px wide at 100 %); a real click on **each of the 22 controls** started its command (Wi-Fi `nmcli radio wifi off`, Bluetooth `bluetoothctl power off`, Network arrow `nm-connection-editor`, Focus `decnotify dnd toggle`, Night Light `omarchy toggle nightlight`, Dark Style `decthemectl set mode`, media `playerctl previous/play-pause/next`, Record/Inhibit Idle/Game Mode/DND `decutil …`, Notifications `decnotify history`, Settings `decsettings`, **Screenshot `decutil screenshot`**, **Lock `rdwm-lock`**, **Power `decsession`**, **Mute `wpctl set-mute … toggle`**, brightness `brightnessctl -q -c backlight set 30%`, volume `wpctl set-volume … 0.30`, Stay Awake `decutil idle-inhibit on`); a click outside closed it and started nothing |
| `keyboard-1080-100-dark` | Down visited all 22 controls once in reading order, wrapped; Up wrapped back; the accent ring was present around the focused control and absent around others; Enter started Wi-Fi's command and Mute's, and started **nothing** on a slider; Escape closed with nothing recorded |
| `short-1280x520-200-light`, `short-1366x600-125-dark`, `short-1024x480-100-dark` | the panel was clipped to the screen (content 1013 px in a 504 px viewport at 200 %); the wheel scrolled and clamped at both ends; keyboard focus on the last control scrolled it into view with a ring; a real click on the revealed control started its command; clicks along the clipped bottom edge started only the control actually under the point; a click beside the panel closed it with nothing started |
| `bar-1920x1080-100-dark`, `bar-1280x520-200-light` | the same panel opened from the **real menu bar's Control Center button** (the bar's own popup, not the overlay): keyboard walk with ring, clipped-viewport scrolling on the short screen, a real click on the revealed control, Escape |
| `groups-*` (macos light) | with only `sound`+`display`, `connectivity`, `focus`, `toggles`, or `media`+`actions` enabled, exactly those controls were offered, and each still answered a real click |
| `unavailable-1080-100-dark` | with every provider absent (no nmcli, bluetoothctl, wpctl, playerctl, brightnessctl, decnotify, omarchy, decutil, decsession, lock program, no battery) none of Wi-Fi, Network, Bluetooth, volume, Mute, brightness, media, Night Light, Focus, Lock, Power, Notifications or the decutil toggles was offered or clickable; Dark Style, Stay Awake, Record, Screenshot and Settings stayed, and a Screenshot click without its provider started nothing (no invented success) |
| `matrix-1080-{100,125,200}-{dark,light}` | panel width `352·k + 48` at every scale in both modes (nord dark, macos light), keyboard focus ring present |

In-crate (`cargo test --lib`): `keyboard_walks_every_control_with_a_ring_and_scrolls_the_clipped_panel` (22 stops, ring pixels, reveal on a short surface) runs everywhere;
the tests that click setters (`common_actions…` now covers Mute, Lock, Power and the Screenshot hook, `wheel_scrolls…` with the below-the-fold click) run in the private-service
driver `gnome-cc-native.py` only. The painter test checks the ring pixels for six control kinds and that hover is not focus.

### The release gate (0.3.0)

Both packages now declare `control_center.style = "gnome"` and `min_shell_version = "0.3.0"` (the release assigned to the batch; the root stays 0.2.1). A build older
than 0.3.0 refuses them for exactly that reason, and `packages_declare_the_gnome_look_behind_the_0_3_0_gate` pins both halves: on this 0.2.x tree the only validator
error is the `min_shell_version` floor (and the rest of the suite uses a copy whose floor is lowered to the build, never hiding the gate); on a 0.3.0 tree the
packages load clean. **Consequence on this branch only:** the lib test `theme::engine::shipped::every_theme_folder_is_a_valid_package_whose_id_is_its_folder`
(not touched here) reports the gate error until the branch is merged into the 0.3.0 tree.

### Branding and explicit applets: proposal against the universal contract (not committed; validated against `71e3434`)

`docs/universal-panels.md` in the universal-panel worktree (`71e3434`) defines `[branding]` (`logo`, `logo_tint`, `launcher_label`, `desktop_label`) and the optional
`[bar] applets` default list. This tree's validator rejects both ("unknown key"), so they cannot be added to the manifests until the framework merges; each block is
proposed here for the merge, with the 0.3.0 floor already in place:

```toml
# gnome-adwaita
[branding]
logo = "icon:start-here-symbolic"   # the Adwaita symbolic GNOME mark from the user's installed icon theme; falls back to the style's own logo
logo_tint = true
launcher_label = "Activities"       # the Shell's launcher; the hot-corner action `overview` already opens the Overview
desktop_label = "Desktop"           # what a bar names when no window is focused
[bar]
applets = ["launcher", "workspaces", "clock", "notifications", "network", "volume", "battery", "control_center"]

# gnome-catppuccin-dock (Activities hidden by Just Perfection)
[branding]
logo = "icon:start-here-symbolic"
logo_tint = true
desktop_label = "Desktop"
[bar]
applets = ["workspaces", "window_title", "clock", "notifications", "tray", "cpu", "memory", "network", "volume", "battery", "control_center"]
[dock]
position = "bottom"
```

`lock`/`power` are deliberately not applets here: the GNOME look puts them in the quick-settings system row. Everything else stays one switch away in Settings → Panels.
Checked against the universal worktree's committed validator (`71e3434`, a private build of its `decthemectl`, 0.3.0): both packages validate **as shipped and with the blocks above added** (`ok`), with one
substitution: that tree predates the GNOME look, so `style = "gnome"` was swapped to `"card"` in the throw-away copies. This tree's own validator rejects the blocks ("unknown key"), so they are not committed here.

### What is still not GNOME

* **Per-toggle menus.** GNOME's network list, Bluetooth devices and power-profile menus do not exist; the Wi-Fi arrow opens the network editor and Power opens Decebal's session menu
  (`decsession`) rather than the Shell's inline power submenu (Suspend, Restart…, Power Off…, Log Out…, Switch User…).
* **Not in the common model:** Power Mode, Airplane Mode, the microphone slider, remote access and the other conditional Shell toggles have no controller state to drive.
* **Slider keyboard adjustment.** Up/Down/Enter focus and activate; there is no Left/Right in the shared `NavKey`, so a slider is changed by pointer or wheel only and Enter on it does nothing (verified).
* **Pill-bar and embedded hosts.** The menu bar and the overlay scroll and navigate; the Caelestia pill rail and the Windows-taskbar embed paint the panel (with the ring) but their popups
  are not clipped, so at 200 % on a short screen they cannot scroll. That is the edge engine's popup placement (universal owner).
* **Panel wider than the screen.** The panel is `352·k + 48` px wide: below 400 px at 100 % (or 752 px at 200 %) the surface is narrower than the panel; it is placed inside the screen
  but its right part cannot be reached. Not exercised by a scenario (no such supported display).
* **Look only.** No drop shadow, pointer-arrow box, transitions or translucency; the panel is a rounded rectangle with the 1 px border.
* **No source-runtime comparison.** `gnome-shell` is not installed here, so there is no pixel diff against a real quick-settings menu; the geometry is measured from the pinned
  source, and the evidence above is the native painter and real-window behaviour. It is not a pixel replica.

### Stable Control Center API (for the universal-panel owner)

Communicated via the parent before any common popup-placement change; the GNOME look depends only on these:

* `controlcenter::layout_styled(&Snapshot, &Tiles, Style) -> (Vec<Item>, f32)` and `controlcenter::panel_width(Style) -> f32` (GNOME's depends on
  `theme::th().font_scale`, so a host must call it on every layout, as it does today).
* `controlcenter::draw(&mut Pixmap, &View, &mut Fonts, &mut SymbolCache)` with `View { origin, items, height, hover, style }`; `look(style).0` is the panel
  radius used for glass regions (36 for GNOME).
* `controlcenter::hit(&[Item], x, y) -> Option<(Target, R)>`; slider mapping is `slider_value(px, r.x, r.w)` on the **matched rectangle**
  (the GNOME slider's hit rectangle is the visible track plus 6 px above and below).
* `Item { r, draw, target }` display-list contract: a host may place the panel anywhere and may translate `r` (the Dux viewport does); GNOME adds
  `Draw::Toggle` (only the GNOME painter draws it).
* New in this revision: `Target::{Mute, Lock, Power}`; `Snapshot.{lock, power}`; `Style::scrolls()`; `controlcenter::{draw_focused, draw_focus, targets_in_order}`;
  `sys::{lock_command, session_menu}`; `ccviewport::Viewport::draw` takes the style. Hosts that paint the panel should call `draw_focused(pm, view, focus, …)` with their keyboard
  focus instead of `draw`.

### Relay to the universal-panel owner (exact blocks, for merge coordination via the root)

The Control Center work touched these host blocks; none changes panel geometry, edge placement or the surface/strut engine:

| File | Block |
|---|---|
| `menubar/mod.rs` | field `debug_panel` and its initialiser; `debug_control_panel()` (test hook); the `Open::Control` arm of `draw` (`debug_control_panel`, `draw_focused`, `cc_viewport.draw(…, style, …)`); `cc_clipped` and `nav_key` use `Style::scrolls()`; `activate` arms for `Screenshot` (now `sys::spawn_application`), `Mute`, `Lock`, `Power`; `#[cfg(test)] mod gnome_cc_tests;` |
| `menubar/overlay.rs` | new `viewport()` replacing every viewport decision that was `dux()` (`viewport_height`, `viewport_rect`, `size`, `draw`, `pointer_motion`, `pointer_button`, `scroll`, `nav_key`); `draw_viewport` takes the corner radius from `controlcenter::look(style)` and paints with `draw_focused`; the `DECBAR_DEBUG_PANEL` report lists controls and focus; `dux()` remains for the Material font hooks |
| `menubar/ccviewport.rs` | `Viewport.style`; `draw(…, style, …)`; the ring is `draw_focused` |
| `menubar/vbar.rs` (~1262), `menubar/embed.rs` (~259) | one line each: `controlcenter::draw(…)` → `controlcenter::draw_focused(…, self.cc_viewport.focus, …)` |
| `controlcenter/{mod,settings,sys}.rs`, `gnome.rs`, `settings/ccpane.rs`, `theme/live.rs` (test-only `thread_theme`) | as listed above |

When the edge engine reworks popup placement it must keep: `cc_visible_height()` (the viewport height the panel is clipped to), `cc_point()` (screen → content mapping incl. scroll), and the
`input_rects()` entry for the panel using `cc_visible_height()`; those are what keep clicks off controls that are scrolled out of view.

## Native evidence

Binary/source: branch `feat/claude-gnome-theme-ports` on top of `d8aa599`, `cargo build -j2` debug profile, private target `~/.cache/gnome-ports/target`;
binary SHA-256 (package captures): `decbar de240e115f0e05e0ad7696e83058efc1db382319cb078e87013adc07e9e9cbcf`, `decdock 6955bb219f93d3e0e7ad02766e7c9de63609821b78552423f3a71cba5158e686`, `decutil c33c3c914192ca7419504cbf7f0e2ff8f5f6432bd985ccf56da93d8ca53f2956`, `decthemectl 4d7a9bcea62daae65a6768b8baabce58f63c3c7d06972b72ac1a003ec4cdfc27`.
Package content hashes (`decthemectl theme hash`): `gnome-adwaita sha256:f806b8b0bef5ba97c23c91917da538a30df5a00c2057558f0cc6df409dc89cf4`,
`gnome-catppuccin-dock sha256:e9014b77c086051c7ff98597890aea4909e3a12e65c7916ca7b10b21212dd2de`. The packages declare `min_shell_version = "0.3.0"`; this tree is 0.2.0, so the package
captures below applied a copy whose floor was lowered to the build (only `theme.toml` differs; the driver says so). The native previews were generated before the floor was raised.

### Native previews (`decthemectl theme render`, generated from the package, fixed sample data)

| Theme | Mode | File | SHA-256 |
|---|---|---|---|
| gnome-adwaita | dark | [`gnome-adwaita-dark.png`](gnome-theme-ports/native-preview/gnome-adwaita-dark.png) | `ae88c7c663082417acbda6e9b8ff5d83cfa5df442f93e0e65037d8f0e68b5c35` |
| gnome-adwaita | light | [`gnome-adwaita-light.png`](gnome-theme-ports/native-preview/gnome-adwaita-light.png) | `a82a245740e8b60037f5f3d6e4aef5800878ddcad6e1de10134be6b55f79888d` |
| gnome-catppuccin-dock | dark | [`gnome-catppuccin-dock-dark.png`](gnome-theme-ports/native-preview/gnome-catppuccin-dock-dark.png) | `ffa7e2789f43c950124bde8801b6b218539a3f2c4da31177b16f39971429f2c5` |
| gnome-catppuccin-dock | light | [`gnome-catppuccin-dock-light.png`](gnome-theme-ports/native-preview/gnome-catppuccin-dock-light.png) | `24e49398263666d4ded473322c68c797b39a5676387061dec4612bf6eddf1bd6` |

The preview worker draws only the foreground items on the window colour (it does not paint the band, and reports "Decoration native helper
unavailable"), so it is not evidence of the band colour or titlebars.

### Real native desktop captures (`tests/e2e/gnome-theme-ports-native.py`)

Private Xvfb 1920×1080, private HOME/XDG/bus, no window manager, private XRender picom, external probes stubbed to fail, plain `#6b6f85`
backdrop window. The real `decbar` (and the `decdock` that `theme apply` starts when the package enables the dock) and `decutil control-center toggle`
run against the packages applied by `decthemectl theme apply --no-rdwm` into a private config. Asserted: panel thickness is exactly 32 px, band pixel
is the declared colour, the dock is absent for gnome-adwaita and painted for the rice, the Control Centre opens (>5000 changed pixels) in the GNOME look.
Items without data on the private host (battery, tray, workspace dots, focused window) are hidden by the bar, not faked; with every provider failing the
panel shows only what has none ("Unavailable" tiles are inert). This script only opens the panel; **pointer/keyboard/action acceptance is `gnome-cc-acceptance.py`** above.

| Theme | Mode | Desktop | Control Centre | Measured |
|---|---|---|---|---|
| gnome-adwaita | dark | [`gnome-adwaita-dark-desktop.png`](gnome-theme-ports/native-desktop/gnome-adwaita-dark-desktop.png) `12b77c514cab742c…` | [`gnome-adwaita-dark-control-center.png`](gnome-theme-ports/native-desktop/gnome-adwaita-dark-control-center.png) `3f150340c13e8b0e…` | band `(34, 34, 38)`, thickness 32 px, dock off-backdrop px 0 |
| gnome-adwaita | light | [`gnome-adwaita-light-desktop.png`](gnome-theme-ports/native-desktop/gnome-adwaita-light-desktop.png) `518fed27ee72d1fc…` | [`gnome-adwaita-light-control-center.png`](gnome-theme-ports/native-desktop/gnome-adwaita-light-control-center.png) `834b47dca12876b8…` | band `(250, 250, 251)`, thickness 32 px, dock off-backdrop px 0 |
| gnome-catppuccin-dock | dark | [`gnome-catppuccin-dock-dark-desktop.png`](gnome-theme-ports/native-desktop/gnome-catppuccin-dock-dark-desktop.png) `0525961db81c4892…` | [`gnome-catppuccin-dock-dark-control-center.png`](gnome-theme-ports/native-desktop/gnome-catppuccin-dock-dark-control-center.png) `acdb018fc85c77a7…` | band `(30, 30, 46)`, thickness 32 px, dock off-backdrop px 25214 |
| gnome-catppuccin-dock | light | [`gnome-catppuccin-dock-light-desktop.png`](gnome-theme-ports/native-desktop/gnome-catppuccin-dock-light-desktop.png) `2e415c4745390cd3…` | [`gnome-catppuccin-dock-light-control-center.png`](gnome-theme-ports/native-desktop/gnome-catppuccin-dock-light-control-center.png) `41c7275f21e91a98…` | band `(239, 241, 245)`, thickness 32 px, dock off-backdrop px 25214 |

Also captured privately with each package's recommended Wallhaven photograph as the backdrop (evidence only, **not committed**: no redistribution licence;
the Catppuccin dark run uses the rejected candidate `833qd2` purely as a backdrop). Same assertions pass; hashes of those private captures:
gnome-adwaita dark desktop `97392bf510e51039…`; gnome-adwaita light desktop `09ef936e8eb7c372…`; gnome-catppuccin-dock dark desktop `55200d717b7d584c…`; gnome-catppuccin-dock light desktop `881c8ec623a41d05…`.

What the captures show honestly: the light Adwaita panel (32 px, Activities grid, centred clock + bell, status icons) and the genuine GNOME quick-settings panel; the dark
Adwaita band is `#222226` with black faces rather than an all-black panel (blocker 4); the rice shows the translucent panel, CPU/memory readouts and a floating bottom dock.
Glyphs of the bar are Material Symbols (blocker 3). rdwm titlebars were not captured.

### Test results

All run with the host session cleared (see [Test isolation](#test-isolation-incident-and-remediation)): `cargo test -j2 --test gnome_theme_ports` 11 passed (packages valid or refused for the 0.3.0 floor only,
bundled font byte-identical, contrast, pinned tokens/geometry, palette and extension defaults, panel items, global tiling preference, apply structure, the 0.3.0/GNOME-look gate,
wallpaper provenance incl. the pending Catppuccin dark slot, native preview render); in the sandbox below: `controlcenter:: menubar:: settings::ccpane theme::live` 262 passed;
`spotlight::` 23 passed; the private-service driver 24 passed (stubs saw only `decnotify dnd status` once and `brightnessctl -m` twice).
Source hashes (prefixes): `tests/gnome_theme_ports.rs` `c61d36db77139073…`, `tests/e2e/gnome-theme-ports-native.py` `d5e56a32591b10ce…`,
`tests/fixtures/gnome-theme-ports/wallhaven-fragment.json` `4e9a4c118f733240…`.
On this 0.2.x tree `theme::engine::shipped::every_theme_folder_is_a_valid_package_whose_id_is_its_folder` (not touched here) fails because the two packages now declare 0.3.0; it passes
once merged into the 0.3.0 tree. A full `--lib` run has **not** been repeated since the isolation incident (root asked to coordinate first); an earlier full run showed only that test
plus timing-sensitive `theme::engine::apply::preview_reverts_by_itself` / `theme::wallpaper_follow` tests failing under `nice` load, which passed alone.

### Test isolation incident and remediation

My earlier `cargo test --lib` runs used this shell's environment, which carries the live `DISPLAY=:0`, session bus, `XAUTHORITY` and the real `HOME`. The spawning test is exactly
`spotlight::tests::leading_space_is_ignored_and_ctrl_enter_uses_alt`: Ctrl+Enter on the calculator result ran the real clipboard helper (`xclip -selection clipboard`) against the live
display and replaced the clipboard text with `2+2 = 4` (found by running every spotlight test alone with a recording `xclip` stub on a dead display; no clipboard content or history was read).
Other effects of those full runs on the live session cannot be ruled out: they were not isolated, so this is not a claim of no live effects.
Remediation, committed separately as `032b408` (spotlight only, production code unchanged): in unit tests `spotlight::run` records the action (`take_recorded`) instead of running it,
the test now asserts the intended Ctrl+Enter action, and a regression test shows Enter on copy/shell/URL/exec actions starts nothing. Practice since then: every Cargo job runs as
`flock --close …/slot1.lock nice -n10 cargo …` with the host session variables unset; test binaries run in a sandbox (`env -i`, dead `DISPLAY`, private HOME/XDG, invalid session and system
buses, a `PATH` of recording stubs that use shell builtins only, absolute `/usr/bin/timeout`, read-only `fc-list`/`fc-match` and harmless utilities linked through) and the spawn log is checked.

## Wallpaper candidates (independently accepted, not merged)

Wallpaper selection belongs to the separate Wallpaper settings and the central curated mapping (`data/wallhaven-curated.json`), which this
change does not edit. That separation does **not** disable the user's follow-the-theme or match-wallpaper-colours preference: the packages
simply do not set `[wallpaper]` keys. The candidates become actual follow-theme choices only when the parent merges them; none of the four
ids is shipped yet. The current curated loader **ignores `recommended_mode` and aspect metadata**, so a "fit" note cannot fix a centre-fill crop.

An independent private acceptance (`/var/tmp/gnome-wallpaper-acceptance/acceptance.json`, sha256 recorded in the fixture) re-fetched all four
from the public API and originals: SHA-256 and dimensions match, SFW, no AI-art tag, and the 16:9, 3440×1440 and portrait centre crops were viewed.
Photographic appearance does not certify capture method; licences stay unknown, so nothing is redistributed. Machine-readable provenance is in
`tests/fixtures/gnome-theme-ports/wallhaven-fragment.json`.

| Theme | Mode | Wallhaven | Subject | Resolution | Uploader | Source / attribution | Acceptance |
|---|---|---|---|---|---|---|---|
| gnome-adwaita | dark | [yx3w57](https://wallhaven.cc/w/yx3w57) | Tasman Lake **sunrise**, blue/violet/pink sky | 3840×2160 | jrmnt | SuperRare listing credits Paul Wilson (15-shot panorama, 2023-08-12; uncertified) | accepted, all crops |
| gnome-adwaita | light | [v9kw2m](https://wallhaven.cc/w/v9kw2m) | Picos de Europa lake, cloud reflections | 5280×2970 | Petunai42 | r/EarthPorn post marked OC by u/Pchabs; image byte-identical | accepted, all crops |
| gnome-catppuccin-dock | light | [d8prrm](https://wallhaven.cc/w/d8prrm) | Mount Fuji, pastel sunset, long exposure | 5120×2880 | Pc7 | none given | accepted, all crops |
| gnome-catppuccin-dock | dark | **pending** | rejected candidate [833qd2](https://wallhaven.cc/w/833qd2) (Guangzhou skyline) | 3840×2160 | arg81 | none given | **rejected** for centre-fill: the 3440×1440 crop removes the upper parts of the tallest towers |

For the Catppuccin dark slot a bounded replacement search (4 queries, 8 originals) found nothing passing every crop; the mapping stays pending
rather than reusing an unrelated or duplicate photo (the Adwaita dark photo would be a plausible Mocha fallback but is deliberately not proposed).

Rejected during first selection: `lm2l3r` and `v9dzg8` (digital art / photo-manipulation / AI-style retro illustration), `jexxqy` (too little scene).

## Framework blockers (for parent relay)

Each is a real limit of the current engine found while porting; none is hidden by a placeholder. Numbers match the manifests' comments.

1. **Activities opens the Overview; no native bar item does.** `launcherButton` opens the Applications list. Mitigated by the top-left hot
   corner (`overview`), but a bar item bound to Overview (and Overview itself shaped like GNOME's: workspace thumbnails + dash + search) is missing.
2. **Panel status capsule.** GNOME draws network/volume/battery inside one rounded button that opens Quick Settings. The bar draws each as its own
   face; Dux "joined segments" are noted as not yet exposed in `docs/native-dux-bar.md`.
3. **Panel glyphs are Material Symbols Rounded, not Adwaita symbolic.** The item-list bar paints only from that font, hence the 15 MB bundled
   font and a Material look on an Adwaita panel. A native symbolic-icon path (icon-theme SVG tinted by foreground, upright on every edge)
   is needed for fidelity.
4. **Band colour uses `background`, the item faces use `bar`.** Shell dark's panel is black (`bar`) across the whole width; the dux band paints
   `background` (`#222226`) with black faces on top (captured in the evidence). A manifest/renderer choice of the band colour is needed
   ("band follows `bar`").
5. **Panel transparency states.** Shell makes the panel transparent in the Overview, on the lock/login screens; no per-state panel exists.
6. **Window buttons.** GNOME's default layout is `appmenu:close`; the rdwm `adwaita` style draws its fixed set. `decoration.buttons`
   (side/none) and `decoration.button_order` are pending in the engine, so a "close only" or reordered layout cannot be declared.
   rdwm decorations could not be captured here (no rdwm session was started; the preview helper reports "Decoration native helper
   unavailable"), so titlebar pixels are not claimed verified.
7. **Dock.** `dock.position = left/right` is pending (and the validator warns even for `bottom`, so the key is omitted); there is no
   "overview-only" (stock dash) or autohide/intellihide mode in the manifest, and Dash to Dock's running-indicator, click-action and
   preview behaviours have no keys. The stock package therefore disables the dock instead of faking the Overview dash.
8. **Bar edges.** The dux item-list bar is top only; `bar.position` left/right/bottom is wired for the Caelestia vertical style, not for
   item lists. The four-edge requirement, upright vertical text and per-edge popup anchoring are the universal-panel owner's work.
9. **Date menu.** GNOME's calendar + notifications + media popover is approximated by the native dashboard. The Quick Settings shape is now the genuine
   `control_center.style = "gnome"` look (see its section); what remains of it is listed there (release gate, no lock/power/mute targets, no per-toggle
   menus, host scroll and keyboard routing).
10. **Overview, app grid, window picker, OSD, notification banners, lock screen** are separate native surfaces without GNOME styling hooks; only
    colour tokens reach them (`overlay` for the launcher card).
11. **One opaque colour per token and per variant.** Shell's overview/dash are always dark even in light style, but Decebal's `overlay`/`dock`
    text colour follows the variant, so `overlay` uses libadwaita's light/dark `--overview-bg-color`, not the Shell's always-dark overview;
    accent can be only fill *or* text colour (see above); there is no per-variant decoration style or icon-theme (`Papirus-Dark` fixed).
12. **Alpha derived colours** (80 % text, 15 % border, 8 % card) are pre-blended against one surface and will be off over others.
13. **Blur.** Blur my Shell's gaussian sigma 30 / brightness 0.6 maps to `glass_blur` 24 (cap) and `bar_alpha` 0.6/`dock_alpha` 0.8 (approximation,
    not equivalence). Needs a compositor (picom); none on Wayland.
14. **Icon themes.** Papirus is declared but not installed on the build host, so the rice's real icon rendering is unverified; the declared fallback
    is Adwaita. Light mode with `Papirus-Dark` relies on `monochrome_tint`.
15. **Desktop widgets.** GNOME ships none, so the Adwaita package explicitly declares an empty desktop; the rice does too. Whether a "date menu
    calendar" or Vitals card should exist as a desktop widget is a design decision not made here.
16. **Font scale.** GNOME's 11 pt base is expressed as `font.scale = 1.0`; the engine's base size relative to 11 pt was not measured, so text size parity
    is not claimed.
17. **Text selection/focus colours, scrollbars, switches** (libadwaita widgets) are drawn by the toolkit "Apply to all apps" path, not by the package.

## Parent relay

Needed from the parent/other owners; none were edited here:

* Name the release that contains `Style::Gnome` (> 0.2.1), then switch both packages to `style = "gnome"` and raise `min_shell_version` to it (proven on a
  patched copy by `packages_declare_card_today_and_are_ready_for_the_gnome_quick_settings_look`); hand the Control Center API/routing requests above to the
  universal-panel owner before any common popup-placement change.
* Register `gnome-adwaita` and `gnome-catppuccin-dock` in the install lists (`Makefile`, packaging, `docs/themes.md`), and decide whether the
  15 MB font is shared engine data instead of duplicated per package.
* Merge the three accepted wallpaper candidates (`yx3w57`, `v9kw2m`, `d8prrm`) from `tests/fixtures/gnome-theme-ports/wallhaven-fragment.json` into
  `data/wallhaven-curated.json` with the packages, and decide the Catppuccin dark photo (or aspect-aware routing) so the themes actually follow them (the lib test `shipped_recommendations_are_valid…` guards that catalogue and must be rerun).
* Add `gnome-*` entries to `tests/theme-preview-evidence.py` runs if automatic previews should be committed under `docs/theme-ports/automatic/`.
* When the universal schema merges: add `[bar] applets` and edge keys using the table above, drop the Dux aliases only after the renderer
  accepts them, rerun `tests/gnome_theme_ports.rs`, and extend the evidence to the four-edge × applet matrix.

## Reproduce

```sh
export CARGO_TARGET_DIR=$HOME/.cache/gnome-ports/target TMPDIR=$HOME/.cache/gnome-ports/tmp
cargo build -j2 --bins --tests
cargo test -j2 --test gnome_theme_ports
cargo test -j2 --lib shipped                      # package-folder and showcase invariants still pass with the new folders
cargo test -j2 --lib controlcenter::gnome menubar::gnome_cc_tests   # layout/painter + real-bar tests (setter clicks self-skip here)
# private-service run of the action tests (stub PATH, no display, one thread); BIN = the `cargo test --lib --no-run` executable:
python3 tests/e2e/gnome-cc-native.py $BIN OUT_DIR
# real decbar window, real pointer and keyboard, recording stand-ins, private Xvfb (DECBAR = the built decbar):
python3 tests/e2e/gnome-cc-acceptance.py $DECBAR OUT_DIR [scenario ...]
# every Cargo job: env -u DISPLAY -u WAYLAND_DISPLAY -u XAUTHORITY -u DBUS_SESSION_BUS_ADDRESS -u RDWM_SOCK -u SESSION_MANAGER flock --close ~/.cache/decebal-build-slots/slot1.lock nice -n10 cargo …
decthemectl theme validate themes/gnome-adwaita   # private HOME/XDG, DECEBAL_THEMES=$PWD/themes
python3 tests/e2e/gnome-theme-ports-native.py $CARGO_TARGET_DIR/debug $PWD/themes OUT_DIR [WALLPAPER_DIR]
```
