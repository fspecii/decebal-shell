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
macthemectl dynamic [--wallpaper PATH] [--light|--dark] [--scheme vibrant]   # Caelestia colours from the wallpaper
macthemectl system on|off|status       # also theme GTK/Nautilus, Qt, terminals, the browser, X11 (see "Apply to all apps")
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
1 px outline at 7 %, titlebars 32 px. Font: Google Sans Flex (caelestia ships it in `assets/google-sans-flex`; install it as a system font),
falling back to Inter like every theme. Caelestia has no window titlebars of its own; rdwm's follow the theme as usual.

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
   `focused_*`/`unfocused_*` bg/fg/border, `inactive_button_color`, `font`, `font_size`, `[decoration.bar]` norm/sel/urgent/font_size),
   validate with `rdwmctl check-config` (an rdwmctl that is too old counts as syntax-only), keep comments and every other key,
   make the one-time `config.toml.bak-macsettings`, write atomically.
3. Write the selection to `macshell.json` (atomic).
4. Reload rdwm once over its socket (skipped when it is not running).
If step 1 or 2 fails nothing is written. rdwm needs no changes: all these keys already exist in its `[decoration]` schema.
Keys the theme owns are overwritten on every apply, a theme without `font` removes `[decoration] font`.

## Apply to all apps

One optional pipeline carries the theme beyond macshell: GTK 3/4 and libadwaita apps (Nautilus), Qt apps, terminals, btop, editors,
the browser colour and plain X11 clients. It is **off until you switch it on** (`macthemectl system on`, or **System Settings ->
Theme -> Apply to all apps**); then every `macthemectl apply/set/style/dynamic`, every choice in the Theme pane and every wallpaper-colour
change re-applies it (debounced: several changes in a row make one run, in a detached `macthemectl apply-system`).

```
macthemectl system on                  # apply now and after every theme change
macthemectl system off                 # put every file and setting back, then switch off
macthemectl system status [--json]     # which targets run, which are skipped and why
macthemectl system target gtk off      # per-target checkbox: omarchy browser editors hooks gtk qt kvantum x11 (default = auto)
macthemectl system set icon-theme Papirus     # optional: also set an icon / cursor theme (cursor-theme); `none` unsets
macthemectl apply-system [--dry-run] [--only gtk,qt]   # run it by hand; --dry-run lists the files it would write
```

The palette comes from the resolved theme (background, sidebar, surface, popup, titlebar, text, accent, red/green/yellow/blue, dark or
light). Each target renders from it (`src/theme/system/*.rs`):

