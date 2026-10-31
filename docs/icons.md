# Icon themes

Every decebal program that draws application, folder or MIME icons (the dock, the launchers `declaunch` and
`decspot`, the window switcher `decswitch`, the overview `decexpose`, the taskbar style, notifications and the menu
bar's popups) looks them up through one module, `src/icons.rs` and `src/icons/*`, following the freedesktop
[Icon Theme Specification](https://specifications.freedesktop.org/icon-theme-spec/latest/). The theme is a setting.

## Choosing a theme

* System Settings -> Icons (`decsettings icons`): the installed themes (Auto first) with a preview grid of 16 common
  icons for the highlighted one, **Use this theme**, icon size, symbolic tint and fallback theme.
* `decthemectl icons list | set NAME|auto | status | reset | scale N | tint on|off | fallback NAME|none`.
* Icon and cursor packs of desktop ports (Deepin, openKylin, Lingmo, KDE, GNOME), installed from pinned and verified
  upstream sources with their licences: `decthemectl assets` and [desktop-asset-packs.md](desktop-asset-packs.md). The
  Icons pane also selects the cursor theme, independently of the icon theme.
* By hand, in `decebal.json` (the folder of `DECEBAL_CONFIG`, default `~/.config/omarchy`):

```json
"icons": {
  "theme": "WhiteSur-dark",
  "size_scale": 1.0,
  "monochrome_tint": false,
  "fallback": "Adwaita"
}
```

| key | default | meaning |
| --- | --- | --- |
| `theme` | `"auto"` (absent) | Theme folder name (`Papirus-Dark`) or its display name. `auto`/empty follows the desktop. |
| `size_scale` | `1.0` | 0.6 - 1.25. How much of its square the artwork fills (themes differ in padding). The square, and so the dock layout, does not change. |
| `monochrome_tint` | `false` | Repaint symbolic icons (`*-symbolic.svg`, `symbolic/` folders) in the shell theme's text colour. |
| `fallback` | `"Adwaita"` | Theme tried after the chosen theme, its `Inherits=` parents and `hicolor`. `"none"` for no extra theme. |

All keys are optional; unknown or mistyped values are ignored (the default is used). `decthemectl icons reset`
(or the pane's Reset) removes the whole object. The object is a plain JSON value, so presets and bundles that copy
keys of `decebal.json` generically carry it along with `theme`, `dock`, `bar`, ...: `icons` is the key to export.

**Auto** asks, in this order: `gsettings get org.gnome.desktop.interface icon-theme`, `gtk-3.0/settings.ini`
`gtk-icon-theme-name`, `gtk-4.0/settings.ini`; if none names an installed theme only hicolor and the fallback are used.
An explicit theme that is not installed (or has only symbolic / cursor files) is reported in `decthemectl icons status`
and the pane, and the desktop's choice is used meanwhile.

## Lookup

1. Roots: `$XDG_DATA_HOME/icons` (`~/.local/share/icons`), `~/.icons`, `$XDG_DATA_DIRS/icons`
   (`/usr/local/share:/usr/share`); unthemed files in the matching `pixmaps` folders come last.
2. Chain: the theme, its `Inherits=` (depth first, loops cut), `hicolor`, the fallback theme (and its parents).
3. Per theme: `index.theme` `Directories`/`ScaledDirectories` with `Size`, `MinSize`, `MaxSize`, `Scale`, `Threshold`, `Type`.
   The directory with the smallest `DirectorySizeDistance` to the wanted pixels wins (a `@2x` folder counts `Size * Scale`
   pixels); ties go to the larger directory, then scalable. A directory listing per theme is read once, so a miss costs a hash
   lookup, not a `stat` per folder.
4. Names (`src/icons/names.rs`): the id as given, lower case, dock aliases (`trash` -> `user-trash`, `finder` ->
   `system-file-manager`), reverse-DNS classes (`org.mozilla.firefox` -> `firefox`), MIME names (`text-x-python` ->
   `text-x-generic`), naming-spec generalisation (`network-wired-disconnected` -> `network-wired`), then the generic
   application icon (`application-x-executable`). Ids that are absolute paths (`Icon=/opt/x/icon.png`) are loaded directly.
   The `.desktop` mapping (`Icon=`, `StartupWMClass`, reverse-DNS suffix) is unchanged (`switcher::resolve_icons`).
5. Formats: SVG (resvg, drawn at the target size on whole pixels), PNG (exact area average when shrinking, bicubic when
   growing), XPM (small built-in reader). Nothing found: the built-in Launchpad and trash drawings, then a neutral tile.

## Memory

Resolved paths (and misses) live in one LRU of 768 entries per process. Each window's `IconCache` holds at most 320
pixmaps / 10 MiB, least recently used out first; the settings preview keeps 128 pixmaps / 6 MiB. Theme listings are
a few hundred KB for the largest themes and only for themes in the chain.

## Hot reload

The theme watcher every program already runs (`theme::live::start_watcher`, 500 ms of `stat`s) also checks
`decebal.json`, the GTK / dconf files and the icon folders; a change drops the caches, bumps `icons::generation()` and
wakes the event loop. Programs without the watcher (notifications, the switcher popup) poll the same files at most every
700 ms when they draw.
