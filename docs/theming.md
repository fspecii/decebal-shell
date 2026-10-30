# Theming

One theme choice restyles the whole desktop: the menu bar and its menus, the dock and its menu, Spotlight, Launchpad,
the Control Center, decsettings, dechaven and (through its own config) rdwm's titlebars, borders and built-in bar.
Running programs follow a change within half a second; nothing is restarted.

```
decthemectl list                       # built-in and user themes (* = current)
decthemectl apply nord                 # or "Windows 11", "macOS Light", windows-11-dark ...
decthemectl apply macos --mode light --accent '#ff9f0a'
decthemectl set mode auto              # light | dark | auto (auto follows the system, see below)
decthemectl set radius 6               # accent opacity radius font font-scale sync-rdwm; `none` unsets
decthemectl font segoe|inter|...       # font preset; list | families | status | set ROLE FAMILY[,FALLBACK...] | scale N | reset
decthemectl get [--json]               # current selection and resolved colours
decthemectl export nord > ~/.config/decebal/themes/mine.toml
decthemectl validate mine.toml
decthemectl style macos|windows|caelestia   # the whole shell look at once (theme + dock + menu bar)
decthemectl layout left|right|top|bottom|split   # where the Caelestia bar sits
decthemectl dynamic on                # "Match wallpaper": recolour the current theme from the wallpaper (off by default, see below)
decthemectl system on|off|status       # also theme GTK/Nautilus, Qt, terminals, the browser, X11 (see "Apply to all apps")
```
Or open **System Settings -> Appearance** (`decsettings appearance`; `decsettings theme` still works).

> **Theme packages.** Themes are now packages of format 1 (`theme.toml` plus wallpapers, widgets and screenshots, or one `.dtheme` file) that
> also carry fonts, the titlebar style, bar, dock, icons, hot corners and desktop widgets, with install / pack / apply / undo / preview commands:
> see [theme-engine.md](theme-engine.md). The single-file format described below still works unchanged, and `decthemectl theme duplicate`
> turns one into a package. The built-in themes are packages in `themes/<id>/theme.toml`.

## Where things live
| What | Where |
|---|---|
| Selection and overrides | `"theme"` key of `decebal.json` in the decebal config folder (`$DECEBAL_CONFIG`, default `~/.config/omarchy`), the file decbar already reads for `workspaces`. Other keys are kept. |
| User themes | `~/.config/decebal/themes/*.toml` (`$DECEBAL_THEMES`, else `$XDG_CONFIG_HOME/decebal/themes`). The file name is the theme id; a user file with a built-in id replaces it. |
| Built-in themes | compiled in (`themes/<id>/theme.toml`, theme packages of format 1): `macos`, `graphite`, `nord`, `dracula`, `solarized`, `windows11`, `caelestia`, `high-contrast` (all but Dracula have a light and a dark variant) |

