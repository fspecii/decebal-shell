# Settings: one home for appearance

## The problem

Everything about how the desktop looks was spread over seven sidebar entries. A person who wants "dark, bigger text, a different
dock" had to guess between **Appearance** (title bars and borders), **Theme** (themes, accent, fonts, "Apply to all apps",
Match wallpaper, theme packages, translucency), **Icons**, **Wallpaper**, **Desktop & Dock** (dock, menu bar, hot corners) and
**Configuration** (presets, import / export, text editing), and the Theme pane alone was a 30-screen list with two different
"theme" pickers (tiles and packages) that did different things.

Before (sidebar of the old build, Display & Appearance section):

| Old pane | Old home of |
|---|---|
| Appearance | title bars, borders, window buttons, rdwm's own bar |
| Theme | Shell style, theme tiles, theme packages, mode, Match wallpaper, accent, translucency, radius, text size, fonts, Apply to all apps |
| Icons | icon theme, size, tint |
| Wallpaper | My backgrounds, Wallhaven |
| Desktop & Dock | dock, menu bar, hot corners, pinned apps |
| Configuration (System) | presets, import / export, text editor |

Screenshots: `docs/settings-design/before-*.png` (theme, appearance, icons, wallpaper, desktop, config).

## The design

One sidebar entry, **Appearance**, with four tabs. The tabs follow the order of a person's needs: pick a look, adjust it,
choose a picture, extend it to other apps.

```
Appearance
  Theme       Style (macOS / Windows / Caelestia) · Themes (cards with live preview, Apply, Preview, Export...) ·
              Light or dark (Light / Dark / Auto) · Match wallpaper colours · link to Customise and to Config & Presets
  Customise   one section at a time, each with a one-line explanation and "Reset to theme default"
                Colours       accent colour · colour scheme tiles · translucency · corner radius
                Fonts         text size · font preset · interface / titles / monospace / clock fonts
                Window style  windows follow the theme · title bars · window buttons · borders · their colours
                Dock & Bar    dock style, spacing, magnification... · menu bar workspace dots · window manager strip · hot corners · pinned apps
                Control Center  (a slot: hidden until the Control Center has settings, see below)
  Wallpaper   My backgrounds / Wallhaven · scaling · background colour · image file · link to "Match wallpaper colours"
  Icons       icon theme (preview grid) · size · tint · fallback
  All apps    one switch "Apply theme to all apps"; under "Advanced" -> "Show options": per-app targets, GTK font,
              icon and cursor theme for apps, Apply now
Config & Presets  (sidebar entry right under Appearance: presets, import / export, edit the files as text)
```

Sidebar sections: Network & Connectivity · **Appearance** (Appearance, Config & Presets) · Displays · Sound · Notifications ·
Keyboard & Input · Windows · Apps · System. Displays moved out of the old "Display & Appearance" group because it is a hardware
setting, not a look.

### Why these choices

* **Theme is the landing tab and is about whole looks.** The old Theme pane had tiles (colours only) and packages (colours,
  fonts, window style, dock, wallpaper), which looked like the same list and behaved differently. Now the Theme tab shows only
  the packages as cards, with a sentence saying what a theme is. The colours-only tiles are kept (the rice-level power) but live
  in Customise > Colours as "Colour scheme", and say so.
* **Customise shows one section at a time.** A section is short enough to read, it has one reason to be open, and a reset that
  does what it says: Colours resets accent, translucency and radius; Fonts resets the font choices; Window style removes the look
  keys from rdwm's file and lets the theme paint them again; Dock & Bar resets the dock sliders; Icons resets the icon settings.
  Behaviour (double-click on a title, middle click) is never touched by a reset.
* **Wallpaper is not a second entry.** It is a tab, and the one setting that touches it ("Match wallpaper colours") lives only
  on the Theme tab; the Wallpaper tab has a button that opens it instead of a second switch. `decsettings wallpaper` opens the tab.
* **"Apply to all apps" is one switch.** The seven per-target switches, options, icon and cursor theme were always visible and
  alarming; they now sit behind "Advanced > Show options".