| target | default | what it changes |
|---|---|---|
| `omarchy` | on when Omarchy is installed | Writes `~/.config/omarchy/themes/macshell/colors.toml` (Omarchy's palette format) and runs `omarchy-theme-set macshell`, so **Omarchy's own template engine** makes alacritty, foot, kitty, ghostty, btop, helix, neovim, vscode, obsidian and chromium theme files from `$OMARCHY_PATH/default/themed/*.tpl` (and your `~/.config/omarchy/themed`). It runs headless with Omarchy's background step off (macshell owns the wallpaper). Live reload: alacritty (its config is touched; it watches it), foot (`omarchy-theme-set-foot`), kitty (`kitty @ set-colors` on the sockets it finds), btop (`omarchy-restart-btop`). **No signal is sent to any other process** unless `macthemectl system set signals on` (then Omarchy's `omarchy-restart-terminal` / `-helix`: SIGUSR1 kitty and helix, SIGUSR2 ghostty). Not run at all: Omarchy's keyboard-RGB, GNOME icon, Hyprland-reload, tmux and AI-tool helpers. |
| `browser` | on only if sudo needs no password for `omarchy-theme-set-browser-policy` (Omarchy's own rule) | `omarchy-theme-set-browser`: the Chromium/Chrome/Edge/Brave `BrowserThemeColor` policy from `chromium.theme`. This writes root-owned policy files, so macshell never triggers a password prompt: without the rule the box stays off. |
| `editors` | off | `omarchy-theme-set-vscode` and `-obsidian` (VS Code / Cursor settings, Obsidian vaults). Off restores the previous Omarchy theme through the same helpers. |
| `hooks` | off | your `~/.config/omarchy/hooks/theme-set.d` scripts |
| `gtk` | on | a marked block in `~/.config/gtk-3.0/gtk.css` and `gtk-4.0/gtk.css` (libadwaita named colours: `window_bg_color`, `accent_bg_color`, `headerbar_*`, `sidebar_*`, `card_*`, `popover_*`, ... plus the `theme_*` names and widget rules for GTK 3, whose Adwaita theme ignores the names), `settings.ini` (`gtk-application-prefer-dark-theme`, `gtk-decoration-layout`: window buttons on the left for the macOS titlebar style, on the right for classic; `gtk-font-name` when the theme names a font) and gsettings `color-scheme` (dark/light), `font-name`, `button-layout`. Icon and cursor theme only if you set them. |
| `qt` | on when qt5ct/qt6ct is installed | `qt5ct/colors/macshell.conf` and `qt6ct/colors/macshell.conf` and `color_scheme_path` + `custom_palette=true` in `qt5ct.conf` / `qt6ct.conf` (an existing `style` is kept; `Fusion` otherwise). Qt reads it with `QT_QPA_PLATFORMTHEME=qt6ct` (qt5ct for Qt 5) in the session environment; `macthemectl system status` warns when it is not set. |
| `kvantum` | off | a colours-only Kvantum theme `~/.config/Kvantum/macshell` selected in `kvantum.kvconfig` (replaces your Kvantum theme while on). |
| `x11` | on when xrdb is installed | `~/.config/macshell/system/Xresources` (`*.foreground`, `*.background`, `*.color0..15`, `Xcursor.theme` if set), included from a block in `~/.Xresources` and merged with `xrdb -merge` when `$DISPLAY` is set. st, xterm and urxvt pick it up when they start (st needs the Xresources patch). |

rdwm's titlebars and borders are still written by `macthemectl apply` itself (see below); nothing changes there.

**Safety.** Nothing outside macshell's own files is touched while the switch is off. When it is on, every file is first read, then all
changes of a run are written together (temporary file + rename, symlinks followed, permissions kept; if one write fails the others are rolled back).
Files shared with you (`gtk.css`, `~/.Xresources`) get one marked block appended, INI files (`settings.ini`, `qt6ct.conf`) get only their keys
set. The first version of each file (or "did not exist") and the old gsettings / xrdb values are saved in
`$XDG_STATE_HOME/macshell/system/` (`manifest.json`, `backups/`). `macthemectl system off` restores them exactly; a file you edited since is
not overwritten (only a macshell block is cut out of it, anything else is reported and left). A target you uncheck gives its files back at once.
Programs missing on the machine (no qt5ct, no xrdb, no Omarchy, no browser) are skipped and listed by `status`.
Run `macthemectl apply-system --dry-run` first to see which files would be written.

Limits: Kvantum was not run against a real Kvantum install; Qt colours were verified as files only (no qt6ct here); st needs the Xresources
patch; ghostty and helix pick the new colours up at their next start unless `signals` is on. Environment: `MACSHELL_SYSTEM_SYNC=1` runs the
apply inline instead of detached (scripts, tests), `MACSHELL_SYSTEM_STATE` moves the state directory, `OMARCHY_PATH` is read-only.

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
`tests/e2e/system-theme.sh` runs "Apply to all apps" for real in a private HOME, Xvfb and D-Bus session (GTK 4/libadwaita and GTK 3 windows, Nautilus, xrdb, Omarchy's template engine, off restoring every file; screenshots in `$SHOTS`).
`tests/e2e/vbar.sh` runs the vertical Caelestia bar for real (strut, geometry, workspace pills, popups, launching the widgets, the dock hiding, switching styles both ways).
The pieces are unit-tested too: `menubar::vbar` (geometry, hit-testing, clicks), `backend::x11` (left placement and strut), `theme::style`, `config`.