`decebal.json`:
```json
{ "theme": { "name": "nord", "mode": "auto", "accent": "#ff375f", "opacity": 0.9, "radius": 8,
             "font": {"preset": "segoe", "mono": ["Fira Code", "Symbols Nerd Font"], "scale": 1.1}, "syncRdwm": true } }
```
`"theme": "dracula"` (a bare name) also works. Only `name` is needed. Without a `theme` key the look is the original macOS dark.
`mode`: `dark` (default), `light`, or `auto` (`$DECEBAL_COLOR_SCHEME`, else `~/.config/omarchy/desktop-styles/appearance`,
Omarchy's `light.mode` marker, GTK's `gtk-application-prefer-dark-theme`; dark when none says otherwise). A theme with only one
variant ignores the mode. A name such as `macOS Light` or `windows-11-dark` also selects the mode.
`palette` is an override too: a colour palette laid over any theme (see [themes.md](themes.md#palettes-colours-separate-from-themes), `decthemectl palette`).
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
and in `decthemectl list`; the shell falls back to the default theme and recovers when the file is fixed). Colours are
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
the Caelestia pill bar with pills for its widgets, on the screen edge `bar.position` in `decebal.json` names (`left` by default; `right`, `top`, `bottom`
or `split`, set with `decthemectl layout`): see [caelestia.md](caelestia.md). That bar reserves a strut on its own edge instead of a top one, and `dock.enabled = "auto"` hides the dock under it),
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
(`decthemectl font list` shows what each one found here; the Theme pane marks them "not installed"):

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
decthemectl font list                          # presets and what each resolves to here
decthemectl font segoe                         # a preset: replaces earlier role overrides
decthemectl font set ui Inter,"Noto Sans CJK","Noto Color Emoji"   # a chain for one role (kept in front of the preset's chain)
decthemectl font set mono "Fira Code"          # ROLE: ui title mono display; `none` goes back to the preset
decthemectl font status | families | scale 1.1 | reset
```
The choice is stored in `decebal.json`: `"theme": {"font": {"preset": "segoe", "ui": ["Inter", "Noto Sans CJK"], "mono": "Fira Code", "title": ..., "display": ..., "scale": 1.1}}`.
A role value is a family or a list. The user's preset replaces the theme's own font keys; a user role chain goes in front of the preset's.
Old files keep working: `"font": "Inter"` (a string) and `"fontScale"` are the single font of text, titles and display numbers
(`decthemectl set font Inter`), and `[font] family` / `font_family` in a theme file is the same.

Layers, lowest first: preset (the user's, else the theme's, else `inter`) < theme `font_ui`/... < theme `family` < user `font` string < user role chains.
Installed fonts are read with one `fc-list` (plus two `fc-match` for the generic names) when a program starts or the theme's fonts change, never per frame;
a program keeps only the faces of the families its chains resolved to, not the table of every installed face.
Faces are memory-mapped and opened on the first glyph that is not cached; a parsed fontdue face (about 13 MB for Inter) is built and freed on a
short-lived worker thread (`src/mem.rs`: a second malloc arena) so that none of it stays resident afterwards: an idle `decnotify` is about 5 MB
with banners on screen, as it was before fonts. Latin fonts are drawn with fontdue as before; fonts with more than 6000 glyphs
(Noto CJK would take 320 MB) and variable fonts (their weight axis is applied) are rasterised glyph by glyph with ttf-parser and tiny-skia.
Newly installed fonts are noticed when the theme's fonts change or the program restarts (the Theme pane rescans every 30 s).

## Shell styles: macOS, Windows, Caelestia
`decthemectl style NAME` (or **Shell style** at the top of the Theme pane) sets the theme, the dock style and the menu bar together:

| style | theme | dock (`dock-settings.json`) | dock shown (`decebal.json` `dock.enabled`) | menu bar (`decebal.json` `bar.enabled`) |
|---|---|---|---|---|
| `macos` | macOS | `style: mac`; `roundness` removed so the theme's `dock_roundness` rules | `true` | `true` |
| `windows` | Windows 11 | `style: windows`, `windowsAlign: center` (the Windows 11 taskbar) | `true` | `"auto"`: hidden while the taskbar is up |
| `caelestia` | Caelestia | `style: mac`; `roundness` removed (the theme's pill) | `"auto"`: hidden while the theme has the pill bar | `true`, drawn as the pill bar on the edge `bar.position` names (left by default; `caelestia-top` and the like set it) |

Accent, translucency, radius, font and text size overrides are kept, and so is Match wallpaper (it works with every style). `decthemectl style`
without a name prints the current preset (`custom` when the theme and dock do not match one). Nothing else in the two JSON files is touched.

## The Control Center in every style
The quick-settings panel is part of the shell: the macOS menu bar has its icon, the Windows taskbar a quick-settings button in the tray (it opens
the same panel, in the `fluent` look: the theme's radius as it is, squared toggles), the Caelestia bar its Control tile on every edge, and
`decutil control-center toggle` (or `decbar --control-center`) opens it in any theme and with no bar at all: bind a key to it (rdwm:
`action = "spawn"`, `args = ["decutil", "control-center", "toggle"]`) or choose **Control Center** for a hot corner. `decebal.json`:

```json
"control_center": { "enabled": true, "position": "auto", "style": "auto", "tiles": ["connectivity", "focus", "display", "sound", "media", "toggles", "actions"] }
```
`position` (`auto`, `top-right`, `top-left`, `bottom-right`, `bottom-left`) is where a key or hot corner opens it (`auto`: beside the bar's own
button, above the taskbar, or the top right with no bar); `style` is `card`, `fluent` or `auto`; `tiles` lists the groups to show (an empty or
unknown list means all). A theme can set the same keys in its `[control_center]` section (docs/theme-engine.md). `decutil control-center
close|open|toggle [--corner C]`.

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

## Match wallpaper
An optional layer over **any** theme (macOS, Windows 11, Caelestia, Nord, your own files ...): the colours of the wallpaper recolour the accent and,
if you want, the surfaces. Off by default; with no `dynamic` key, or `"enabled": false`, nothing changes. Turn it on in **System Settings -> Theme ->
Match wallpaper** or from a terminal:
```
decthemectl dynamic on [--scheme S] [--surfaces K] [--strength N] [--contrast N] [--mode M] [--source X]
decthemectl dynamic off                          # back to the theme's own colours; the settings are kept
decthemectl dynamic status [--json]              # settings, the picture's colours, the resulting accent
decthemectl dynamic scheme vibrant               # tonal-spot | vibrant | expressive | monochrome | neutral | content | palette
decthemectl dynamic surfaces tint                # accent | tint | full
decthemectl dynamic strength 60                  # 0-100 %, how far surfaces move (tint, full)
decthemectl dynamic contrast 20                  # -100..100 %, Material's contrast level
decthemectl dynamic mode wallpaper               # light/dark follow the picture's brightness (keep = the Mode setting rules)
decthemectl dynamic source wallpaper|~/a.jpg|'#2a9d8f'   # what rdwm shows | a fixed image | a fixed colour
decthemectl dynamic refresh                      # read the wallpaper now (a no-op while off)
decthemectl dynamic preview [--wallpaper P] [--scheme S] [--json]   # show the palette without applying anything
decthemectl watch [--interval MS] [--once]       # follow wallpaper changes (see below)
decthemectl dynamic --wallpaper a.jpg --scheme vibrant --light      # the older form: Caelestia with a full palette from a.jpg
```

**What it recolours** (`surfaces`):

| value | effect |
|---|---|
| `accent` | only the accent (and the text on it); the rdwm focus border follows because it is the accent |
| `tint` (default) | the accent, plus backgrounds, bars, popups, borders and text tinted towards the wallpaper: the theme keeps its own lights and darks (a white Windows 11 stays white, Nord stays dark) and gains a wallpaper-coloured cast. Lightness is untouched; hue is pulled to the palette's neutral hue and chroma towards a small goal, both by `strength` (0 = just the accent). |
| `full` | each colour blends into the Material palette of the wallpaper (`strength` 100 % replaces them: the Caelestia look on any theme). Caelestia starts here. |

A colour you pick by hand (`decthemectl apply --accent`, the accent swatches) wins over the wallpaper's accent. Metrics (radius, translucency,
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

**Where it lives.** `"dynamic"` inside the `theme` object of `decebal.json`:
```json
{ "theme": { "name": "nord", "dynamic": { "enabled": true, "scheme": "tonal-spot", "source": "wallpaper", "surfaces": "tint",
   "strength": 0.5, "contrast": 0, "mode": "keep",
   "seed": "#2a9d8f", "accents": ["#e07a3a"], "tone": 38.2, "image": "/home/me/Pictures/a.png" } } }
