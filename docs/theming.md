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
macthemectl get [--json]               # current selection and resolved colours
macthemectl export nord > ~/.config/macshell/themes/mine.toml
macthemectl validate mine.toml
macthemectl style macos|windows|caelestia   # the whole shell look at once (theme + dock + menu bar)
macthemectl dynamic on                # "Match wallpaper": recolour the current theme from the wallpaper (off by default, see below)
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
             "font": "Inter", "fontScale": 1.1, "syncRdwm": true } }
```
`"theme": "dracula"` (a bare name) also works. Only `name` is needed. Without a `theme` key the look is the original macOS dark.
`mode`: `dark` (default), `light`, or `auto` (`$MACSHELL_COLOR_SCHEME`, else `~/.config/omarchy/desktop-styles/appearance`,
Omarchy's `light.mode` marker, GTK's `gtk-application-prefer-dark-theme`; dark when none says otherwise). A theme with only one
variant ignores the mode. A name such as `macOS Light` or `windows-11-dark` also selects the mode.
Overrides: `accent` (accent text colour is re-derived for contrast), `opacity` 0.4-1 (multiplies the translucency of menus,
dock and launchers; the menu bar stays solid), `radius` 0-24, `font` (family), `fontScale` 0.8-1.4.
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
family = "Inter"
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
`titlebar_height`, `font_family`, `font_scale`. The table with ranges is `TOKENS` in `src/theme/model.rs`.
The dock uses `dock_alpha` as its opacity at the default `dock-settings.json` (`transparency` still scales it) and
`dock_roundness` unless `dock-settings.json` sets `roundness`.

## Shell styles: macOS, Windows, Caelestia
`macthemectl style NAME` (or **Shell style** at the top of the Theme pane) sets the theme, the dock style and the menu bar together:

| style | theme | dock (`dock-settings.json`) | dock shown (`macshell.json` `dock.enabled`) | menu bar (`macshell.json` `bar.enabled`) |
|---|---|---|---|---|
| `macos` | macOS | `style: mac`; `roundness` removed so the theme's `dock_roundness` rules | `true` | `true` |
| `windows` | Windows 11 | `style: windows`, `windowsAlign: center` (the Windows 11 taskbar) | `true` | `"auto"`: hidden while the taskbar is up |
| `caelestia` | Caelestia | `style: mac`; `roundness` removed (the theme's pill) | `"auto"`: hidden while the theme's bar is vertical | `true`, drawn as a vertical bar on the left edge |

Accent, translucency, radius, font and text size overrides are kept, and so is Match wallpaper (it works with every style). `macthemectl style`
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
1 px outline at 7 %, titlebars 32 px. Font: Google Sans Flex (caelestia ships it in `assets/google-sans-flex`; install it as a system font),
falling back to Inter like every theme. Caelestia has no window titlebars of its own; rdwm's follow the theme as usual.

## Match wallpaper
An optional layer over **any** theme (macOS, Windows 11, Caelestia, Nord, your own files ...): the colours of the wallpaper recolour the accent and,
if you want, the surfaces. Off by default; with no `dynamic` key, or `"enabled": false`, nothing changes. Turn it on in **System Settings -> Theme ->
Match wallpaper** or from a terminal:
```
macthemectl dynamic on [--scheme S] [--surfaces K] [--strength N] [--contrast N] [--mode M] [--source X]
macthemectl dynamic off                          # back to the theme's own colours; the settings are kept
macthemectl dynamic status [--json]              # settings, the picture's colours, the resulting accent
macthemectl dynamic scheme vibrant               # tonal-spot | vibrant | expressive | monochrome | neutral | content | palette
macthemectl dynamic surfaces tint                # accent | tint | full
macthemectl dynamic strength 60                  # 0-100 %, how far surfaces move (tint, full)
macthemectl dynamic contrast 20                  # -100..100 %, Material's contrast level
macthemectl dynamic mode wallpaper               # light/dark follow the picture's brightness (keep = the Mode setting rules)
macthemectl dynamic source wallpaper|~/a.jpg|'#2a9d8f'   # what rdwm shows | a fixed image | a fixed colour
macthemectl dynamic refresh                      # read the wallpaper now (a no-op while off)
macthemectl dynamic preview [--wallpaper P] [--scheme S] [--json]   # show the palette without applying anything
macthemectl watch [--interval MS] [--once]       # follow wallpaper changes (see below)
macthemectl dynamic --wallpaper a.jpg --scheme vibrant --light      # the older form: Caelestia with a full palette from a.jpg
```

**What it recolours** (`surfaces`):

| value | effect |
|---|---|
| `accent` | only the accent (and the text on it); the rdwm focus border follows because it is the accent |
| `tint` (default) | the accent, plus backgrounds, bars, popups, borders and text tinted towards the wallpaper: the theme keeps its own lights and darks (a white Windows 11 stays white, Nord stays dark) and gains a wallpaper-coloured cast. Lightness is untouched; hue is pulled to the palette's neutral hue and chroma towards a small goal, both by `strength` (0 = just the accent). |
| `full` | each colour blends into the Material palette of the wallpaper (`strength` 100 % replaces them: the Caelestia look on any theme). Caelestia starts here. |

A colour you pick by hand (`macthemectl apply --accent`, the accent swatches) wins over the wallpaper's accent. Metrics (radius, translucency,
fonts, bar layout) are never touched. Text keeps at least the contrast it had on every surface (or 4.5:1, whichever is lower): a unit test sweeps 12 hues x
4 themes x 2 variants x 3 surface modes x 3 schemes.

**Schemes** (`scheme`), the Material You families of matugen / material-color-utilities plus a pywal-like pick:

| scheme | what it does |
|---|---|
| `tonal-spot` | Material's default: a soft pastel accent in the wallpaper's hue |
| `vibrant` | high chroma; secondary and tertiary hues rotate away |
| `expressive` | the accent hue is turned 240 degrees from the wallpaper's, with strongly rotated companions |
| `monochrome` | greys only (works even on a colourless wallpaper) |
| `neutral` | nearly grey, a hint of the wallpaper |
| `content` | keeps the wallpaper's own saturation (Material's "fidelity") |
| `palette` | pywal / wallust style: primary, secondary and tertiary are the wallpaper's own most distinct colours |

**How the colours are found** (`src/theme/extract.rs`, `material.rs`): the image is decoded and shrunk to a 96 px thumbnail (the only expensive step: about 100 ms
for a 4K PNG, once). The seed is Material's scoring (hue popularity smoothed over +-15 degrees, then chroma). For `palette` a deterministic k-means in OKLab
(k = 6, farthest-point start) adds up to two more clusters at least 28 degrees of hue away from the seed and each other. The mean CIE L* tells light
pictures from dark ones. Tonal palettes work on exact CIE L* tones (tone 80 on tone 20 always has the same contrast) with OKLCH hue and chroma; colours outside
sRGB lose chroma, never tone. A picture without colour (grey, black, white: no pixel group with real chroma) is flagged `grey`: the theme keeps its
own colours (except with `monochrome`), rather than inventing an accent.

**Where it lives.** `"dynamic"` inside the `theme` object of `macshell.json`:
```json
{ "theme": { "name": "nord", "dynamic": { "enabled": true, "scheme": "tonal-spot", "source": "wallpaper", "surfaces": "tint",
   "strength": 0.5, "contrast": 0, "mode": "keep",
   "seed": "#2a9d8f", "accents": ["#e07a3a"], "tone": 38.2, "image": "/home/me/Pictures/a.png" } } }
