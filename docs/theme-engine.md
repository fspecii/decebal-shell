# Theme engine

A theme is a **package of data**: colours for dark and light, metrics, fonts, a wallpaper, a titlebar style, the bar and dock, icons, hot
corners, a few safe rdwm keys and desktop widgets. Every built-in theme (macOS, Windows 11, Caelestia, Nord, Dracula, ...) is such a package
(`themes/<id>/theme.toml`, compiled in), so there is **one engine**: your own themes, a theme from a friend and the ones shipped with decebal
use the same keys and the same code. Nothing in a package is ever executed. That is what makes a future theme market safe.

```
decthemectl theme new mine --from caelestia      # start from a copy
$EDITOR ~/.config/decebal/themes/mine/theme.toml
decthemectl theme validate ~/.config/decebal/themes/mine
decthemectl theme apply mine --preview 10        # 10 s, then everything is put back
decthemectl theme apply mine ; decthemectl theme undo
decthemectl theme pack ~/.config/decebal/themes/mine -o mine.dtheme
```

* [Package format](#package-format-version-1) * [Manifest reference](#manifest-reference) * [Inheritance](#inheritance-extends)
* [Commands](#commands) * [What apply does](#what-apply-does-undo-preview) * [Safety](#safety-model) * [Widgets](#widgets)
* [Theme pane](#the-theme-pane) * [Status of every key](#what-the-shell-applies-today) * [Tests](#tests)

## Package format (version 1)

A package is a directory, or the same directory as one `.dtheme` file (a reproducible gzip-compressed tar: sorted, mtime 0, no owners).

```
sunset-grid/
  theme.toml                 required: the manifest
  wallpapers/sunset.jpg      png / jpg / webp only
  screenshots/main.jpg       shown in the gallery and the market
  fonts/Inter.ttf            ttf / otf only, under fonts/
  widgets/sunset-clock.toml  widget files (see Widgets)
  README.md  LICENSE.txt     .md / .txt in the root, never read by the shell
```
Installed themes live in `~/.config/decebal/themes/<id>/` (`$DECEBAL_THEMES`), system-wide ones in `/usr/share/decebal/themes/<id>/`
(`$DECEBAL_SYSTEM_THEMES`); the user's folder wins. A single legacy `~/.config/decebal/themes/<name>.toml` (no `format` key) keeps working
exactly as before, and `decthemectl theme duplicate NAME NEW` upgrades it to a package.

`theme.toml` starts with `format = 1`. A reader that does not know the format refuses the file; format 2 will be a different number, not
a different meaning of these keys.

## Manifest reference

`decthemectl theme schema` prints every key with its type, range and whether the shell applies it yet (`--json` for tools); it is generated
from the same table the validator uses (`src/theme/engine/schema.rs`), so it cannot drift from the code. Unknown keys are errors
("did you mean ...?"), and every error names the line and the key.

```toml
format = 1                          # required
id = "sunset-grid"                  # required in a package: [a-z0-9][a-z0-9._-]*, at most 64; the install folder
name = "Sunset Grid"                # required
version = "1.0.0"                   # required in a package (MAJOR.MINOR.PATCH)
author = "Ada"   license = "MIT"    # the market will require both
description = "..."  homepage = "https://..."
min_shell_version = "0.1.0"         # rejected by Decebal 0.2+ when this shell is older
screenshots = ["screenshots/main.jpg"]   # at most 8, inside the package
tags = ["dark", "minimal"]               # at most 12
extends = "caelestia"               # inherit; see below

[integrity]                         # written by `theme pack` / `theme hash --write`
content_hash = "sha256:..."         # checked on install and by `theme list`
signature = ""  signer = ""         # reserved for the market: stored, not verified yet

[colors.dark]                       # also [colors.light]; keys directly under [colors] apply to both
background = "#1b1230"              # only background, text and accent are needed; the rest is derived
text = "#fbeee0"   accent = "#ff8c42"
# sidebar surface bar bar_text popup dock overlay titlebar titlebar_inactive border text_secondary text_faint accent_text danger ok warn

[metrics]
radius = 14                         # popup radius; other surfaces scale from it
dock_roundness = 0.6   dock_alpha = 0.78   popup_alpha = 0.95   panel_alpha = 0.97   bar_alpha = 1
shadow = 1.2   outline = 0.1   dock_padding = 16
gap = 6   blur = 0.4                # reserved (accepted, nothing reads them yet)

[font]
preset = "inter"                    # system inter segoe cantarell rubik jetbrains ibm-plex fira noto monospace-ui
ui = ["Inter", "Noto Sans CJK"]     # roles: ui title mono display; a family or a list (first installed draws,
title = "Rubik"  mono = "JetBrains Mono"  display = "Inter Display"   #  later ones supply missing characters)
scale = 1.0   fallback = ["Noto Color Emoji"]   # fallback is appended to every role written in this file

[wallpaper]
image = "wallpapers/sunset.jpg"     # or images = [...]; the first is used
mode = "fill"  color = "#1b1230"    # fill fit stretch center tile; colour behind the picture
match_colors = true                 # switch on "Match wallpaper" for this theme, reading the bundled image
scheme = "vibrant"  surfaces = "tint"  strength = 0.5  follow_mode = false
slideshow = true  interval = 600  shuffle = true      # stored, no runner yet

[decoration]                        # rdwm titlebars
style = "adwaita"                   # macos macos-light classic windows windows-light adwaita adwaita-light breeze arc materia
height = 36  radius = 12            #   nord gruvbox dracula minimal borderonly (`rdwmctl styles`); the style decides the side
title_align = "center"  buttons = "none"   # of the window buttons

[rdwm]                              # the only rdwm keys a theme can touch
gap_inner = 10  gap_outer = 12  border_px = 2  smart_gaps = true  mfact = 0.55  nmaster = 1
layouts = ["tile", "monocle"]       # first is the default
focused_border = "#ff8c42"  normal_border = "#33245a"  urgent_border = "#ff6b6b"

[bar]
enabled = true                      # true | false | "auto"
style = "floating"                  # flat | floating | vertical (the Caelestia pill bar)
position = "left"                   # with style = "vertical": left right top bottom split
height = 26
left = ["workspaces"]  right = ["clock"]   # item lists: stored, the bar does not read them yet

[dock]
enabled = true                      # true | false | "auto" (hidden under the vertical bar)
style = "mac"                       # mac | windows (the taskbar)
size = 54   align = "center"   position = "bottom"   roundness = 0.6   magnification = 1.6

[icons]
theme = "WhiteSur-dark"  size_scale = 1.0  monochrome_tint = false  fallback = "Adwaita"

[hot_corners]
top_right = "dashboard"             # none dashboard session spotlight launchpad overview lock tile-all notifications control-center
dwell_ms = 200                      # `custom:<command>` is NOT accepted from a theme

[control_center]                    # the quick-settings panel; every key optional, the defaults make it present in every theme
enabled = true                      # false removes its buttons and turns `decutil control-center` into a no-op
position = "auto"                   # auto | top-right | top-left | bottom-right | bottom-left: where a key or hot corner opens it
style = "auto"                      # auto | card | fluent (auto = fluent under the Windows taskbar, card elsewhere)
tiles = ["connectivity", "focus", "display", "sound", "media", "toggles", "actions"]   # groups to show (default: all)

[[widgets]]                         # up to 32 instances
use = "sunset-clock"                # widgets/sunset-clock.toml of this package, or "builtin:cpu-ram-ring"
place = "desktop"                   # desktop | bar
anchor = "top-right"  x = -48  y = 64     # offsets from the anchor; negative moves inwards on right/bottom anchors
[widgets.options]                   # becomes {opt.format} etc. inside the widget
format = "%H:%M"
```
The file is read with real TOML. Colours are `#rgb` or `#rrggbb`. Numbers outside their range are errors, not clamped.

### Inheritance (`extends`)
`extends = "caelestia"` starts from another theme (installed or built-in) and overrides only what the file says: colour tokens and metrics
merge key by key per variant; the other sections merge key by key; lists (`layouts`, `[[widgets]]`, bar items) are replaced as a whole.
`id`, `version`, `author`, `screenshots`, `tags` and `[integrity]` belong to the theme itself and are never inherited. Loops and chains
deeper than 8 are errors. A package may extend a built-in id (`extends = "nord"` inside a theme named `nord` extends the built-in).

## Commands

`decthemectl theme help` lists them. Output is plain text; `list` and `schema` take `--json`.

| command | does |
|---|---|
| `list`, `info ID` | installed themes (built-in / user / system) with version, author, variants, integrity |
| `schema` | every manifest key |
| `validate PATH` | a folder, a `.dtheme` or one `theme.toml`; all errors with line numbers, then warnings |
| `new ID [--name N] [--from BASE]` | scaffold `~/.config/decebal/themes/ID/` from a theme |
| `install PATH [--force] [--apply]` | check, then copy atomically into the user folder (a built-in id needs `--force`) |
| `uninstall ID` | remove a user theme (not built-ins, not the one in use) |
| `pack DIR [-o FILE]`, `unpack FILE [DIR]`, `hash DIR [--write]` | make / open a `.dtheme`; the content hash |
| `apply ID [--preview SECS] [--no-rdwm]`, `undo` | see below |
| `duplicate ID NEW`, `export ID [FILE]` | an editable copy (flattened, with its assets); a `.dtheme` of any theme |
| `snapshot ID [--name N]` | create a theme from the current settings |

The old commands are unchanged (`decthemectl apply NAME`, `style`, `set`, `dynamic`, `export`, ...); `theme apply` is the one that applies a
whole package.

## What apply does, undo, preview

`theme apply ID` (and the Apply button) is one transaction:

1. resolve the theme (nothing is written if it is broken or unknown);
2. compute the new text of every file it owns: **rdwm `config.toml`** (colours, titlebar style/height/radius, `[rdwm]` keys, wallpaper
   image/mode/colour, validated with `rdwmctl check-config`; a titlebar style the installed rdwm rejects is dropped and reported, the
   rest still applies), **`dock-settings.json`** (style, align, position, magnification, roundness), **`decebal.json`** (theme selection,
   `bar`, `dock`, `hot_corners`, `control_center`, `icons`, `widgets`, `theme_wallpaper`);
3. save the old contents as an undo record (`<shell config>/theme-undo/<time>/`, the last ten are kept);
4. write the files atomically one by one; if any write fails, everything already written is put back;
5. reload rdwm once; ask the "Apply to all apps" pipeline to follow, if the user switched that on.

Keys the running rdwm does not know, or the shell cannot apply yet, are left out and printed as `note:` lines. `theme undo` restores every
file byte for byte (a file the apply created is removed again) and steps back through earlier applies. `--preview N` applies, waits N seconds
(or Ctrl-C) and undoes; the Theme pane's Preview does the same for 8 seconds. The widgets a theme placed carry `"from_theme": true`: the next
apply replaces them, the user's own instances stay. A theme with `match_colors` records it in `theme_dynamic` so the next theme switches
"Match wallpaper" off again. Programs follow within half a second like for every theme change; `decwidgets` rebuilds when its widget files
or `decebal.json` change.

## The Control Center is part of the shell, not of a theme
Every theme gets the quick-settings panel (Wi-Fi, Bluetooth, Focus, Night Shift, dark mode, display and sound sliders, Now Playing, stay awake,
screenshot, notifications, Settings) through the shell, so a theme package never has to ship it. Its entry points:

| shell | button | panel opens |
|---|---|---|
| macOS menu bar | switches icon next to the clock | under the bar, right edge on the button |
| Windows taskbar (`dock.style = "windows"`, top or bottom) | quick-settings button in the tray, left of the clock | against the right screen edge, above (below) the taskbar, in the `fluent` look |
| Caelestia pill bar, left / right / top / bottom | the Control tile of the tray pill | beside the bar, on the screen side |
| Caelestia split | the same tile, in the bottom bar | above the bottom bar |
| any shell, or no bar at all | `decutil control-center toggle` (or `decbar --control-center`), the hot-corner action `control-center` | `control_center.position`; `auto` is beside the bar's own button |

`[control_center]` only chooses what it shows and how: it never runs anything (the section holds names from fixed lists, validated like the
rest). Applying a theme copies the section to `decebal.json` `control_center`, and the Desktop & Dock pane of decsettings edits the same object.
`theme snapshot` exports it. Panels follow the monitor the bars are on (the primary one) and stay inside it on all four edges.

## Safety model

A package is data. The loader enforces, for archives and folders alike (`package::check`):

* **Only data files**: `theme.toml`; `widgets/<name>.toml`; images (png, jpg, webp) whose magic bytes match the extension and whose
  dimensions are at most 16384 px a side and 100 megapixels; fonts (ttf, otf) under `fonts/` with a font header; `.md`/`.txt` in the root.
  Anything else (scripts, binaries, `.so`, `.svg`, other `.toml`, hidden files) rejects the package.
* **No escaping the package**: asset paths are relative, `/`-separated, without `..`, `.`, empty parts, backslashes or control characters;
  every asset must exist inside the package; archives with absolute or `..` paths are refused; case-only duplicates are refused.
* **No links**: symlinks, hard links, devices and pax/GNU extension headers are refused, in folders and archives; widget and
  theme files are read without following links.
* **Caps**: 512 files, 32 MiB a file, 128 MiB a package, 256 KiB manifest, 64 KiB per widget file, 200 widget nodes, depth 8; the
  decompressed stream is cut off while it is read, so a compression bomb cannot fill memory.
* **Content hash**: `[integrity] content_hash = "sha256:..."` covers every file (the manifest without its `[integrity]` table, so the hash can
  live inside it): `sha256(path NUL length NUL sha256(content) LF ...)` over the files sorted by path. `theme pack` writes it; install and
  `theme list` check it and refuse / flag a mismatch. `signature` and `signer` are reserved for the market: a signature will cover
  `content_hash`; they are accepted and stored but not verified yet.
* **No commands from a theme, anywhere**: no key of the format takes a command line, hot corners refuse `custom:`, `[rdwm]` offers
  gaps/borders/layouts/colours only (no binds, rules, `spawn`, autostart), and widgets from a package are parsed as `Trust::Theme`
  (below). Installing never runs anything. `apply` touches only the files listed above.

## Widgets

A widget is a small declarative file, `widgets/<name>.toml`: containers, text with format strings, icons, bars, rings, graphs, images,
workspace dots, and click actions from a fixed list. It is drawn on the desktop by `decwidgets` (an X11 daemon, one small override-redirect
window per widget at the bottom of the stack, SHAPE-cut so rounded cards look right without a compositor and real alpha with one).
The widget uses the theme's colours by name, so it follows theme, mode and wallpaper-colour changes.

```toml
format = 1
id = "clock-card"                  # optional; lower-case id
name = "Clock card"
size = [236, 136]                  # optional; the natural size of the content otherwise
refresh = 1000                     # ms, 250..60000

[root]
type = "col"
padding = [14, 20]                 # all | [vertical, horizontal] | [left, top, right, bottom]
gap = 1
justify = "center"                 # start center end between
background = "popup"               # a theme colour or "#rrggbb", optionally "@alpha": "text@0.1"
radius = "theme"                   # px, or "theme" for the popup radius
border = "border@0.6"
on_click = "open dashboard"

[[root.children]]
type = "text"
text = "{clock:%H:%M}"
size = 54
weight = "bold"
```

**Nodes.** Common to all: `width` `height` `grow` (share of free space in the parent's direction) `padding` `background` `radius` `border`
`border_width` `opacity` `align` (cross axis: start center end stretch) `show_if` (`"battery.present"` or `"!media.playing"`) `on_click`
`on_scroll_up` `on_scroll_down`.

| type | extra keys |
|---|---|
| `row`, `col` | `gap`, `justify`, `children` |
| `stack` | `children` drawn over each other, each filling the box (centre a text over a ring) |
| `button` | like `row`, centred; for clickable icons |
| `spacer` | grows to push siblings apart |
| `text` | `text` (format string), `size`, `weight` (regular/bold), `color`, `text_align`, `max_chars` |
| `ascii` | `text` (format string, e.g. `{fetch.logo}`), `size` (6–40), `color`; multiline fixed monospace grid, up to 40 rows × 80 columns, scales down to fit without wrapping or ellipses |
| `icon` | `name` (a bar symbol such as `play`, `pause`, `speaker-2`, or a format string like `{weather.icon}`), `size`, `color` |
| `bar` | `value` (number or data path), `min`, `max`, `color`, `track`, `height` |
| `analog-clock` | `size` (24–600, default 180), `color` (hands/numerals), `track` (rim/ticks), `second_color`, `numerals` (default true), `seconds` (default true); live local time with smoothly positioned hour/minute hands |
| `ring` | `value`, `max`, `size`, `thickness`, `color`, `track` |
| `graph` | `series` (e.g. `cpu.history`), `max`, `color`, `fill` |
| `image` | `file` (in the package) or `source` (a data path such as `media.art`), `radius_px`, `fit` (cover/contain) |
| `workspaces` | `size`, `gap`, `color`, `active_color`: one dot per rdwm tag, clickable |

**Format strings.** `{path}`, `{path:format}`, `{path:format|fallback}`; `{{` `}}` are literal braces. Formats: `pct` (0.37 -> 37%), `pct1`,
`int`, `.1` (decimals, 0-4), `bytes`, `rate`, `dur`, `mmss`, `upper`, `lower`, and for `clock` the strftime codes `%H %I %M %S %p %a %A %b %B %d %e
%m %y %Y %j %%`. There is no arithmetic, no calls, no nesting.

**Data sources** (`decwidgets data` prints them): `clock` `clock.hour/minute/second`; `cpu.usage/temp/load/cores/history`; `mem.usage/used/total`;
`swap.usage`; `disk.usage/used/total/free`; `net.rx/tx/rx_history/tx_history`; `gpu.present/usage/mem/temp/name`; `battery.present/level/charging/status`;
`volume.level/muted`; `brightness.level`; `workspace.current/count/name`; `window.title/app`; `media.present/playing/title/artist/album/progress/position/length/art`
(MPRIS); `weather.present/temp/icon/code` (the bar's cache); `user.name/display`; `host.name`; `uptime`; `opt.<key>` (instance options);
`cmd.<name>` (user widgets with `allow_exec` only). The daemon samples only the groups some widget names, once a second, and sleeps otherwise.

**Actions** (`on_click`, `on_scroll_up/down`): the whole allowlist is

```
open dashboard|launcher|spotlight|session|notifications|overview
rdwm <action> [arg]       view toggleview viewnext viewprev focusstack focusmon cyclelayout cyclelayout_back setlayout togglefloating
                          togglefullscreen togglemaximize minimize unminimize zoom togglegaps togglebar tileall snap_* focus_* reload
workspace <1..32>|next|prev
dnd toggle     volume up|down|mute     brightness up|down     media play-pause|next|prev
exec <command line>       user widgets only, see below
```
Every action runs a fixed program with fixed arguments, never through a shell (`rdwm` actions allow one argument of `[A-Za-z0-9_.+-]`).
`spawn`, `kill`, `quit`, scratchpads and mouse grabs are not in the list.

**Trust.** Where a widget file comes from decides what it may do (`widgets::Trust`):

| source | trust | `exec` and `[commands]` |
|---|---|---|
| inside a theme package, or a shipped sample | `Theme` | rejected when the file is validated (so `theme install` fails) |
| the user's own folder (`~/.config/decebal/widgets/`, `$DECEBAL_WIDGETS`) | `User` | rejected with a message pointing at the switch |
| the same, with `"widgets": {"allow_exec": true}` in `decebal.json` | `UserExec` | allowed |

With the switch on, a user widget may use `on_click = "exec notify-send hi"` and `[commands.up] run = "uptime -p"  interval = 10` (at least 1
second, 5 s timeout, first line of stdout, 4 KiB cap) read as `{cmd.up}`. Theme widgets never get this, even when the switch is on, and
`theme snapshot` leaves exec-capable widgets out of the package.

**Theme selection.** Applying a theme installs its widget layout when the current layout is untouched. A custom layout requires an explicit Keep/Replace choice in Settings (or `--keep-widgets` / `--replace-widgets` on `theme apply`). Cancel changes nothing. Themes with no enabled widget instances hide the saved collection without deleting it. Palette and mode changes within the same theme preserve widget placement.

**Placing.** Themes place widgets with `[[widgets]]`; you can too: `decwidgets add clock-card --anchor top-right --x -40 --y 60`,
`decwidgets list`, `decwidgets remove clock-card`, or edit `decebal.json` -> `widgets.items`. `decwidgets` (no arguments) runs the daemon;
put it in rdwm's autostart. `decwidgets render NAME --png OUT [--live] [--bg #rrggbb|IMAGE]` draws a widget with fixed sample data for
documentation. Shipped samples (`widgets/*.toml`): `clock-card`, `cpu-ram-ring`, `now-playing-mini`, `workspace-dots`, `sysmon-graph`,
`battery-pill`; refer to them as `builtin:<id>`.

`place = "bar"` is accepted and stored; hosting widgets inside decbar waits for the bar item lists (see the table below).

## The Theme pane

**System Settings -> Theme** has the old tile gallery and, under it, **Theme packages**: a card for every installed theme (its screenshot, or
a miniature desktop in its colours; name, version, author, licence, origin, description, tags, a hash warning) with **Apply**, **Preview** (8 s),
**Duplicate**, **Export** (to `~/Downloads/<id>.dtheme`) and **Delete** (two clicks; user themes only). Below the cards: *Create theme from
current settings...* (type a name, Enter), *Install a theme file* (path of a `.dtheme` or folder), and *Get more themes* (the market stub).

## What the shell applies today

Everything in the manifest is validated; these keys are stored but nothing acts on them yet (they are marked `[pending]` in `theme schema`
and listed as `note:` by `theme apply`):

| key | waiting for |
|---|---|
| `metrics.gap`, `metrics.blur` | no surface reads a UI gap; blur needs compositor support |
| `font.files` | registering bundled fonts with fontconfig (files are validated and shipped) |
| `wallpaper.slideshow/interval/shuffle` | a runner that rotates `theme_wallpaper.images` |
| `decoration.buttons` left/right, `decoration.button_order` | rdwm has no such keys: the style decides (`none` is applied) |
| `bar.left/center/right`, `[[widgets]] place = "bar"`, `slot`, `order` | decbar reading item lists (stored in `decebal.json` `bar.*`) |
| `dock.position` left/right | the dock only has bottom/top (Windows taskbar) |
| `integrity.signature/signer` | the market's key policy |

Wired to the real implementations: fonts (`font.*` -> theme font roles and presets), "Match wallpaper" (`wallpaper.match_colors/scheme/surfaces/strength/follow_mode`),
rdwm titlebar styles (`decoration.style` -> `[decoration] style`), the Caelestia bar position (`bar.position` -> `decebal.json` `bar.position`), icons
(`icons.*` -> the `icons` object), and "Apply to all apps" (an apply asks for one run when the user switched it on; a theme cannot switch it on).

## Tests

* `cargo test`: manifest parse/validate with good and bad fixtures and line numbers, inheritance, the built-in packages against the frozen
  pre-engine files (`tests/fixtures/legacy-builtin`, identical resolved tokens for every theme, variant and override), writer round trips,
  archive and package safety (traversal, links, devices, bombs, caps, hash tampering), install / uninstall / duplicate / export, apply with
  every owned file, undo byte for byte, rollback on a failing write, preview, snapshot -> apply elsewhere; widget DSL parse and errors,
  allowlist and trust enforcement (a theme widget with `exec` is rejected at every entry point), layout goldens
  (`tests/golden/widgets/*.layout`, regenerate with `UPDATE_GOLDEN=1`), a pixel-hash render, hit testing; the pane cards.
* `tests/e2e/theme-engine.sh`: private Xvfb + rdwm; installs the community sample (`examples/themes/sunset-grid`), applies it, checks rdwm's config, the
  dock, the wallpaper and the widget windows, clicks a widget, edits a widget live, previews and undoes, checks the refusals, and drives
  the Theme pane's cards; screenshots in `$SHOTS`.

### Font-independent dot text

`type = "dot-text"` accepts `text` (the same safe format string as ordinary text),
`size` (7–300 px, the full seven-row height) and `color` (semantic token by
 default). It draws an original round-dot alphabet without a font dependency.
ASCII letters are uppercase, digits and basic date/metric punctuation are
supported; unsupported glyphs use a question mark. Measurement and painting both
limit the string to 400 characters. It is intended for short clocks, dates and
metrics, not full multilingual prose. Ordinary `text` remains preferable for
names and unavailable explanations. Existing safe actions and visibility rules
apply; this primitive adds no data sources or execution capability.

New package-font/Dux/explicit-empty-desktop exports require Decebal 0.2.0.
Version 0.2 enforces `min_shell_version`; older 0.1 validators only warned, so
use an updated validator before installing packages that use the new selectors.
Classic-only exports retain the 0.1.0 compatibility floor.
