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
```
Or open **System Settings -> Theme** (`macsettings theme`).

## Where things live
| What | Where |
|---|---|
| Selection and overrides | `"theme"` key of `macshell.json` in the macshell config folder (`$MACSHELL_CONFIG`, default `~/.config/omarchy`), the file macbar already reads for `workspaces`. Other keys are kept. |
| User themes | `~/.config/macshell/themes/*.toml` (`$MACSHELL_THEMES`, else `$XDG_CONFIG_HOME/macshell/themes`). The file name is the theme id; a user file with a built-in id replaces it. |
| Built-in themes | compiled in (`src/theme/builtin/*.toml`): `macos`, `graphite`, `nord`, `dracula`, `solarized`, `windows11`, `high-contrast` (all but Dracula have a light and a dark variant) |

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
`radius` (popup corner radius, px; other surfaces scale from it), `dock_roundness` (0..1), `dock_alpha`, `popup_alpha`,
`panel_alpha`, `bar_alpha`, `shadow`, `outline` (alpha of the 1 px outline), `dock_icon_size`, `dock_padding`, `bar_height`
(these three are published for the layout code, which does not read them yet), `titlebar_style` (`macos`/`classic`),
`titlebar_height`, `font_family`, `font_scale`. The table with ranges is `TOKENS` in `src/theme/model.rs`.
The dock uses `dock_alpha` as its opacity at the default `dock-settings.json` (`transparency` still scales it) and
`dock_roundness` unless `dock-settings.json` sets `roundness`.

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
`THEME_SHOTS=dir cargo test --lib -- --ignored launchpad_in_three_themes`.