```
The settings come first; `seed`, `accents`, `tone` (and `grey`) are what the picture looked like when it was last read. Every program derives the palette
from those few words (microseconds, nothing decoded) and follows the file with its normal 500 ms theme poll, so decbar, decdock, the popups, Spotlight, Launchpad,
the Control Center and decsettings all restyle without a restart. Files written by earlier versions (`seed`, `scheme`, `wallpaper`, `follow`, only ever
used by Caelestia) still load: on for Caelestia as a full palette, off elsewhere.

**Following wallpaper changes.** Who decodes the picture when it changes:

* `decthemectl watch`: one tiny daemon (a `stat` of `decebal.json` per 4.5 s while the feature is off; one rdwm request plus one `stat` per 1.5 s while on).
  It looks at the wallpaper path and the file's modification time and size, so a new file *and* the same file overwritten are both noticed. Palettes are cached
  in `$XDG_CACHE_HOME/decebal/wallpaper-colors.json` (32 entries, keyed by path + mtime + size): going back to a known wallpaper, or changing the scheme,
  strength or contrast, reads no image at all (a cached refresh takes about 10 ms). Start it from the session, e.g. `exec-once = decthemectl watch`
  in rdwm's session script, or enable `contrib/decebal-wallpaper-colors.service` (systemd user unit; `contrib/decebal-wallpaper-colors.desktop` is the XDG
  autostart for other sessions). A second `watch` exits at once.
* decsettings: the Wallpaper pane, and rdwm's state while the window is open, follow too. dechaven does when it sets a wallpaper.
* `decthemectl dynamic refresh`: for a script that changes the wallpaper (`rdwmctl wallpaper`, an Omarchy hook such as `~/.config/omarchy/hooks/theme-set`).

The wallpaper is looked up in this order: rdwm's live state (`wallpaper.image`), `[wallpaper] image` in rdwm's `config.toml`, Omarchy's `current/background`
link (`$XDG_STATE_HOME/omarchy/current/background`, i.e. `~/.local/state/omarchy/...`), then `<decebal config>/current/background`.
Note that an rdwm config reload (every colour change that touches rdwm's titlebar colours causes one) puts back the wallpaper of the config file, so keep `[wallpaper] image`
in step with what `rdwmctl wallpaper` shows, as the Wallpaper pane's "Save to config" does.

Inspiration: Omarchy's theme system (a `colors.toml` per theme, a light/dark marker, a picker that shows a preview image per theme; the gallery at the top
of the Theme pane is the same idea, and its tiles preview each theme with Match wallpaper applied while it is on), and matugen, pywal and wallust
for the extraction ideas. Nothing is depended on: it is all in this repository.

Following wallpaper changes: a wallpaper set from dechaven, or from the Wallpaper pane of decsettings, re-derives the seed by itself (`follow`).
A change made by other tools (`rdwmctl wallpaper`, `omarchy theme bg set`, ...) is picked up by running `decthemectl dynamic refresh`
(a no-op while the feature is off), e.g. from the script that sets the wallpaper. `--no-follow` pins the seed.

## What happens on apply (`decthemectl apply`, the Theme pane)
1. Resolve the theme (nothing is written if it is broken).
2. Apply the matching keys to rdwm's `config.toml` in memory (`[colors]` focused/normal/urgent, `[decoration]` style, height, radius,
   `focused_*`/`unfocused_*` bg/fg/border, `inactive_button_color`, `font` (the `title` role's first installed family), `fallback_fonts` (the
   rest of its chain, plus Noto Sans CJK and an emoji font when installed), `font_size`, `[decoration.bar]` norm/sel/urgent/font_size),
   validate with `rdwmctl check-config` (an rdwmctl that is too old counts as syntax-only), keep comments and every other key,
   make the one-time `config.toml.bak-decsettings`, write atomically.
3. Write the selection to `decebal.json` (atomic).
4. Reload rdwm once over its socket (skipped when it is not running).
If step 1 or 2 fails nothing is written. rdwm needs no changes: all these keys already exist in its `[decoration]` schema.
Keys the theme owns are overwritten on every apply, a theme whose title role has only generic fonts removes `[decoration] font` and `fallback_fonts`.

## Apply to all apps

`DECEBAL_SYSTEM_AUTOMATIC=0` disables automatic app retinting for one process (for example, isolated desktop tests). It leaves saved preferences unchanged and does not suppress an explicit `apply-system` or `system on`. App-integration tests must establish their private HOME/XDG environment before starting their private D-Bus activation bus.

One automatic pipeline carries the theme beyond decebal: GTK 3/4 and libadwaita apps (Nautilus), Qt apps, terminals, btop, editors,
the browser colour and plain X11 clients. It is **on unless explicitly disabled** (`decthemectl system off`, or **System Settings -> Appearance -> All apps**). Every `decthemectl apply/set/style/dynamic`, every choice in the Theme pane and every wallpaper-colour
change re-applies it (debounced: several changes in a row make one run, in a detached `decthemectl apply-system`).

```
decthemectl system on                  # apply now and after every theme change
decthemectl system off                 # put every file and setting back, then switch off
decthemectl system status [--json]     # which targets run, which are skipped and why
decthemectl system target gtk off      # per-target checkbox: omarchy browser editors hooks gtk qt kvantum x11 terminals (default = auto)
decthemectl system set icon-theme Papirus     # optional: also set an icon / cursor theme (cursor-theme); `none` unsets
decthemectl system set gtk-font off    # keep your own GTK font (the theme's font is carried to GTK by default, see the `gtk` row)
decthemectl apply-system [--dry-run] [--only gtk,qt]   # run it by hand; --dry-run lists the files it would write
```

The palette comes from the resolved theme (background, sidebar, surface, popup, titlebar, text, accent, red/green/yellow/blue, dark or
light). Each target renders from it (`src/theme/system/*.rs`):

| target | default | what it changes |
|---|---|---|
| `omarchy` | on when Omarchy is installed | Writes `~/.config/omarchy/themes/decebal/colors.toml` (Omarchy's palette format) and runs `omarchy-theme-set decebal`, so **Omarchy's own template engine** makes alacritty, foot, kitty, ghostty, btop, helix, neovim, vscode, obsidian and chromium theme files from `$OMARCHY_PATH/default/themed/*.tpl` (and your `~/.config/omarchy/themed`). It runs headless with Omarchy's background step off (decebal owns the wallpaper). Live reload: alacritty (its config is touched; it watches it), foot (`omarchy-theme-set-foot`), kitty (`kitty @ set-colors` on the sockets it finds), btop (`omarchy-restart-btop`). **No signal is sent to any other process** unless `decthemectl system set signals on` (then Omarchy's `omarchy-restart-terminal` / `-helix`: SIGUSR1 kitty and helix, SIGUSR2 ghostty). Not run at all: Omarchy's keyboard-RGB, GNOME icon, Hyprland-reload, tmux helpers. |
| `browser` | on when the browser and passwordless `omarchy-theme-set-browser-policy` rule are available | `omarchy-theme-set-browser`: the Chromium/Chrome/Edge/Brave `BrowserThemeColor` policy from `chromium.theme`. This writes root-owned policy files, so decebal never triggers a password prompt: without the rule automatic integration stays off. |
| `editors` | on when a supported app is installed | Omarchy sync helpers for VS Code / Cursor, Obsidian, T3 Code, Pi, Claude Code and Hermes. Off restores the previous Omarchy theme through the same helpers. |
| `hooks` | off | your `~/.config/omarchy/hooks/theme-set.d` scripts |
| `gtk` | on | a marked block in `~/.config/gtk-3.0/gtk.css` and `gtk-4.0/gtk.css` (libadwaita modern CSS variables and compatibility named colours: `window_bg_color`, `accent_bg_color`, `headerbar_*`, `sidebar_*`, `card_*`, `popover_*`, ... plus the `theme_*` names and widget rules for GTK 3, whose Adwaita theme ignores the names), `settings.ini` (`gtk-application-prefer-dark-theme`, `gtk-decoration-layout`: window buttons on the left for the macOS titlebar style, on the right for classic; `gtk-font-name`) and gsettings `color-scheme` (dark/light), `font-name`, `button-layout`. The font keys are written only when the theme's interface font is an explicit, installed family (a generic `sans-serif` or a font that is not installed leaves yours alone) and the option **Theme font in GTK apps** (`decthemectl system set gtk-font on|off`, `system.options."gtk.font"` in `decebal.json`, on by default; switch it off in Settings > Theme > Apply to all apps) is on. Icon and cursor theme only if you set them. |
| `qt` | on when qt5ct/qt6ct is installed | `qt5ct/colors/decebal.conf` and `qt6ct/colors/decebal.conf` and `color_scheme_path` + `custom_palette=true` in `qt5ct.conf` / `qt6ct.conf` (an existing `style` is kept; `Fusion` otherwise). Qt reads it with `QT_QPA_PLATFORMTHEME=qt6ct` (qt5ct for Qt 5) in the session environment; `decthemectl system status` warns when it is not set. |
| `kvantum` | off | a colours-only Kvantum theme `~/.config/Kvantum/decebal` selected in `kvantum.kvconfig` (replaces your Kvantum theme while on). |
| `terminals` | on when a supported terminal is installed | Native alacritty/kitty/ghostty/foot configuration and st-256color OSC updates plus guarded Bash/Zsh startup hooks; works without Omarchy. Running st updates immediately; GTK custom palettes require a new process. See [actual app behaviour and runtime verification](systemtheming/app-propagation.md). |
| `x11` | on when xrdb is installed | `~/.config/decebal/system/Xresources` (`*.foreground`, `*.background`, `*.color0..15`, `Xcursor.theme` if set), included from a block in `~/.Xresources` and merged with `xrdb -merge` when `$DISPLAY` is set. st, xterm and urxvt pick it up when they start (st needs the Xresources patch). |

rdwm's titlebars and borders are still written by `decthemectl apply` itself (see below); nothing changes there.

**Safety.** Nothing outside decebal's own files is touched while the switch is off. When it is on, every file is first read, then all
changes of a run are written together (temporary file + rename, symlinks followed, permissions kept; if one write fails the others are rolled back).
Files shared with you (`gtk.css`, `~/.Xresources`) get one marked block appended, INI files (`settings.ini`, `qt6ct.conf`) get only their keys
set. The first version of each file (or "did not exist") and the old gsettings / xrdb values are saved in
`$XDG_STATE_HOME/decebal/system/` (`manifest.json`, `backups/`). `decthemectl system off` restores them exactly; a file you edited since is
not overwritten (only a decebal block is cut out of it, anything else is reported and left). A target you uncheck gives its files back at once.
Programs missing on the machine (no qt5ct, no xrdb, no Omarchy, no browser) are skipped and listed by `status`.
Run `decthemectl apply-system --dry-run` first to see which files would be written.

Limits: Kvantum was not run against a real Kvantum install; Qt colours were verified as files only (no qt6ct here); st needs the Xresources
patch; ghostty and helix pick the new colours up at their next start unless `signals` is on. Environment: `DECEBAL_SYSTEM_SYNC=1` runs the
apply inline instead of detached (scripts, tests), `DECEBAL_SYSTEM_STATE` moves the state directory, `OMARCHY_PATH` is read-only.

## Programs and how they follow
Each process runs one 500 ms `stat` poll (`theme::live::start_watcher`) of `decebal.json`, the selected user theme file (and its
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
variant, live reload, rdwm edits, the settings pane), `tests/e2e/theme.sh` (private Xvfb + rdwm + decbar + decdock +
decsettings: decthemectl, user theme file edits, the UI, pixel checks, screenshots in `$SHOTS`),
`THEME_SHOTS=dir cargo test --lib -- --ignored launchpad_in_three_themes`, `tests/e2e/style.sh` (the three shell styles on the real decbar and
decdock, pixel checks per style, the pill geometry, wallpaper colours, and `BASELINE_BIN=dir` to prove the macOS style is pixel-identical to a build from before the Caelestia theme).
`tests/e2e/layouts.sh` runs every Caelestia layout (left, right, top, bottom, split) the same way (geometry, strut, popout side, split's two processes).
`tests/e2e/system-theme.sh` runs "Apply to all apps" for real in a private HOME, Xvfb and D-Bus session (GTK 4/libadwaita and GTK 3 windows, Nautilus, xrdb, Omarchy's template engine, off restoring every file; screenshots in `$SHOTS`).
`tests/e2e/vbar.sh` runs the vertical Caelestia bar for real (strut, geometry, workspace pills, popups, launching the widgets, the dock hiding, switching styles both ways).
`tests/e2e/wallpaper-colors.sh` is Match wallpaper end to end (wallpapers of different hues set through a private rdwm, `decthemectl watch`, the accent per hue and per theme, every scheme, grey and pale
wallpapers, fixed sources, the Theme pane section, PNGs in `$SHOTS`; `PROFILE=debug` uses a debug build). Unit tests: `theme::extract` (known colour in, hue out; determinism; grey/black/white; cache), `theme::dynamic`
(any theme, accent-only/tint/full, strength 0 = untouched, schemes differ, contrast levels, light/dark follow, refresh and the watcher, text contrast across a hue sweep), `settings::wallpapercolors`.
The pieces are unit-tested too: `menubar::vbar` (geometry, hit-testing, clicks), `backend::x11` (left placement and strut), `theme::style`, `config`.

Browser/editor targets run independently after staging, including when their
switch is enabled after the same palette is already current. Per-target opt-outs
remain respected. GTK custom CSS is read at application startup; existing GTK
processes need reopening for a complete palette change. Decebal does not silently
terminate applications with open documents or file operations.

### Whole-window shadows on X11

Themes' `metrics.shadow` now drives a unified shadow around server-decorated
applications in supporting rdwm sessions. Zero frees the shadow carrier. The
renderer needs a compositor and keeps geometry-only moves free of pixel uploads.
An already running older rdwm remains unchanged until a new session; apply the
theme there to pick up its strength. Custom/CSD decorations and Wayland remain
separate work. See [implementation and limits](window-shadow-design.md).