* **Advanced stays reachable and named.** Presets, import / export and the text editor are a sidebar entry called
  "Config & Presets" next to Appearance (it is a full-width editor and does not fit in a tab), with a button on the Theme tab.
* **Words.** User-facing names are Theme, Style, Icons, Fonts, Wallpaper, Dock, Bar, Window style. "Decoration", "titlebar",
  "rdwm's own bar" and "monocle stacking" are gone from labels (the keywords still match them in search).
* **No setting is editable in two places.** The window-style "corner radius" is "Window corner radius" (rdwm's titlebar) and
  the theme's "Corner radius" (menus, cards) is a different key; the dock "Dock style" changes the dock only, the Theme tab's
  "Desktop style" changes theme, dock and menu bar together and says so.

### Compatibility

Nothing stored changed: `config.toml`, `decebal.json`, `dock-settings.json`, `decthemectl` and every saved format are as before;
the tabs only arrange the builders that already existed. Old names keep working (`decsettings theme|icons|wallpaper|desktop`
open the Theme tab / the Icons pane / the Wallpaper pane / Customise > Dock & Bar; `appearance` opens the Theme tab; `appearance/fonts`,
`appearance/window`, `appearance/dock`, `appearance/colours`, `appearance/apps` are explicit forms). The dock's "Settings" entry
(`decsettings desktop`) lands on Dock & Bar.

### Search

Ctrl+F finds every setting by name. `appearance::INDEX` lists each setting with plain-language and old words ("titlebar",
"decoration", "traffic lights", "chrome", "hex"); a hit opens the tab *and* the section. A unit test builds every tab and section and fails
when a row is not in the index, so a new setting cannot be unfindable; `tests/e2e/appearance.sh` checks the old names and 19
searches end to end.

### Slot for the Control Center

`appearance::control_center_items(&Model) -> Option<Vec<Item>>` returns `None` today. When it returns `Some(items)`, a "Control
Center" section appears at the end of the Customise section bar. No other file needs to change.

## Scroll performance

The same change fixes a laggy scroll. Causes found with the paint counters in the layout dump (`frames`, `items_us`, `paint_us`):

1. every repaint, including each wheel notch and each mouse move, **rebuilt the whole item list**; on the Theme pane that read
   theme folders, installed fonts, icon themes and the "apply to all apps" status for 27 ms;
2. off-screen theme cards and tiles were painted (and their text shaped) every frame;
3. the scroll offset was fractional, so text and borders shimmered between frames.

Fixes: the items of the last paint are reused while only the wheel, the pointer or a slider drag happened since (any key, click,
timer or file change rebuilds them); theme cards, tiles and the icon preview grid are laid out but not painted when scrolled out of
view (Tab navigation still reaches their buttons); the scroll offset is rounded to whole pixels when drawing.

`tests/e2e/scroll-perf.sh` drives 400 wheel notches (down / up) on a private Xvfb for each long pane and prints the number of
frames, mean paint time and CPU; it fails when the mean paint time exceeds 25 ms. Measured on the development machine (400 notches, mean paint per frame):

| Pane | before | after |
|---|---|---|
| Theme | 40.7 ms (27-33 ms of it rebuilding the items) | 5.1 ms |
| Icons | 5.2 ms | 3.7 ms |
| Dock & Bar (old Desktop & Dock) | 5.1 ms | 3.7 ms |
| Wallpaper | 8.5 ms | 7.3 ms |
| Window style (old Appearance) | 4.2 ms | 3.3 ms |

The window also keeps up now (the Theme pane painted 217 of 400 requested frames before because each took 40 ms). Idle: no change in
CPU (the e2e idle check still reports about 2 timer wakeups in 10 s) and RSS stays at ~16.5 MB on opening (16.3 MB before) and 21 MB
after visiting every pane.

## Screenshots

`docs/settings-design/`: `before-*.png` (old panes) and `after-*.png` (Theme, Customise sections, Wallpaper, All apps, Config & Presets).
