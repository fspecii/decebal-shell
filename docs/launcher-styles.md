# Launcher styles

`declaunch` draws one of seven launcher styles. All of them list the same installed applications (the
`.desktop` scan in `src/apps.rs`), search with `src/fuzzy.rs`, launch through the same typed
`launch::spawn_app` path, and take their colours from the current theme, so each works with every theme
in light and dark mode. None of them ships artwork: icons come from the user's icon theme and the few glyphs
(search, power, chevrons, star, gear, grid) are vector paths drawn in code.

| Key | Title | Placement | Categories | Pinned | Recent | Paged | Power |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `default` | Launchpad | centred card | | | | | |
| `dux` | Dux | centred card | | | | | |
| `windows` | Windows Start | anchored popup | | yes | yes | yes | yes |
| `gnome` | GNOME | full screen | | yes (dash) | | yes | |
| `kde` | KDE Kickoff | anchored popup | yes | yes | yes | | yes |
| `xfce` | Xfce Whisker | anchored popup | yes | yes | yes | | yes |
| `blackberry` | BlackBerry 10 | full screen | yes (Hub) | yes | yes | yes | |

The registry is `src/launcher/style.rs` (`LauncherStyle`, `KEYS`, `Capabilities`). Renderers:
`src/launcher.rs` and `src/launcher/dux.rs` (the two original looks) and `src/launcher/styled/*.rs` on the
shared engine in `src/launcher/styled.rs`. Placement is `src/launcher/placement.rs`.

## Choosing a style

* `decebal.json` → `"launcher": {"style": "<key>"}` is the user's choice. Settings → Launcher writes it.
* Absent, empty, `"theme"` or an unknown key follow the theme's `launcher_style` token (`[launcher] style` in a
  manifest). The token's choice list grew from `default|dux` to all seven keys; old manifests and configs keep
  working, and an unknown token falls back to `default`.
* Theme apply, palette and mode changes never write the `launcher` key (`theme::engine::apply` tests
  `theme_palette_and_mode_changes_never_touch_the_users_launcher_choice` and
  `old_manifests_and_configs_without_new_launcher_keys_still_work`).
* The Settings selector shows what is saved, never the theme's style: "Use theme default" is its own option
  and the one shown when nothing is saved. A line below says which style is in effect and why.

## Interaction (every style)

* Type anywhere to search (live results); Backspace/Delete edit, Ctrl+U clears.
* Arrow keys move inside a list or grid and on into the neighbouring area (sidebar, dash, footer);
  Tab / Shift+Tab move between areas; PageUp/PageDown jump a screenful; Home/End go to the ends.
* Enter opens the focused item (with a search and nothing focused: the best match). Keyboard focus draws a
  focus ring, hover only a highlight.
* Pointer: hover highlights, click opens, right click (or Ctrl+P) pins or unpins an app, the wheel scrolls a
  list or turns a page of a paged grid.
* Escape closes, so does a click outside the card (including on the panel). Pins are saved to
  `launcher-pinned.json` next to `decebal.json`; until the first pin the dock's pins are used, then a browser,
  file manager, terminal, editor and settings app, topped up to six.
* "Recent" and the search bonus use the usage history file Spotlight writes too. The launcher records each
  launch as `appid:<desktop id>` (stable when two apps share a `Name=`) and, only while the name is unique,
  also as Spotlight's `app:<Name>`. Reading prefers id keys and accepts a name key only when exactly one
  installed app has that name, so older history keeps working and ambiguous entries are ignored.
* The wheel scrolls at most a screenful less one row per event, so a short list never skips an item.
* Power buttons open the session menu (`decsession`, which asks before acting); gear buttons open Settings.

## Panel adapter contract

The panel's launcher applet (owned by the universal-panel work) starts:

```
declaunch [toggle|open|close] [--edge top|bottom|left|right] [--anchor X,Y] [--thickness N] [--output NAME]
```

* `toggle` (the default, also with no verb): open, or close the running launcher. `open` opens unless one is
  already open (that one is left alone). `close` closes a running launcher and never opens one. Every
  intentional close exits with status 0 (SIGUSR1 to the holder), and a closing invocation waits (up to 2 s) for
  the holder to release its lock. Every request, whatever its verb, first takes a separate
  `declaunch.request.lock` (released before the UI starts), so overlapping invocations run one after another
  and their close signals cannot coalesce: a burst of N concurrent toggles leaves the launcher open exactly
  when N is odd (`tests/e2e/launcher-concurrent-toggle.py`, which pauses its own holder with SIGSTOP to make
  the overlap deterministic). `open` on a running launcher only probes it and returns at once. A real SIGTERM
  is still reported as a failure.
