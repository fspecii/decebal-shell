# Configuration pane and presets

System Settings > System > **Configuration** (`decsettings config`) has two halves: a text editor for the raw
configuration files, and **presets**: named snapshots of the whole setup that you can apply, undo, export and share.
Code: `src/settings/configpane.rs`, `src/settings/editor.rs` (the widget), `src/preset/` (everything that touches files).
Command line: `decthemectl profile --help`.

## Edit files

Tabs: **Shell** (`decebal.json`), **Dock** (`dock-settings.json`), **Pinned apps** (`dock-pinned-macos.json`),
**Window manager** (rdwm's `config.toml`, the same file decsettings edits) and **All settings**, a read-only listing of every
effective setting with the defaults filled in and marked `# default` (rdwm's key table from `rdwmctl schema`, plus the
decebal defaults).

The editor is a small multi-line widget of its own (the toolkit only had single-line fields): line numbers, JSON/TOML colouring,
cursor, mouse and keyboard selection (double click = word, triple click = line, drag), undo/redo that groups typing,
Ctrl+C/X/V with the system clipboard (`wl-paste`/`xclip`, an internal buffer otherwise), Tab/Shift+Tab indent, auto-indent.
Esc leaves the editor; Ctrl+S saves.

* **Validation** runs on every key (syntax + the typed checks decbar/decdock use, hot-corner actions, theme mode) and again
  on Save (for the rdwm file also `rdwmctl check-config`, the same code as `rdwm --check-config`). Problems show with line
  and column in the status strip, as a note under the editor, and as a red line in the text. **An invalid file is never written.**
* **Saving** is atomic (temp file, fsync, rename; symlinked dotfiles are written through, the mode is kept) and keeps the old
  version as `NAME.bak.YYYYmmdd-HHMMSS` next to the file (the newest 20 are kept). **Revert** puts the newest backup back; the
  version being replaced becomes a backup too, so Revert twice goes forward again.
* **Live**: the rdwm file is reloaded over IPC right after a save; decbar, decdock and the theme follow their JSON files
  within half a second, as for every other pane. A file changed on disk while you have unsaved edits shows a warning
  (Reload discards your edits; saving overwrites). Unmodified buffers follow the disk silently.

## Presets

A preset is a folder `~/.config/decebal/presets/<id>/` (`$DECEBAL_PRESETS` overrides) with `manifest.toml` and the files
that carry its parts. **Save as preset** lets you choose the parts:

| part | what | file in the preset |
|---|---|---|
| Shell and theme | `decebal.json` except the keys below: theme, mode, accent, `bar` (incl. `bar.position`), `dock`, `system`, workspaces, clock; a custom theme file from `~/.config/decebal/themes` travels along | `decebal.json`, `themes/<id>.toml` |
| Fonts | `theme.font`, `theme.fontScale` | `decebal.json` |
| Icons | `icons` (the icon theme settings; ticked by default when present) | `decebal.json` |
| Hot corners | `hot_corners` | `decebal.json` |
| Dock | `dock-settings.json` | `dock-settings.json` |
| Pinned apps | `dock-pinned-macos.json` (machine specific, off by default) | `dock-pinned-macos.json` |
| Window manager | rdwm: everything except the tables below | `rdwm.toml` |
| Keyboard shortcuts | rdwm `[[bind]]`, `[[mode]]` | `rdwm.toml` |
| Autostart and plugins | rdwm `[[autostart]]`, `[[plugin]]`, `[[scratchpad]]` (they run programs; off by default) | `rdwm.toml` |
| Wallpaper | rdwm `[wallpaper]`, optionally with the image embedded (up to 24 MB) | `rdwm.toml`, `wallpaper/<file>` |

rdwm's `[[output]]` (monitor layout) is machine specific and is never saved or replaced. Keys that look like credentials
(`apiKey`, `token`, `password`, `secret`, ...) are removed from everything a preset captures, at any depth, so a Wallhaven key
or similar never ends up in a file you share. The system-wide theming *state* (`~/.local/state/decebal/system`) is not part
of a preset either; only the `system` settings key in `decebal.json` is.

**Apply** always shows a preview first: every file with the settings that would change (`+` added, `~` changed, `-` removed,
capped), notes (e.g. a wallpaper that does not exist here) and every command the preset would run. Then: everything is
validated before the first byte is written (rdwm through `rdwmctl check-config`); a **restore point** preset
`before-<name>` of the parts being replaced is saved; the files are written with their timestamped backups; if a write
fails, the files already written are put back. **Undo** applies the restore point. rdwm is reloaded when its file changed.

Modes: a preset made from your setup **replaces** the parts it contains (so "no hot corners" really clears them). The
built-in presets **merge**: they change only the keys they mention (`null` removes a key), so applying "Caelestia" does not
touch your workspaces, clock format, pinned apps, shortcuts or autostart.

Built-in presets (read only, generated from the built-in themes and `theme::style`): **macOS default**, **Caelestia**,
**Windows-like**, **Minimal tiling** (no dock or decebal bar, rdwm's own slim bar, thin borders, no titlebars, small gaps).
Duplicate one to edit it. Per preset: Details, Apply, Duplicate, Rename, Delete (not built-ins), Export, Copy to clipboard.

## Sharing: export and import

**Export** writes one `NAME.decebal-preset`: a gzip-compressed tar with `manifest.toml` first, then the files above.
**Copy** puts the same bundle on the clipboard as text (`DECEBAL-PRESET-1:` + base64; not for presets with a wallpaper).
**Import** reads a file (path field, or Browse with `zenity` when installed) or the clipboard text.

Importing is deliberately strict, because bundles come from other people. Nothing is extracted to disk by the reader; the
entries are checked in memory first:

* only these names are accepted: `manifest.toml`, `decebal.json`, `dock-settings.json`, `dock-pinned-macos.json`,
  `rdwm.toml`, `themes/<id>.toml`, `wallpaper/<name>.{png,jpg,jpeg,webp}`; anything else (`run.sh`, a folder, ...) is refused;
* absolute paths, `..`, empty/`.` components, backslashes, NULs and control characters are refused (path traversal);
* symbolic links, hard links, devices and fifos are refused; pax headers are skipped, never obeyed; GNU long names refused;
* limits: 1 MB per text file, 16 KB manifest, 24 MB per image, 24 entries, 48 MB expanded (a decompression bomb stops at
  the cap), 40 MB bundle; duplicate names and bad header checksums are refused;
* the manifest must be valid (format 1, known parts, sane name), every part must have its file and every file a part;
  JSON files must pass the same typed checks as the editor, TOML must parse, a theme must load, an image must really
  be an image;
* nothing is ever executed by the importer.

The **preview** lists the files and the same change summary as Apply. **Commands**: rdwm starts `[[autostart]]` and
`[[plugin]]` entries by itself and runs `spawn` shortcuts and scratchpads with your full user rights, and `custom:` hot
corners run a shell line. The preview lists every one of them with where it is configured. The switch "Let these commands
run" is **off** by default: the imported copy then has them removed (autostart tables, plugins, scratchpads, all `spawn`
shortcuts, custom hot corners; the Autostart part is dropped) and the manifest says `commands_stripped = true`. Applying
a preset with commands shows the list again and the button reads "Apply, I trust these commands".

## Command line

```
decthemectl profile list [--json]
decthemectl profile show NAME
decthemectl profile save NAME [--parts shell,dock,...|all] [--description TEXT] [--embed-wallpaper] [--overwrite]
decthemectl profile apply NAME [--dry-run] [--allow-commands]
decthemectl profile undo NAME
decthemectl profile duplicate NAME [NEW] | rename NAME NEW | delete NAME
decthemectl profile export NAME FILE | import FILE [--strip-commands | --allow-commands] [--apply]
```

`import` and `apply` refuse (exit 1) a preset that would run commands unless you pass `--allow-commands` (keep) or, for
import, `--strip-commands`.

## manifest.toml

```toml
format = 1
name = "Cosy"
description = "Nord, dock on the left"
author = ""
created = "2026-10-28T12:00:00Z"
app_version = "0.1.0"
parts = ["shell", "fonts", "dock", "rdwm", "keys", "wallpaper"]
mode = "replace"            # or "merge"
wallpaper_file = "wallpaper/pic.png"   # optional
theme_file = "themes/mine.toml"        # optional
commands_stripped = true               # set by a stripping import
origin = "imported"                    # local | imported | backup
```

## Tests

`cargo test` covers: bundle round trip and the hostile cases (traversal, absolute, symlink and hard link, devices, unknown names,
oversized, bomb, duplicates, bad checksum, pax), manifest validation, part splitting and merging, secrets, command
finding and stripping, atomic save + timestamped backup + revert, plan/apply/undo/rollback, the editor model (insert,
delete, undo, selection, line numbers, tokens) and the validation error mapping. `tests/e2e/configpane.sh` (Xvfb `:91` + rdwm, private
HOME and XDG dirs; `SHOTS=dir` keeps screenshots) types into the editor, saves, saves a preset, exports it, imports it in a
second private HOME with its own decsettings and runs hostile bundles through `decthemectl profile import`.
