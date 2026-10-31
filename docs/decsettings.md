# decsettings

System Settings for the rdwm + decebal desktop: a native window (own tiny-skia/fontdue toolkit, no GTK/Qt/web)
that edits `~/.config/rdwm/config.toml` in place, applies changes live and follows edits made by hand.
Window class `DecSettings` (rdwm draws the titlebar). Launch again to toggle it closed (like the other decebal windows).

```
decsettings [PANE]          # general appearance tiling windows rules keyboard autostart displays
                            # old names still work: theme icons wallpaper desktop (-> Appearance tabs); appearance/fonts|window|dock|colours|apps
                            # + system panes: sound network bluetooth notifications inputdev power defaults datetime users privacy storage accessibility config (see "System panes" below; `config` = text editor + presets, docs/presets.md)
decsettings --config PATH   # edit another rdwm config
```
Environment: `DECSETTINGS_CONFIG`/`RDWM_CONFIG` (config file), `DECEBAL_CONFIG` (omarchy folder with the JSON files),
`RDWMCTL` (path of rdwmctl), `RDWM_SOCK`, `DECSETTINGS_SCALE` (UI scale; on X11 `Xft.dpi/96` is used),
`DECSETTINGS_DEBUG_LAYOUT=file` (dumps the control map after every paint; used by the e2e test).
Keys: Tab/Shift+Tab/arrows move, Space/Enter activate, Left/Right adjust sliders, Ctrl+F search, Esc or Ctrl+W close.
Right-click a control to reset it to the default (removes the key from the file).

## Source of truth and safety
* The TOML file is the only state. Every change is a `toml_edit` edit of the parsed document: comments, key order,
  trailing comments of replaced values and unknown keys survive (`src/settings/tomlcfg.rs`, round-trip tests).
* Write path: the window never writes. A control change becomes an immutable job for the one persistence worker
  (`src/settings/persist.rs`, thread `decset-persist`, details in [settings-persistence.md](settings-persistence.md)); the
  change is shown at once with a "Saving…" toast (AT-SPI: busy) and only the committed result says "Saved". In the worker:
  refuse if the file changed on disk since this process read or wrote it (the window then reloads it) -> apply the edit's
  keys to the file as it is -> validate with `rdwmctl check-config` on a scratch copy (same code as `rdwm --check-config`;
  syntax only when rdwmctl is missing) -> write temp file next to the target, fsync, rename (symlinked configs are written
  through, mode kept) -> one-time `config.toml.bak-decsettings` of the original -> `reload` over IPC. A rejected or failed
  write is shown inline (red banner) and only that change is taken back; nothing is written. rdwm not running: the file is
  still edited.
