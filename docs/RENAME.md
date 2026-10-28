# macshell is now Decebal

The project was called **macshell** (programs `macbar`, `macdock`, ...). It is now **Decebal**: a KDE-like, customisable,
rice-friendly desktop environment on [rdwm](https://github.com/fspecii/rdwm). The macOS look is one of the shipped styles (the
`macos` theme and the `mac` dock style keep their ids); Decebal is not a macOS clone.

Nothing of yours is deleted or moved. The first run of any `dec*` program **copies** your old settings to the new names, the old
program names keep working for one release, and the old environment variables are still read.

## What changed

| Before | Now |
|---|---|
| repository / package / crate `macshell` | `decebal` (library `decebal`) |
| `macbar macdock macspot maclaunch macsettings macnotify macthemectl macwm machud macclip machot macdash macsession macswitch macexpose macutil machaven macwidgets` | `decbar decdock decspot declaunch decsettings decnotify decthemectl decwm dechud decclip dechot decdash decsession decswitch decexpose decutil dechaven decwidgets` |
| `macshell-session` (Hyprland session script) | `decebal-session`; session file `hyprland-decebal.desktop` |
| `~/.config/macshell/` (presets, themes, widgets, `session-settings.sh`, `wallhaven.json`, `notifications.toml`, `system/`) | `~/.config/decebal/` |
| `~/.local/state/macshell/` (spotlight and clipboard history, `system/`, `launch.log`) | `~/.local/state/decebal/` |
| `~/.cache/macshell/` (app index, wallpaper colours, album art, wallhaven thumbnails) | `~/.cache/decebal/` (a cache: rebuilt, not copied) |
| `~/.config/omarchy/macshell.json` (or `$MACSHELL_CONFIG/macshell.json`) | `~/.config/omarchy/decebal.json` (or `$DECEBAL_CONFIG/decebal.json`). `dock-settings.json` and `dock-pinned-macos.json` keep their names |
| `/usr/share/macshell/themes`, `/usr/share/macshell/dbus` | `/usr/share/decebal/themes`, `/usr/share/decebal/dbus` |
| `MACSHELL_*` (`_CONFIG`, `_BACKEND`, `_SCALE`, `_THEMES`, `_PRESETS`, `_WIDGETS`, `_LOCK_*`, `_REDUCE_MOTION`, `_FONT_SCALE`, `_HIGH_CONTRAST`, ...) | `DECEBAL_*` |
| `MACBAR_* MACDOCK_* MACSPOT_* MACSETTINGS_* MACNOTIFY_* MACHUD_* MACSESSION_* MACUTIL_* MACHAVEN_* MACEXPOSE_*` | `DECBAR_* DECDOCK_* DECSPOT_* DECSETTINGS_* DECNOTIFY_* DECHUD_* DECSESSION_* DECUTIL_* DECHAVEN_* DECEXPOSE_*` |
| systemd user units / XDG autostart `macshell-{bar,dock,notify,clip,hot,wallpaper-colors}.{service,desktop}` | `decebal-{bar,dock,notify,clip,hot,wallpaper-colors}.{service,desktop}` |
| `macsettings.desktop`, `machaven.desktop` | `decsettings.desktop`, `dechaven.desktop` (`StartupWMClass=DecSettings`) |
| single-instance locks `$XDG_RUNTIME_DIR/macbar.lock`, `macbar-bottom.lock`, `macdash.lock`, ... and message files `machud.msg` | `decbar.lock`, `decbar-bottom.lock`, `decdash.lock`, `dechud.msg`, ... |
| window classes / Wayland app ids and layer namespaces `macbar` `macdock` `macspot` `maclaunch` `MacSettings` `macnotify` `macclip` `machaven`, `macshell-bar`, `macshell-dock` | `decbar` `decdock` `decspot` `declaunch` `DecSettings` `decnotify` `decclip` `dechaven`, `decebal-bar`, `decebal-dock` |
| D-Bus `org.macshell.Notifications` (`org.freedesktop.Notifications` is unchanged) | `org.decebal.Notifications` |
| preset files `*.macshell-preset`, text `MACSHELL-PRESET-1:`, `macshell.json` inside a preset | `*.decebal-preset`, `DECEBAL-PRESET-1:`, `decebal.json`. **The old ones still import** |
| X11 atoms `_MACCLIP_*`, `_MACSHELL_WAKE` | `_DECCLIP_*`, `_DECEBAL_WAKE` |
| Omarchy theme `~/.config/omarchy/themes/macshell`, qt5ct/qt6ct scheme `macshell.conf`, Kvantum theme `macshell` (the optional "Apply to all apps") | `.../decebal`, `decebal.conf`, `decebal` (written on the next apply; the old files stay until you delete them) |
| managed blocks `>>> macshell system theme ... <<<` in GTK / Xresources / terminal files | `>>> decebal system theme ... <<<`; **blocks written by the old version are still recognised** and replaced or removed |