* `--edge`: the screen edge of the panel holding the applet.
* `--anchor X,Y`: the applet's centre in the panel window's own coordinate space: X11 root-window coordinates;
  on Wayland, output-local surface coordinates.
* `--thickness N`: the panel's thickness in the same units. The launcher never draws over that strip (it is
  left transparent, so the panel shows through; a click there closes the launcher).
* `--output NAME`: on X11 the popup opens on the RandR monitor of that name; without it, on the monitor holding
  the anchor point; without either, on the panels' (primary) monitor. An unknown name is logged and falls back.
* Missing pieces are inferred: an anchor without `--edge` uses the nearest screen edge; without `--thickness`
  the anchor is taken to be the middle of the panel (thickness = twice its distance from the edge, at most a
  third of the screen). An edge without an anchor opens from the middle of that edge.
* Unknown or malformed arguments are ignored with a message on stderr; they never stop the launcher.
* With no panel information (keyboard shortcut, hot corner) each style opens in its natural place: Windows
  centred above a 48 px bottom taskbar, Kickoff from a 44 px bottom panel, Whisker under a 28 px top panel,
  GNOME and BlackBerry full screen, Launchpad and Dux centred.
* Anchored styles (Windows, KDE, Xfce) open from the applet towards the screen, centred on it along the panel,
  with the style's gap to the panel (12 / 4 / 2 logical px) and an 8 logical px margin elsewhere, shrinking to
  fit small screens. Full-screen styles (GNOME, BlackBerry) ignore the anchor point and cover everything but the
  panel strip. Launchpad and Dux centre in the space the panel leaves.

`placement::placement(style, Option<Anchor>, screen, scale) -> Rect` is pure; its tests cover every style × the
four edges × scales 1, 1.25, 1.5, 2 × applets at both ends and the middle × screens from 800×480 to 3840×2160:
always on screen, never over the panel strip, gap and margins as above.

### Units and scale

The card size is logical pixels × the UI scale: X11 `Xft.dpi / 96`, or `DECEBAL_SCALE` when set. On Wayland the
launcher draws in surface pixels (no buffer scale), so the scale is 1 unless `DECEBAL_SCALE` says otherwise.

## Hosting

On Wayland every style uses the full-output overlay layer surface the launcher always used. On X11 the five new
styles open a full-monitor override-redirect popup: with a compositor it is 32-bit and only the card is opaque;
without one the window is shaped to the card, and the card is painted opaque. Launchpad and Dux keep their
card-sized X11 popup; with a panel anchor it gets the size and position `placement()` gives.

## Sources and attribution

Layouts and metrics were taken from public documentation and source code, read only; no code, artwork or
proprietary assets were copied. Names of products are used only to describe the style a user picks.

