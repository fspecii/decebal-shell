# Theming

One theme choice restyles the whole desktop: the menu bar and its menus, the dock and its menu, Spotlight, Launchpad,
the Control Center, macsettings, machaven and (through its own config) rdwm's titlebars, borders and built-in bar.
Running programs follow a change within half a second; nothing is restarted.

```
macthemectl list                       # built-in and user themes (* = current)
macthemectl apply nord                 # or "Windows 11", "macOS Light", windows-11-dark ...
macthemectl apply macos --mode light --accent '#ff9f0a'
macthemectl set mode auto              # light | dark | auto (auto follows the system, see below)
macthemectl set radius 6               # accent opacity radius font font-scale sync-rdwm; `none` unsets
macthemectl font segoe|inter|...       # font preset; list | families | status | set ROLE FAMILY[,FALLBACK...] | scale N | reset
macthemectl get [--json]               # current selection and resolved colours
macthemectl export nord > ~/.config/macshell/themes/mine.toml
macthemectl validate mine.toml
macthemectl style macos|windows|caelestia   # the whole shell look at once (theme + dock + menu bar)
macthemectl dynamic [--wallpaper PATH] [--light|--dark] [--scheme vibrant]   # Caelestia colours from the wallpaper
```
Or open **System Settings -> Theme** (`macsettings theme`).

## Where things live
| What | Where |
|---|---|
| Selection and overrides | `"theme"` key of `macshell.json` in the macshell config folder (`$MACSHELL_CONFIG`, default `~/.config/omarchy`), the file macbar already reads for `workspaces`. Other keys are kept. |
| User themes | `~/.config/macshell/themes/*.toml` (`$MACSHELL_THEMES`, else `$XDG_CONFIG_HOME/macshell/themes`). The file name is the theme id; a user file with a built-in id replaces it. |
| Built-in themes | compiled in (`src/theme/builtin/*.toml`): `macos`, `graphite`, `nord`, `dracula`, `solarized`, `windows11`, `caelestia`, `high-contrast` (all but Dracula have a light and a dark variant) |