Unchanged on purpose: the `.dtheme` theme package extension, the `macos` theme / style / preset ids, `style = "mac"` and
`style = "windows"` in `dock-settings.json`, the "macOS" labels of that style, `dock-pinned-macos.json`, the `rdwm` names.

## What happens on first run

Every `dec*` program starts with `decebal::compat::init()`:

1. **Environment.** A variable set under its old name (`MACSHELL_CONFIG`, `MACBAR_POWER_SUPPLY`, ...) is copied to the new name
   unless the new one is set. The new name wins when both are set. Foreign programs started from the dock or Spotlight never get either.
2. **Files.** If `~/.config/decebal` does not exist but `~/.config/macshell` does, the folder is copied (permissions kept, so
   `wallhaven.key` stays private) to a temporary sibling and renamed into place, so programs started at the same moment cannot
   see a half copy. The same for `~/.local/state/macshell` and for `macshell.json` -> `decebal.json` next to `dock-settings.json`.
   A note `MIGRATED-FROM-MACSHELL.txt` is left in the new config folder, and the program prints one line to stderr.
   Nothing that already exists under the new name is touched, and the old files are never modified or deleted.
3. **Single instance.** `decbar` (and every other program) also checks the old lock (`macbar.lock`): while a pre-rename `macbar`
   from the running session is alive, a new `decbar` does not start a second bar (toggle apps like `decspot` close the old one instead).
4. **Session settings.** `decsettings` writes `~/.config/decebal/session-settings.sh`. While `~/.config/macshell/session-settings.sh`
   exists it is kept in step too, because the not-yet-updated session script still sources that path.

Settings you change after the first run live in the new places only (the old `macbar` keeps reading `macshell.json`; it does not see them).

## Old program names (deprecated, one release)

`make install` and the PKGBUILD install `macbar`, `macdock`, ..., `macwidgets` and `macshell-session` as a small wrapper
(`contrib/compat/mac-compat`) that runs the `dec*` program with the same arguments and prints a deprecation hint when stderr is a
terminal. Existing rdwm keybinds (`spawn "macspot"`), autostart lines and `Exec=maclaunch` keep working. They will be removed in
the next release: update your own config to the new names. The PKGBUILD `provides/conflicts/replaces` `macshell`.

Not kept: the old window classes (rules matching `class = "macspot"`), the old D-Bus interface name, the `_MACCLIP_*` atoms. Restart
the session (or at least all panels and daemons) after upgrading instead of mixing old and new `macclip`/`machud` processes.

## Migration guide

1. Install the new package (or `make install`), which leaves your old files alone.
2. Update `~/.config/rdwm/config.toml` (see the list below), reload rdwm, and log out and in once (or restart the panels with the new names).
3. Disable old systemd units if you enabled them: `systemctl --user disable --now macshell-bar.service macshell-dock.service macshell-notify.service macshell-clip.service macshell-hot.service macshell-wallpaper-colors.service`
   and enable the `decebal-*` ones instead. Old `/etc/xdg/autostart/macshell-*.desktop` entries are removed by the package; after a
   plain `make install` delete them by hand (the lock check keeps a second copy from starting meanwhile).
4. Optional clean-up once you are happy: `rm -r ~/.config/macshell ~/.local/state/macshell ~/.cache/macshell ~/.config/omarchy/macshell.json`.

### rdwm config (`~/.config/rdwm/config.toml`) in a live session

* Keybinds and autostart: `macspot` -> `decspot`, `macdash`, `macsession`, `macexpose`, `macswitch`, `macclip`, `macnotify history`,
  `machud volume|mic|brightness`, `macthemectl`, ... (the old names still work through the wrappers).
* Window rules: `class = "macspot"` / `"Macspot"` -> `"decspot"` / `"Decspot"`; `class = "maclaunch"` -> `"declaunch"`;
  `class = "Machaven"` -> `"Dechaven"`; `MacSettings` -> `DecSettings`. These do **not** have a compatibility shim.
* Hyprland users (the Hyprland session): `layerrule`/`windowrule` entries matching the layer namespaces `macshell-bar`, `macshell-bar-bottom`, `macshell-dock` or the class `maclaunch` / `machaven` (see `contrib/hyprland-*.lua`) need the new names.
* Anything exporting `MACSHELL_*` in your own scripts keeps working; switch to `DECEBAL_*` when convenient.

### Rolling back

Nothing was moved, so going back is: reinstall the old `macshell` package. It reads `~/.config/macshell`, `~/.local/state/macshell`
and `macshell.json` exactly as you left them. (Settings changed after the migration are in the `decebal` places only; copy them back by
hand if you want them: `cp -a ~/.config/omarchy/decebal.json ~/.config/omarchy/macshell.json`.) Presets exported with the new version
(`DECEBAL-PRESET-1:`, `decebal.json`) are not readable by the old release.

## Changes needed in the rdwm repository (`contrib/rdwm-macshell-session`, not done here)