* "Restore original" (General) puts the backup back.
* dock/menu-bar settings are the few keys decdock/decbar really read (`dock-settings.json`: appSpacing, magnification,
  transparency, roundness; `decebal.json`: workspaces; `dock-pinned-macos.json`: pinned/order). Other keys in those files are kept.
  Both programs read them at start, so the pane has "Restart Dock/Menu Bar" buttons. (The task text says `shell.json`; this repo's decbar reads `decebal.json`.)

## Two-way sync
* A watcher thread stats the config + the three JSON files every 500 ms (two `stat`s, no CPU otherwise) and asks rdwm for
  state/config/`bind list` every second; it wakes the UI only when something changed.
* External change -> `Store::check_external` re-parses; the UI is rebuilt from the documents on the next paint, so focus,
  scroll, selected rule, open popups and text being edited are untouched; toast "Reloaded from disk". A half-written or
  invalid file keeps the last good values and shows a banner until it parses again.
* Runtime state (wallpaper set with `rdwmctl wallpaper`, monitors, binds, last reload error) comes from IPC; the wallpaper
  pane previews what rdwm really shows and offers "Save to config" when it differs from the file.

## rdwm side (branch `settings-api` in rdwm)
`rdwmctl config` (effective settings, defaults filled in, + last reload error), `rdwmctl schema [--offline]` (key table:
type, range, default, choices, description; layouts with symbols; action names), `rdwmctl check-config PATH`.
The table lives in `rdwm-core/src/schema.rs`; its tests feed every key to the real parser, compare each section's key list
with the field list serde reports for an unknown key, and compare defaults with `Config::default()`. decsettings reads the
table at start (`rdwmctl schema --offline`, bundled snapshot `src/settings/rdwm-schema.json` as fallback) and a test checks
that every key the panes use exists in it (and the snapshot against an installed rdwmctl).

## Appearance pane
One pane (`decsettings appearance`, source `src/settings/appearance.rs`) holds everything about how the desktop looks, in five tabs:
Themes (desktop designs and layout) · Palettes (colour sets, light/dark/auto, Match wallpaper colours, accent) · Customise (sections Colours, Fonts, Window style, Dock & Bar,
Control Center once it has settings; each with a one-line explanation and a reset) · All apps (one switch, per-app choices under Advanced).
Wallpaper and Icons are panes of their own next to Appearance (see below). The rationale and before/after screenshots are in `docs/settings-design.md`.
It arranges the builders of themepane, fontpane, iconspane, systempane and wallhaven; the shared decebal theme: miniature desktops, light/dark/auto,
accent swatches, translucency, corner radius and text size. A choice edits rdwm's colour keys through the same
comment-preserving store (validated, one reload) and writes `decebal.json`; every running decebal program follows within
half a second. Details: `docs/theming.md`. (`src/settings/themepane.rs`)

## Icons
A pane of its own next to Wallpaper (`decsettings icons`, `src/settings/iconspane.rs`, sidebar section Appearance): the installed icon themes (Auto first; symbolic-only and cursor-only ones
greyed out), a 4 x 4 preview of common icons for the highlighted theme, Use this theme, icon size, symbolic tint, fallback
theme and reset. It edits the `icons` object of `decebal.json` (`docs/icons.md`). The old `Appearance > Customise > Icons`
section is no longer shown; the route still resolves to the pane.

## Panes
General (versions, file, reload, open in editor, restore, tags, chord timeout) · Appearance (see above: window style = title bars, borders, colours, window buttons; Dock & Bar = dock sliders, menu-bar workspaces, hot corners, pinned apps; Wallpaper tab = My backgrounds and Wallhaven, see wallpapers.md, live preview, mode, colour, path, lazily decoded thumbnails of `~/.config/omarchy/backgrounds/**`) · Config & Presets (the old Configuration pane) ·
Tiling & Layouts (all layouts as pictograms, default/cycle order, mfact/nmaster/gaps, per-tag layout, per-layout options) ·
Windows & Snapping (focus, `[snap]`; `snap.activation` appears as soon as rdwm's schema has it) · Rules (add/edit/reorder/delete,
"from a running window") · Keyboard (all binds incl. chords and modes, search, conflicts, key recorder, add/remove) ·
Autostart & Plugins (autostart, plugins, scratchpads) · Displays (monitors, per-output wallpapers, Wayland outputs).

Layout pictograms: `settings::icons::layout_rects/layout_icon` (pure, shared with whatever draws layout icons elsewhere).

## Keyboard pane details
Binds come from rdwm's `bind list` (built-ins, file, IPC) merged with the file's `[[bind]]`. Rebinding a file bind edits it
(keeps `mods`/`key` vs `keys`); rebinding or removing a built-in writes an `action = "none"` bind for the old keys (+ the new
bind). `keys = [...]` sequences and `[[mode]]` binds are listed; modes/sequences are read-only for rebinding.
The recorder captures the pressed key plus modifier chips (Super/Ctrl/Alt/Shift) because the window manager grabs Super
combinations that are already bound (they never reach the window). Function keys come through `WindowApp::unmapped_key`.

## Tests
`cargo test` (unit: TOML editor, atomic writes, sync, widgets, panes, binds, schema), and
`tests/e2e/decsettings.sh` (Xvfb :95 + rdwm x11 + xdotool, private HOME/XDG dirs; `SHOTS=dir` saves screenshots).

# System panes (branch `decsettings-system`)

decsettings also covers the system side of the desktop. All of it lives in `src/settings/system/` and is registered through
the pane list (`panes::PANES`, ids below) and the sidebar sections (`system::registry::SECTIONS`).

| Pane (id) | Backend | Notes |
|---|---|---|
| Sound (`sound`) | `pactl -f json` (preferred) or `wpctl status/get-volume` | default in/out, volume + mute per device, per-app streams |
| Network (`network`) | `nmcli -t` | Wi-Fi radio, scan, join (password sheet for secured networks), saved/forget, VPN toggles, ethernet + IP/DNS |
| Bluetooth (`bluetooth`) | `bluetoothctl` | power, discoverable, 8 s scan, pair (+trust+connect), connect, disconnect, forget |
| Displays (`displays`, extended) | `xrandr --query` + `xrandr --output ...` | resolution, refresh, rotation, position, primary, mirror/extend, enable; **15 s keep/revert**; scale (rdwm `[decoration] scale` + `Xft.dpi`); Night Shift when gammastep/redshift exist |
| Keyboard & Input (`inputdev`) | `setxkbmap`, `localectl list-x11-keymap-layouts`, `xset r rate`, `xinput list/list-props/set-prop` | layout, Caps Lock behaviour (XKB option), key repeat, pointer speed / natural scrolling / tap-to-click per device (libinput properties); trackpad swipes (rdwm `[gestures]`, see below). The old key-binding editor is now "Shortcuts" (`keyboard`) |
| Notifications (`notifications`) | file `~/.config/decebal/notifications.toml` | see schema below |
| Power & Battery (`power`) | `powerprofilesctl`, `xset q/s/dpms`, sysfs `power_supply`, logind.conf (read-only) | battery card only when a battery exists |
| Default Apps (`defaults`) | `xdg-mime query/default`, `xdg-settings`, `.desktop` files | candidates are the installed apps whose `MimeType=` claims the type |
| Date & Time (`datetime`) | `timedatectl show/list-timezones/set-timezone/set-ntp`; `decebal.json` | polkit refusals are shown inline, a password is never asked |
| Users & Login (`users`) | /etc/passwd, /etc/group, `~/.face` | picture: path of an image, cropped to 256x256 PNG |
| Privacy & Security (`privacy`) | session settings | screen-lock command / idle time / lock on suspend |
| About & Storage (`storage`) | /proc, /etc/os-release, `lspci -mm`, `statvfs` | "Check config" runs `rdwmctl check-config` |
| Accessibility (`accessibility`) | session settings | cursor size, reduce motion, text size, high contrast |

## Rules every system pane follows
* External tools only through `system::run::Runner` (real: spawn + hard timeout, 8 s default, kills a hung tool; fake: recording, used by
  the unit tests). Nothing runs on the UI thread: loads and changes are jobs on short-lived worker threads whose results come back through a
  queue drained in `tick()`. Optimistic updates keep sliders from jumping back.
* A missing tool or a failing backend shows "Not available" plus the reason (never a crash or a hang): e.g. no `nmcli` -> "NetworkManager's nmcli is not installed", `bluetoothctl show` hanging -> "The Bluetooth service did not answer".
* Polling: one poller thread, parked unless the visible pane has a poll interval (sound 3 s, network 6 s, bluetooth 5 s, displays 10 s, power 5 s, date/time 30 s; the others load when opened and after each change).
  It also stops while the window is unmapped (`WindowApp::set_visible`, driven by Map/UnmapNotify on X11).
* Every setter validates its input (ranges, names against what the backend reported, MAC addresses, timezone names, XKB option syntax ...) and reports errors inline.
* The last change of a pane can be undone (an "Undo" row at the top of the pane runs the inverse commands and restores the session file).

## Display confirmation
Every layout change (mode, rate, rotation, position, primary, enable, mirror) starts a countdown (`system::confirm::Confirm`, a pure state machine;
15 s, `DECSETTINGS_CONFIRM_SECS` overrides it for tests). The pane shows "Keep changes" / "Revert"; when the countdown ends the previous
layout is restored by re-applying the layout captured *before* the first unconfirmed change. A second change while one is pending keeps the original revert.
As a backstop that survives a crashed or closed window, a detached `sh` waits `secs+3` and re-applies the old layout unless a flag file
(`$XDG_RUNTIME_DIR/decsettings-keep-*`) says it was kept. Keeping a layout stores the full `xrandr` argument list in the session settings.

## Session settings (applied at login)
`~/.config/decebal/session-settings.json` is the source of truth (keys: `kbd_layout`, `kbd_variant`, `kbd_options`, `repeat_delay`, `repeat_rate`,
`xinput[]` (`device`, `prop`, `value`), `blank_secs`, `dpms_secs`, `cursor_size`, `xft_dpi`, `terminal`, `reduce_motion`, `font_scale`, `high_contrast`,
`lock_cmd`, `lock_secs`, `lock_on_suspend`, `xrandr[]`, `night`). From it decsettings generates **`~/.config/decebal/session-settings.sh`**, a POSIX
shell snippet. It exports `RDWM_TERMINAL`, `XCURSOR_SIZE`, `DECEBAL_REDUCE_MOTION`, `DECEBAL_HIGH_CONTRAST`, `DECEBAL_FONT_SCALE`, `DECEBAL_LOCK_CMD`,
`DECEBAL_LOCK_SECS`, `DECEBAL_LOCK_ON_SUSPEND` and, when `DISPLAY` is set, runs the guarded `xrandr` / `setxkbmap` / `xset` / `xinput --set-prop` /
`xrdb -merge` / night-shift commands (each skipped if its tool is missing). **The login session must source it**:
```sh
f="${XDG_CONFIG_HOME:-$HOME/.config}/decebal/session-settings.sh"; [ -r "$f" ] && . "$f"
```
(The updated `rdwm-decebal-session` transfers known exports from a bounded child after the display starts and before rdwm/autostart.) Both files are written atomically; the `.sh` always
reflects the latest in-memory settings even when several changes happen quickly.

Terminal choice: `rdwm-terminal` honours `RDWM_TERMINAL` (a command name); the pane exports it from the session file, so it applies from the next login.
"Automatic" removes the override (rdwm-terminal then picks the first terminal that works in the session).

## Notifications schema (`~/.config/decebal/notifications.toml`)
```toml
dnd = false                 # do not disturb: no banners or sounds (notifications still go to the history)
banner_seconds = 5          # 1..=60, 0 = until clicked
muted_apps = ["Slack"]      # notification app_name, matched case-insensitively: no banner, no sound
```
Missing file = defaults above; comments and unknown keys are kept when decsettings rewrites the file; written atomically. decnotify reads it
(re-reading on change is up to the daemon).

## Menu bar clock
`decebal.json` (in the omarchy config dir) gets `clockHour12` (bool, default false) and `clockDate` (`weekday-day-month` default, `day-month`,
`weekday-day`, `none`). decbar reads them once a minute (`menubar::clock::format_with`); the defaults give exactly the old `Mon 26 Oct  03:08`.

## Sidebar and search
Panes are grouped by `registry::SECTIONS` (Network & Connectivity, Appearance, Displays, Sound, Notifications, Keyboard & Input, Windows, Apps, System).
The sidebar scrolls. Search (Ctrl+F) matches pane titles/keywords first, then individual settings: the static index `registry::SETTINGS` for the system panes, `appearance::INDEX` for every setting of the Appearance tabs (a unit test fails when a row is not indexed; a hit opens its tab and section)
and the live row labels of the older panes ("volume" -> Sound > Volume, "gap" -> Tiling > Gap between windows). Results show as `Pane > Setting` rows; Enter/click jumps to the pane.

## Tests
`cargo test --lib` has parser tests for pactl/wpctl/nmcli/bluetoothctl/xrandr/xinput/xset/timedatectl/powerprofilesctl/lspci/passwd fixtures, command-line tests with a recording
`FakeRunner`, the confirm state machine, the search index and the session file generator. `tests/e2e/decsettings_system.sh [display]` drives every new pane on a
private Xvfb with rdwm and the fake tools in `tests/e2e/fakes/` (screenshots in `$SHOTS`); `tests/e2e/decsettings.sh` still covers the original panes.

## Accessibility and targeted navigation

`decsettings welcome` (also `help` or `getting-started`) opens the native first-session
guide. A specific destination navigates an existing Settings instance without
closing it or discarding drafts. Requests use a private per-display local socket,
with bounded data-only messages. An explicit conflicting `--config` is refused.
If the runtime directory exceeds Linux's Unix-socket path limit, navigation uses
a short socket under a user-owned mode-0700 `/tmp/decsettings-UID` directory. Its
name includes separate runtime-directory and display hashes, so independent
runtime directories keep separate navigation endpoints. Ordinary runtime paths
keep their existing names. An unsafe owner or directory mode is refused.
X11 requests reveal/focus through the WM; Wayland retains compositor focus policy.
The legacy singleton lock is still shared across displays, so a request to a
Settings instance on another display fails rather than closing it.

Accessibility exports are consumed by newly started Decebal processes. Text size
80–200% overrides the theme's text scale in memory; **Use theme** clears that override; high contrast supplies opaque,
readable shell colors; reduced motion disables dock magnification and launch bounce.
Stored theme, palette and widget layout are retained. Settings rows, controls and
wrapped notes adapt to large text. This is not screen-reader integration or a
claim that every external application supports these preferences. Complex custom
widget/editor layouts still need their own large-text checks.

The session generator writes explicit zero/empty values when preferences are
turned off, so activation environments do not retain stale overrides. The updated
rdwm wrapper transfers known exports from a bounded child before launching the WM
and imports even an explicit empty font override. Installing that system wrapper
requires administrator access; an older installed wrapper still drops the exports.

When a theme asks whether to replace a custom widget layout, Escape cancels that
pending choice and keeps Settings open without saving changes. Outside an edit,
popup or pending choice, the documented sidebar Escape close shortcut remains.


Wallpaper also exposes an optional rounded desktop frame on supporting rdwm
sessions: enable/disable, corner radius, thickness and colour. Resetting colour
uses the wallpaper background. It sits behind applications and works independently
of theme selection. An older live WM hides these controls even when a newer
offline control tool is installed. The frame does not move or resize windows.

### Wallpaper gallery cache

Local wallpaper thumbnails use a bounded 32 MiB/512-entry cache. The current
viewport and one margin row remain resident; offscreen tiles keep navigation
and hit geometry without promoting their images in the cache. Scrolling replaces
the pending decode list, with one active decode and at most two queued results.
This fixes the old 32-entry loop where a tall gallery needed 44–49 images and
loaded tiles repeatedly disappeared and reappeared.

`DECSETTINGS_BIN=target/release/decsettings python3 tests/e2e/wallpaper-cache.py`
checks 73 synthetic images on private Xvfb at 1759×1194 and 2600×1700, including
visible pixels after redraw, bounded memory and zero decoding once settled.

### Live window-shadow support

Window shadows use the running WM's advertised support, not just a newer
`rdwmctl` schema. X11 keeps its existing live-parser check. A Wayland session
(including Settings forced through XWayland) additionally requires the live
`output_query.effects` protocol 1 reply with `backend="wayland"` and
`ssd_shadows=true`. Older or disconnected compositors keep the controls
unavailable without changing saved values. Client-decorated apps still manage
their own shadows.

The existing background runtime watcher performs the query. Paint only reads
its cached result. Socket replacement invalidates a mixed-instance poll;
coalesced, ordered runtime observations prevent a late reload worker from
restoring support after a newer disconnect. Identical idle observations do not
request another repaint.

## Trackpad swipes (Keyboard & Input)
rdwm recognises native three- and four-finger swipes (`[gestures]`, rdwm `docs/gestures.md`). The section at the
end of Keyboard & Input (`src/settings/gestures.rs`) offers a control for each `[gestures]` key the loaded schema lists
(the installed `rdwmctl schema --offline`, else the bundled snapshot): **Swipe gestures** (`enabled`), **Required
swipe travel** (`threshold`, with the schema's range, in rdwm's logical touchpad delta units, not millimetres) and
**Reverse swipe direction** (`gestures.natural`, independent of the pointer's natural scrolling). They save like every
other control (schema range check, `rdwmctl check-config`, the persistence worker); a change touches only its key.
* The bundled snapshot carries rdwm's published `[gestures]` rows and `gesture_bindings` (rdwm 1181481, imported
  verbatim from its `rdwmctl schema --offline`, sha256 39d8c65b…; the rest of the snapshot unchanged). An installed
  rdwm whose schema does not list `[gestures]` hides the controls: the section says they need an updated rdwm and
  writes nothing; a `[gestures]` section already in config.toml stays as it is.
* Bindings are listed with their args (3/4 fingers, direction, action; rdwm's schema default when config.toml has
  none; `[]` means every swipe is off) but not edited here (`table_list` gets no control): they are an inline list in
  config.toml, kept exactly as written; edit them in Config & Presets, whose overview shows the default as TOML.
* A schema entry says rdwm can parse the keys, not that the touchpad, driver and session deliver swipe events.
* Search: "swipe", "gesture", "trackpad" lead to the section.