`macshell.json`:
```json
{ "theme": { "name": "nord", "mode": "auto", "accent": "#ff375f", "opacity": 0.9, "radius": 8,
             "font": {"preset": "segoe", "mono": ["Fira Code", "Symbols Nerd Font"], "scale": 1.1}, "syncRdwm": true } }
```
`"theme": "dracula"` (a bare name) also works. Only `name` is needed. Without a `theme` key the look is the original macOS dark.
`mode`: `dark` (default), `light`, or `auto` (`$MACSHELL_COLOR_SCHEME`, else `~/.config/omarchy/desktop-styles/appearance`,
Omarchy's `light.mode` marker, GTK's `gtk-application-prefer-dark-theme`; dark when none says otherwise). A theme with only one
variant ignores the mode. A name such as `macOS Light` or `windows-11-dark` also selects the mode.
Overrides: `accent` (accent text colour is re-derived for contrast), `opacity` 0.4-1 (multiplies the translucency of menus,
dock and launchers; the menu bar stays solid), `radius` 0-24, `font` (a family, or the font object described under [Fonts](#fonts)), `fontScale` 0.8-1.4.
`syncRdwm: false` leaves rdwm's `config.toml` alone.

## Theme file
```toml
name = "Mine"                  # required
description = "..."            # optional
inherit = "nord"               # optional: start from another theme (user or built-in), override key by key
dark = true                    # only for files without [dark]/[light]: which variant [colors] is

[metrics]                      # both variants (also accepted inside [dark]/[light])
radius = 8
dock_alpha = 0.7

[font]
preset = "segoe"               # see "Fonts"; or family = "Inter" (one font for text, titles and numbers)
ui = ["Inter", "Noto Sans CJK"]   # optional per-role chains: ui, title, mono, display
scale = 1.0

[dark]                         # a variant: any colour or metric key
background = "#2e3440"
text = "#eceff4"
accent = "#88c0d0"

[light]
background = "#eceff4"
text = "#2e3440"
accent = "#3b6a85"
```
Resolution for the chosen variant: built-in fallbacks < `inherit` < `[colors]`/`[metrics]`/`[font]` < `[dark]`/`[light]` < user overrides.
Unknown keys, malformed colours and out-of-range numbers are errors (the file is then ignored and shown with its error in the gallery
and in `macthemectl list`; the shell falls back to the default theme and recovers when the file is fixed). Colours are
`#rgb` or `#rrggbb`. Only `background`, `text` and `accent` are really needed; the rest is derived.

### Colour keys
| key | used for | default |
|---|---|---|
| `background` | settings window | macOS grey |
| `sidebar` | settings sidebar | darker/lighter `background` |
| `surface` | cards, fields | `text` 7 % over `background` |
| `bar`, `bar_text` | menu bar | `background`, `text` |
| `popup` | menus, popovers, calendar | `surface` |
| `dock` | dock panel | `background` |
| `overlay` | Spotlight, Launchpad, Control Center | `popup` |
| `titlebar`, `titlebar_inactive` | rdwm titlebars | `surface`, `background` |
| `border` | rdwm window borders, bar outlines | `text` 16 % over `background` |
| `text`, `text_secondary`, `text_faint` | text; hover/separator overlays are `text` at low alpha | 62 % / 42 % mixes |
| `accent`, `accent_text` | selection, focus, switches; text on the accent | blue; black or white by contrast |
| `danger`, `ok`, `warn` | destructive items, battery, notices | system red/green/orange |

### Metric keys
`radius` (popup corner radius, px; other surfaces scale from it), `dock_roundness` (0..1; 1 = pill), `bar_style` (`flat`, the
default full-width strip; `floating`, a horizontal pill with a margin, the bar content moving in so it clears the rounded ends; or `vertical`,
a bar down the left screen edge with pills for its widgets, Caelestia's layout: see [caelestia.md](caelestia.md). A vertical bar reserves a
left strut instead of a top one, and `dock.enabled = "auto"` hides the dock under it),
`dock_alpha`, `popup_alpha`,
`panel_alpha`, `bar_alpha`, `shadow`, `outline` (alpha of the 1 px outline), `dock_icon_size`, `dock_padding`, `bar_height`
(these three are published for the layout code, which does not read them yet), `titlebar_style` (`macos`/`classic`),
`titlebar_height`, `font_family`, `font_scale`, `font_preset`, `font_ui`, `font_title`, `font_mono`, `font_display`. The table with ranges is `TOKENS` in `src/theme/model.rs`.
The dock uses `dock_alpha` as its opacity at the default `dock-settings.json` (`transparency` still scales it) and
`dock_roundness` unless `dock-settings.json` sets `roundness`.

## Fonts
A theme has four font roles, each a **chain** of family names:

| role | used for |
|---|---|
| `ui` | default text |
| `title` | settings page titles, notification titles (and, exported to rdwm, window titlebars) |
| `mono` | terminal-like text: the dashboard's performance numbers, clipboard entries |
| `display` | large numbers: the dashboard clock, the workspace card digit |

The first installed family of a chain draws the text. A character it lacks (CJK, emoji, Nerd Font / Material Symbols icons)
is drawn by the next installed family of the chain that has it, then by a built-in tail: Inter and the system sans, an emoji font,
Noto Sans CJK (the variant of your `LANG`), a Nerd Font and Noto Sans Symbols. Colour emoji (Noto Color Emoji, CBDT) are drawn in colour.

**Presets** are ready-made sets of chains. Each lists a preferred font and stand-ins, so a preset works on a machine without its font
(`macthemectl font list` shows what each one found here; the Theme pane marks them "not installed"):

| preset | look | ui chain (first installed wins) | mono |
|---|---|---|---|
| `system` | fontconfig defaults | sans-serif | monospace |
| `inter` | macOS-like (the original look) | Inter, SF Pro | SF Mono, Menlo, JetBrains Mono |
| `segoe` | Windows 11-like | Segoe UI Variable, Segoe UI, Selawik, Noto Sans | Cascadia Mono, Consolas |
| `cantarell` | GNOME | Cantarell, Adwaita Sans | Adwaita Mono |
| `rubik` | Caelestia | Rubik, Google Sans Flex, Inter, Material Symbols | CaskaydiaCove NF, JetBrains Mono |
| `jetbrains` | JetBrains Mono everywhere | JetBrains Mono | JetBrains Mono |
| `ibm-plex` | IBM Plex | IBM Plex Sans | IBM Plex Mono |
| `fira` | Fira | Fira Sans | Fira Code |
| `noto` | Noto | Noto Sans | Noto Sans Mono |
| `monospace-ui` | terminal look | JetBrains Mono, Fira Code, ..., monospace | same for every role |

Built-in themes: `macos`, `graphite`, `nord` use `inter`; `windows11` `segoe`; `caelestia` `rubik`; `dracula` `jetbrains`; `solarized` `ibm-plex`;
`high-contrast` `system`. A theme without any font key keeps the original look (`inter`). Family names are matched ignoring case and spaces
("JetBrains Mono" finds "JetBrainsMono Nerd Font"; "Noto Sans CJK" finds "Noto Sans CJK JP").

Choosing: **System Settings -> Theme -> Fonts** (a preset picker, a family picker per role with type-ahead search, a live preview line with
Latin, digits, CJK, emoji and Nerd Font icons, text size under Customise), or

```
macthemectl font list                          # presets and what each resolves to here
macthemectl font segoe                         # a preset: replaces earlier role overrides
macthemectl font set ui Inter,"Noto Sans CJK","Noto Color Emoji"   # a chain for one role (kept in front of the preset's chain)
macthemectl font set mono "Fira Code"          # ROLE: ui title mono display; `none` goes back to the preset
macthemectl font status | families | scale 1.1 | reset
```
The choice is stored in `macshell.json`: `"theme": {"font": {"preset": "segoe", "ui": ["Inter", "Noto Sans CJK"], "mono": "Fira Code", "title": ..., "display": ..., "scale": 1.1}}`.
A role value is a family or a list. The user's preset replaces the theme's own font keys; a user role chain goes in front of the preset's.
Old files keep working: `"font": "Inter"` (a string) and `"fontScale"` are the single font of text, titles and display numbers
(`macthemectl set font Inter`), and `[font] family` / `font_family` in a theme file is the same.

Layers, lowest first: preset (the user's, else the theme's, else `inter`) < theme `font_ui`/... < theme `family` < user `font` string < user role chains.
Installed fonts are read once per process with one `fc-list` (plus two `fc-match` for the generic names), never per frame;
faces are memory-mapped and opened on the first glyph that is not cached. Latin fonts are drawn with fontdue as before; fonts with more than 6000 glyphs
(Noto CJK would take 320 MB) and variable fonts (their weight axis is applied) are rasterised glyph by glyph with ttf-parser and tiny-skia.
Newly installed fonts are noticed by a restart of the program (the Theme pane rescans every 30 s).

## Shell styles: macOS, Windows, Caelestia
`macthemectl style NAME` (or **Shell style** at the top of the Theme pane) sets the theme, the dock style and the menu bar together:

| style | theme | dock (`dock-settings.json`) | dock shown (`macshell.json` `dock.enabled`) | menu bar (`macshell.json` `bar.enabled`) |
|---|---|---|---|---|
| `macos` | macOS | `style: mac`; `roundness` removed so the theme's `dock_roundness` rules | `true` | `true` |
| `windows` | Windows 11 | `style: windows`, `windowsAlign: center` (the Windows 11 taskbar) | `true` | `"auto"`: hidden while the taskbar is up |
| `caelestia` | Caelestia | `style: mac`; `roundness` removed (the theme's pill) | `"auto"`: hidden while the theme's bar is vertical | `true`, drawn as a vertical bar on the left edge |

Accent, translucency, radius, font and text size overrides are kept; wallpaper colours stay only for `caelestia`. `macthemectl style`
without a name prints the current preset (`custom` when the theme and dock do not match one). Nothing else in the two JSON files is touched.

## The Caelestia theme
Ported from [caelestia-shell](https://github.com/caelestia-dots/shell): the dark palette is its shipped default scheme
(`services/Colours.qml`, the rose Material You scheme) role for role, the light palette is the same seed's tonal-spot light scheme,
generated by the same code that makes the wallpaper palettes. Role mapping (`Roles::tokens` in `src/theme/material.rs`):

| theme colour | Material role |
|---|---|
| `background` | surface |
| `sidebar`, `titlebar_inactive` | surface container low |
| `surface`, `bar`, `dock` | surface container |
| `popup`, `overlay`, `titlebar` | surface container high |
| `text` / `text_secondary` / `text_faint` | on-surface / on-surface-variant / outline |
| `border` | outline variant |
| `accent` / `accent_text` | primary / on-primary |
| `danger`, `ok`, `warn` | error, success, tertiary |

Shape: popup radius 20 (caelestia's large rounding), `dock_roundness = 1` (a pill dock, for when the dock is switched back on), `bar_style = "vertical"` (the vertical bar of caelestia-shell, [caelestia.md](caelestia.md)), a
1 px outline at 7 %, titlebars 32 px. Font: the `rubik` preset (Rubik, then Google Sans Flex, Inter; Material Symbols icons; a Nerd Font monospace, see [Fonts](#fonts)). Caelestia has no window titlebars of its own; rdwm's follow the theme as usual.

## Colours from wallpaper
```
macthemectl dynamic                              # the wallpaper on screen (rdwm state, [wallpaper] image, omarchy current/background)
macthemectl dynamic --wallpaper ~/Pictures/a.jpg --light --scheme vibrant
macthemectl dynamic --seed '#2a9d8f'             # a colour instead of an image
macthemectl dynamic refresh                      # re-read the wallpaper (after changing it by other means)
macthemectl dynamic off
```
Or the **Colours from wallpaper** switch (and a Palette picker) in the Theme pane. It selects the Caelestia theme and replaces its colours with a
Material You palette made from the wallpaper's dominant colour:

* The seed: the wallpaper is shrunk to 96 px, quantised, and Material's scoring picks the colour (hue popularity smoothed over +-15 degrees, then chroma).
  A grey wallpaper keeps a hint of its average colour or the default rose.
* Tonal palettes: tone is exactly CIE L* (so tone 80 on tone 20 always has the same contrast), hue and chroma come from OKLCH with Material's chroma
  numbers rescaled (calibrated on caelestia's default scheme). Colours that do not fit sRGB at their tone lose chroma, never tone.
* Variants: `tonalspot` (Material's default, pastel), `vibrant` (high chroma, rotated secondary/tertiary hues), `neutral` (nearly grey), `content` (the wallpaper's own chroma).
  caelestia's other variants (expressive, fidelity, fruit salad, rainbow, monochrome) are not generated.
* Roles use Material's tone mapping for dark and light; every `on-*` role reads on its container at 4.5:1 or better (a unit test sweeps seeds and variants).
* Stored as `"dynamic": {"seed": "#2a9d8f", "scheme": "tonalspot", "wallpaper": "...", "follow": true}` in the `theme` object of `macshell.json`.
  The image is decoded once, by the command that sets it; all programs derive the palette from the seed. Other themes ignore it.

Following wallpaper changes: a wallpaper set from machaven, or from the Wallpaper pane of macsettings, re-derives the seed by itself (`follow`).
A change made by other tools (`rdwmctl wallpaper`, `omarchy theme bg set`, ...) is picked up by running `macthemectl dynamic refresh`
(a no-op while the feature is off), e.g. from the script that sets the wallpaper. `--no-follow` pins the seed.

## What happens on apply (`macthemectl apply`, the Theme pane)
1. Resolve the theme (nothing is written if it is broken).
2. Apply the matching keys to rdwm's `config.toml` in memory (`[colors]` focused/normal/urgent, `[decoration]` style, height, radius,
   `focused_*`/`unfocused_*` bg/fg/border, `inactive_button_color`, `font` (the `title` role's first installed family), `fallback_fonts` (the
   rest of its chain, plus Noto Sans CJK and an emoji font when installed), `font_size`, `[decoration.bar]` norm/sel/urgent/font_size),
   validate with `rdwmctl check-config` (an rdwmctl that is too old counts as syntax-only), keep comments and every other key,
   make the one-time `config.toml.bak-macsettings`, write atomically.
3. Write the selection to `macshell.json` (atomic).
4. Reload rdwm once over its socket (skipped when it is not running).
If step 1 or 2 fails nothing is written. rdwm needs no changes: all these keys already exist in its `[decoration]` schema.
Keys the theme owns are overwritten on every apply, a theme whose title role has only generic fonts removes `[decoration] font` and `fallback_fonts`.

## Programs and how they follow
Each process runs one 500 ms `stat` poll (`theme::live::start_watcher`) of `macshell.json`, the selected user theme file (and its
parent) and the system scheme files (auto mode). On a change it rebuilds the theme, bumps `theme::generation()` and wakes the event
loop; `tick` redraws. Fonts rebuild their faces only when the font plan changed. Drawing code reads the theme at paint time
(`crate::theme::th()`), never at start-up.

Without a compositor, ARGB surfaces show their premultiplied colour (translucent light surfaces look greyer). Set the
`Translucency` slider to 100 % to avoid that.

## For code
`theme::th()` returns `Arc<Theme>`; colours are `Rgb` fields (`th.accent`, `th.popup`...) with helpers: `th.ink(a)` (text colour at
alpha a: hover, hairlines), `th.popup_c()`, `th.accent_c()`, `th.accent_text_c()`, `th.outline_c()`, `th.r(factor)` (radius).
`theme::take_changed()` is true once per change. `theme::live::publish_theme(t)` sets a theme without files (previews, tests).

## Tests
`cargo test` (parsing, validation, inherit, variants, overrides, contrast >= 4.5:1 for text on every surface of every built-in
variant, live reload, rdwm edits, the settings pane), `tests/e2e/theme.sh` (private Xvfb + rdwm + macbar + macdock +
macsettings: macthemectl, user theme file edits, the UI, pixel checks, screenshots in `$SHOTS`),
`THEME_SHOTS=dir cargo test --lib -- --ignored launchpad_in_three_themes`, `tests/e2e/style.sh` (the three shell styles on the real macbar and
macdock, pixel checks per style, the pill geometry, wallpaper colours, and `BASELINE_BIN=dir` to prove the macOS style is pixel-identical to a build from before the Caelestia theme).
`tests/e2e/vbar.sh` runs the vertical Caelestia bar for real (strut, geometry, workspace pills, popups, launching the widgets, the dock hiding, switching styles both ways).
The pieces are unit-tested too: `menubar::vbar` (geometry, hit-testing, clicks), `backend::x11` (left placement and strut), `theme::style`, `config`.