`rdwm-macshell-session` and its neighbours are in the rdwm repository. This change does not touch it. What has to change there:

**Session wrapper `contrib/rdwm-macshell-session`** (rename it, e.g. `rdwm-decebal-session`, keeping a deprecated `rdwm-macshell-session` that `exec`s it):

* Backend variable: `MACSHELL_BACKEND` -> `DECEBAL_BACKEND` (read `${DECEBAL_BACKEND:-$MACSHELL_BACKEND}`; export both for one release; also in `IMPORT_SH`'s variable list).
* Panels: `PANELS=${RDWM_MACSHELL_PANELS-macbar macdock}` -> `RDWM_DECEBAL_PANELS` default `decbar decdock` (honour the old variable as a fallback).
* Services: `RDWM_MACSHELL_SERVICES` -> `RDWM_DECEBAL_SERVICES`, default `decnotify polkit idlelock decclip dechot wsosd`; in the `case`:
  `macclip) have macclip && tend macclip "$_t" macclip daemon` -> `decclip) have decclip && tend decclip "$_t" decclip daemon`;
  `machot` -> `dechot daemon`; `wsosd) have machud && tend wsosd "$_t" machud watch` -> `dechud watch`. Supervisor labels (pid files) follow the names.
* State directory `$XDG_RUNTIME_DIR/rdwm-macshell` (and `/tmp/rdwm-macshell-$(id -u)`) -> `rdwm-decebal`; the `rdwm-svc-` markers are unchanged.
* Per-user build: `$XDG_DATA_HOME/rdwm-macshell/bin` -> `$XDG_DATA_HOME/rdwm-decebal/bin` (also accept the old path).
* Session settings: `${XDG_CONFIG_HOME:-$HOME/.config}/macshell/session-settings.sh` -> `.../decebal/session-settings.sh` (fall back to the old path when the new file is missing).
* All log/usage strings `rdwm-macshell-session: ...`, the header comments, and the `RDWM_MACOS_CONFIG` default (`config-macos.toml` can keep its name).
* `contrib/rdwm-autostart`: `RDWM_AUTOSTART_SKIP` default `"macshell-bar macshell-dock macshell-notify"` -> `"decebal-bar decebal-dock decebal-notify"` (keep the old three too).
* Programs it greps/looks for with `have`/`command -v`: `macclip machot machud macbar macdock macnotify` -> the `dec*` names.

**Desktop files**: `contrib/rdwm-macshell.desktop` and `contrib/rdwm-macshell-wayland.desktop` -> `rdwm-decebal.desktop` /
`rdwm-decebal-wayland.desktop` (`Name=rdwm + Decebal (X11)`, `Exec=rdwm-decebal-session x11`, `TryExec=rdwm-decebal-session`; Wayland: `... wayland`).
Keep the old two (Exec pointing at the old wrapper name) for a release so a display manager's remembered session still works.

**Config templates** `contrib/config-macos.toml` and `contrib/config.toml`: every `macspot macnotify machud macclip macdash macsession macexpose macswitch macbar`
in `args = [...]`/shell snippets; window rules `class = "macspot"` (twice: `macspot` and `Macspot`) -> `decspot`/`Decspot`, `class = "maclaunch"` -> `declaunch`,
`class = "Machaven"` -> `Dechaven`; comments and `command = "macbar &"` (`contrib/config.toml:125`, and the same sample in `crates/rdwm-core/src/config.rs:1504,1544`).

**Docs/packaging/other**: `README.md`, `docs/{CONFIG,DESIGN,ECOSYSTEM,PARITY}.md`, `man/rdwm.1`, `man/rdwm.5`, `CONTRIBUTING.md`, `CHANGELOG.md`;
`Makefile` (`rdwm-macshell-session`, `rdwm-macshell*.desktop`, `MACSHELL_DIR ?= ../macshell` -> `DECEBAL_DIR ?= ../decebal`, `install-desktop`/`uninstall-desktop`);
`PKGBUILD` (`optdepends=('macshell: ...')` -> `decebal`, description); `packaging/macdesktop/*` (the meta package, its build script and README: package names,
`rdwm-macshell-session`) and `packaging/README.md`; `contrib/sddm/Main.qml:34` (`n.indexOf("macshell")` -> also match `decebal`) and `packaging/macdesktop/sddm-*`;
`contrib/portals/rdwm-portals.conf` comment; `tests/e2e/{macshell.sh,x11-macos.sh,session.sh}` and `tests/package.sh`; code comments mentioning `macbar/macdock`
in `crates/rdwm-core/src/{api,schema}.rs`, `crates/rdwm-x11/src/lib.rs`, `crates/rdwm-core/src/tests/snap.rs`, `crates/rdwm/src/main.rs`.

**Live user configuration** (not in any repository): `~/.config/rdwm/config.toml` binds and window rules as listed above;
`~/.local/share/rdwm-macshell/` (per-user build) if you have one; SDDM's remembered session; any `systemctl --user` enablement of `macshell-*.service`.