* Windows 11 Start: Microsoft Learn, *Geometry in Windows 11*
  (<https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/geometry>): 8 px corners for
  top-level containers and flyouts, 4 px for in-page elements such as buttons and list backplates. Microsoft
  Support, *Customize the Windows Start menu*
  (<https://support.microsoft.com/en-us/windows/pin-and-unpin-apps-to-the-start-menu-10c95188-5f75-bb6c-3fab-cfd678ac8476>):
  Pinned, Recommended, All apps, pin/unpin. The 642 × 726 size, 6 × 3 pinned grid, 12 px taskbar gap and 64 px
  footer are measurements of the shipping Start menu, not published numbers.
* GNOME: gnome-shell `js/ui/iconGrid.js` (`defaultGridModes` 8 × 3, 6 × 4, 4 × 6, 3 × 8; `IconSize` 96, 64, 48,
  32, 24, 16, tried largest first) and `js/ui/appDisplay.js` (horizontal paging and page indicators, scroll to
  turn pages, the Applications search provider); GNOME HIG, *Search* (type to search, live results) and
  *Popovers* (Escape closes).
* KDE Plasma Kickoff: plasma-desktop `applets/kickoff/main.qml` and `FullRepresentation.qml` (header with avatar
  and search, sidebar of categories, content area that becomes the search view and hides the sidebar while
  searching, footer; `favoritesDisplay` / `applicationsDisplay` grid-or-list switches).
* Xfce Whisker Menu: xfce4-whiskermenu-plugin `panel-plugin/settings.cpp` (menu-width 450, menu-height 500,
  launcher-show-description on, category-icon-size Smaller, launcher-icon-size Small, default favourites
  browser, mail, file manager, terminal), `icon-size.cpp` (Smaller = 24 px, Small = 32 px) and `window.cpp`
  (title box, search, contents; categories on the right in left-to-right locales; the panel's position only
  moves the window).
* BlackBerry 10: the official BlackBerry 10 UI Guidelines PDF could not be retrieved (BlackBerry's developer
  site is gone). The design rests on secondary descriptions: the BlackBerry jQuery Mobile BB10 theme README
  (<https://github.com/blackberry/jQueryMobile-BB10-Theme/blob/master/docs/README.md>, the action bar in a fixed
  footer), GSMArena's BlackBerry Classic review (a paged grid of all apps) and CrackBerry / LaptopMag coverage
  of the Hub beside Active Frames. It is labelled "BlackBerry 10-inspired" for that reason.

## Fidelity gaps (known, not hidden)

* Windows: on short menus the best-match pane drops its large header (a small header row, or only Open and Pin
  side by side, Open naming its app when there is room); Open and Pin always stay inside the menu.
  No Recent files or "Recently added" entries in Recommended (apps only, from the usage history); no
  pinned-tile drag reordering, folders or the alphabet jump list; the search pane shows apps only (no web,
  settings or files); the Start menu's acrylic/mica blur is a flat translucent card.
* GNOME: no app folders, no drag-and-drop reordering, no workspace thumbnails; the overview background is the
  theme's background dimmed, not the blurred wallpaper; search shows only the Applications provider.
  "Show Apps" in the dash closes the launcher (it returns to the desktop).
* KDE Kickoff: only the Applications tab (no Places tab), the Leave button opens decsession instead of
  separate Sleep / Restart / Shut Down buttons, no pin-to-keep-open button, no per-app context menu actions.
  Favourites show as a grid and other views as a list; main.qml has `favoritesDisplay` / `applicationsDisplay`
  switches between the two, but their defaults (the config schema) could not be retrieved, so this choice is
  not verified against Plasma's defaults.
* Xfce Whisker: two command buttons (Settings, session menu) instead of Whisker's configurable set; no
  "Edit applications", no recently-used list beyond the shared history; descriptions are the untranslated
  `Comment=` (localized `Comment[xx]` values are not read for these styles yet).
* BlackBerry: no Active Frames (there are no app thumbnails to show), no Hub messages: the Hub lists app
  categories with counts; the bottom bar shows search, page indicator and close instead of phone / search /
  camera. Gestures (peek, swipe up) are not implemented.
* Category icons use the icon theme's `applications-*` names; themes that dropped them (recent Adwaita) get a
  tinted tile with the category's initial instead.
* Wayland: `--output` is not honoured; the compositor chooses the output of the overlay surface (normally the
  focused one), and anchors are taken as coordinates on that output. On X11 the output is honoured as described
  above.
* X11 output choice is checked natively on two RandR monitors (a private `Xvfb -noreset` with `xrandr
  --setmonitor LEFT/RIGHT`): `--output RIGHT`, `--output LEFT`, an anchor on RIGHT, and `--output LEFT` with an
  anchor on RIGHT (the output wins), for every style, by the mapped window's geometry and an untouched other
  monitor. Monitors arranged above one another and RandR hot-plug while open are not exercised.
* Launchpad and Dux do not apply the UI scale to their own drawing (unchanged from before); their card size
  follows `placement()` and so the screen.

## Evidence

See "Evidence" in the final report of this change for the run that produced them. Re-run with:

```
CARGO_TARGET_DIR=... cargo build --release --bin declaunch --bin decspot --example launchapp
LAUNCHER_BIN_DIR=$CARGO_TARGET_DIR/release LAUNCHER_STYLES_EVIDENCE=/var/tmp/launcher-styles python3 tests/e2e/launcher-styles.py
LAUNCHER_BIN_DIR=$CARGO_TARGET_DIR/release LAUNCHER_RAPID_EVIDENCE=/var/tmp/launcher-rapid python3 tests/e2e/launcher-rapid-toggle.py
LAUNCHER_BIN_DIR=$CARGO_TARGET_DIR/release python3 tests/e2e/launcher-concurrent-toggle.py
LAUNCHER_BIN_DIR=$CARGO_TARGET_DIR/release LAUNCHER_ACTIONS_EVIDENCE=/var/tmp/launcher-actions python3 tests/e2e/launcher-windows-actions.py
```

`launcher-styles.py --interactions-only` skips the capture matrix; `LAUNCHER_STYLES=windows,kde` narrows the
styles. `launcher-windows-actions.py` gives every fixture app its own executable, so the app that runs proves
which result Open acted on.

They all run in a private HOME / XDG tree with their own Xvfb display and no session bus, and never touch the live
desktop. `manifest.json` / `result.json` record the declaunch sha256 and the source commit of every capture.