```
The settings come first; `seed`, `accents`, `tone` (and `grey`) are what the picture looked like when it was last read. Every program derives the palette
from those few words (microseconds, nothing decoded) and follows the file with its normal 500 ms theme poll, so macbar, macdock, the popups, Spotlight, Launchpad,
the Control Center and macsettings all restyle without a restart. Files written by earlier versions (`seed`, `scheme`, `wallpaper`, `follow`, only ever
used by Caelestia) still load: on for Caelestia as a full palette, off elsewhere.

**Following wallpaper changes.** Who decodes the picture when it changes:

* `macthemectl watch`: one tiny daemon (a `stat` of `macshell.json` per 4.5 s while the feature is off; one rdwm request plus one `stat` per 1.5 s while on).
  It looks at the wallpaper path and the file's modification time and size, so a new file *and* the same file overwritten are both noticed. Palettes are cached
  in `$XDG_CACHE_HOME/macshell/wallpaper-colors.json` (32 entries, keyed by path + mtime + size): going back to a known wallpaper, or changing the scheme,
  strength or contrast, reads no image at all (a cached refresh takes about 10 ms). Start it from the session, e.g. `exec-once = macthemectl watch`
  in rdwm's session script, or enable `contrib/macshell-wallpaper-colors.service` (systemd user unit; `contrib/macshell-wallpaper-colors.desktop` is the XDG
  autostart for other sessions). A second `watch` exits at once.
* macsettings: the Wallpaper pane, and rdwm's state while the window is open, follow too. machaven does when it sets a wallpaper.
* `macthemectl dynamic refresh`: for a script that changes the wallpaper (`rdwmctl wallpaper`, an Omarchy hook such as `~/.config/omarchy/hooks/theme-set`).

The wallpaper is looked up in this order: rdwm's live state (`wallpaper.image`), `[wallpaper] image` in rdwm's `config.toml`, Omarchy's `current/background`
link (`$XDG_STATE_HOME/omarchy/current/background`, i.e. `~/.local/state/omarchy/...`), then `<macshell config>/current/background`.
Note that an rdwm config reload (every colour change that touches rdwm's titlebar colours causes one) puts back the wallpaper of the config file, so keep `[wallpaper] image`
in step with what `rdwmctl wallpaper` shows, as the Wallpaper pane's "Save to config" does.

Inspiration: Omarchy's theme system (a `colors.toml` per theme, a light/dark marker, a picker that shows a preview image per theme; the gallery at the top
of the Theme pane is the same idea, and its tiles preview each theme with Match wallpaper applied while it is on), and matugen, pywal and wallust
for the extraction ideas. Nothing is depended on: it is all in this repository.

Following wallpaper changes: a wallpaper set from machaven, or from the Wallpaper pane of macsettings, re-derives the seed by itself (`follow`).
A change made by other tools (`rdwmctl wallpaper`, `omarchy theme bg set`, ...) is picked up by running `macthemectl dynamic refresh`
(a no-op while the feature is off), e.g. from the script that sets the wallpaper. `--no-follow` pins the seed.

## What happens on apply (`macthemectl apply`, the Theme pane)
1. Resolve the theme (nothing is written if it is broken).
2. Apply the matching keys to rdwm's `config.toml` in memory (`[colors]` focused/normal/urgent, `[decoration]` style, height, radius,
   `focused_*`/`unfocused_*` bg/fg/border, `inactive_button_color`, `font`, `font_size`, `[decoration.bar]` norm/sel/urgent/font_size),
   validate with `rdwmctl check-config` (an rdwmctl that is too old counts as syntax-only), keep comments and every other key,
   make the one-time `config.toml.bak-macsettings`, write atomically.
3. Write the selection to `macshell.json` (atomic).
4. Reload rdwm once over its socket (skipped when it is not running).
If step 1 or 2 fails nothing is written. rdwm needs no changes: all these keys already exist in its `[decoration]` schema.
Keys the theme owns are overwritten on every apply, a theme without `font` removes `[decoration] font`.

## Programs and how they follow
Each process runs one 500 ms `stat` poll (`theme::live::start_watcher`) of `macshell.json`, the selected user theme file (and its
parent) and the system scheme files (auto mode). On a change it rebuilds the theme, bumps `theme::generation()` and wakes the event
loop; `tick` redraws. Fonts rebuild their faces only when the family changed. Drawing code reads the theme at paint time
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
`tests/e2e/wallpaper-colors.sh` is Match wallpaper end to end (wallpapers of different hues set through a private rdwm, `macthemectl watch`, the accent per hue and per theme, every scheme, grey and pale
wallpapers, fixed sources, the Theme pane section, PNGs in `$SHOTS`; `PROFILE=debug` uses a debug build). Unit tests: `theme::extract` (known colour in, hue out; determinism; grey/black/white; cache), `theme::dynamic`
(any theme, accent-only/tint/full, strength 0 = untouched, schemes differ, contrast levels, light/dark follow, refresh and the watcher, text contrast across a hue sweep), `settings::wallpapercolors`.
The pieces are unit-tested too: `menubar::vbar` (geometry, hit-testing, clicks), `backend::x11` (left placement and strut), `theme::style`, `config`.
