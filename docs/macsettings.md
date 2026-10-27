# macsettings

System Settings for the rdwm + macshell desktop: a native window (own tiny-skia/fontdue toolkit, no GTK/Qt/web)
that edits `~/.config/rdwm/config.toml` in place, applies changes live and follows edits made by hand.
Window class `MacSettings` (rdwm draws the titlebar). Launch again to toggle it closed (like the other macshell windows).

```
macsettings [PANE]          # general appearance desktop wallpaper tiling windows rules keyboard autostart displays
macsettings --config PATH   # edit another rdwm config
```
Environment: `MACSETTINGS_CONFIG`/`RDWM_CONFIG` (config file), `MACSHELL_CONFIG` (omarchy folder with the JSON files),
`RDWMCTL` (path of rdwmctl), `RDWM_SOCK`, `MACSETTINGS_SCALE` (UI scale; on X11 `Xft.dpi/96` is used),
`MACSETTINGS_DEBUG_LAYOUT=file` (dumps the control map after every paint; used by the e2e test).
Keys: Tab/Shift+Tab/arrows move, Space/Enter activate, Left/Right adjust sliders, Ctrl+F search, Esc or Ctrl+W close.
Right-click a control to reset it to the default (removes the key from the file).

## Source of truth and safety
* The TOML file is the only state. Every change is a `toml_edit` edit of the parsed document: comments, key order,
  trailing comments of replaced values and unknown keys survive (`src/settings/tomlcfg.rs`, round-trip tests).
* Write path (`io::Store::commit`): refuse if the file changed on disk since it was read (then reload it) ->
  validate the new text with `rdwmctl check-config` on a scratch copy (same code as `rdwm --check-config`; syntax only when
  rdwmctl is missing) -> write temp file next to the target, fsync, rename (symlinked configs are written through, mode kept)
  -> one-time `config.toml.bak-macsettings` of the original -> `reload` over IPC on a thread. Invalid values are shown inline
  (red banner) and nothing is written. rdwm not running: the file is still edited.
* "Restore original" (General) puts the backup back.
* dock/menu-bar settings are the few keys macdock/macbar really read (`dock-settings.json`: appSpacing, magnification,
  transparency, roundness; `macshell.json`: workspaces; `dock-pinned-macos.json`: pinned/order). Other keys in those files are kept.
  Both programs read them at start, so the pane has "Restart Dock/Menu Bar" buttons. (The task text says `shell.json`; this repo's macbar reads `macshell.json`.)

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
with the field list serde reports for an unknown key, and compare defaults with `Config::default()`. macsettings reads the
table at start (`rdwmctl schema --offline`, bundled snapshot `src/settings/rdwm-schema.json` as fallback) and a test checks
that every key the panes use exists in it (and the snapshot against an installed rdwmctl).

## Panes
General (versions, file, reload, open in editor, restore, tags, chord timeout) · Appearance (titlebars, borders, colours,
traffic lights, rdwm's own bar) · Desktop & Dock (dock sliders, menu-bar workspaces, pinned apps add/remove/reorder) ·
Wallpaper (live preview, mode, colour, path, lazily decoded thumbnails of `~/.config/omarchy/backgrounds/**`) ·
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
`tests/e2e/macsettings.sh` (Xvfb :95 + rdwm x11 + xdotool, private HOME/XDG dirs; `SHOTS=dir` saves screenshots).
