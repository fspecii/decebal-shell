# Desktop readiness audit

Inspected tree: `d45f075` (includes `837799f`). Read-only rdwm: `64a31e2` at `/home/ambsd/Projects/rdwm`. One pass on 2026-10-30. No feature code was changed.

Audience used as the bar: a desktop people can learn from inside the session, theme without wrecking recovery, and trust when a labelled control says it changed something. Scope stays inside this repository pair. `docs/getting-started.md` already states Decebal is the shell on a window manager, and is not a full session replacement for every KDE or GNOME service.

## Checks run

Private temp directories only. `DBUS_SESSION_BUS_ADDRESS` was unset. `E2E_WALLS` pointed at an empty directory. Displays `:284` and `:287` were free (`/tmp/.X11-unix` had only `X0`). Release binaries were not rebuilt: `decutil` and `decdock` 2026-10-30 02:35, `decthemectl` 03:05, rdwm `target/release/rdwm` 01:32.

| Command | Result |
|---|---|
| `bash tests/e2e/launcher-screenshot.sh` with `E2E_DISPLAY=:287` | Pass. Super+Space → Screenshot, X11 region PNG size, spaced path, clipboard PNG, editor arguments, popup teardown, Escape cancellation. |
| `bash tests/e2e/theme-dock-lifecycle.sh` with `E2E_DISPLAY=:284` | Pass, 9/9. Stale `decdock.off` cleared, duplicate launch ignored, Orbit Glass / Caelestia / Tidal Loom stop the dock, macOS and Windows start one, Tidal Loom → macOS restarts one. |
| Copy of `rdwm-decebal-session` lines 117–120 in a temp `XDG_CONFIG_HOME` | Child shell received `RDWM_TERMINAL=alacritty` and `XCURSOR_SIZE=48`. Parent kept its previous cursor size (`24`) and left `RDWM_TERMINAL`, `DECEBAL_HIGH_CONTRAST`, and `DECEBAL_LOCK_CMD` unset. |
| `pkill -x -SIGUSR2 zzznone` | Exit 1. No matching process. Used only to confirm `pkill` status when the name is absent. |

`tests/e2e/theme-preview-cache.sh` and `make e2e-desktop-smoke` were not run. The smoke target depends on `all` and would rebuild.

## Findings

### 1. Login exports die in a child shell

Verified defect.

- Where: `rdwm` `contrib/rdwm-decebal-session` lines 119–120 run `sh "$_settings"`. Decebal `docs/decsettings.md` lines 128–131 require the login session to source the file. `src/settings/system/session.rs` lines 97–118 write `export` lines. `src/settings/system/defaults.rs` line 165 and `src/settings/system/access.rs` lines 74 and 159 tell the user the login session sources them. `rdwm` `contrib/rdwm-terminal` lines 8–9 read only `RDWM_TERMINAL` from the environment. The activation import at `rdwm-decebal-session` lines 132–133 does not list these variables. Wayland skips the file entirely (`backend = x11` guard on line 119).
- Trigger: save a terminal, cursor size, lock command, or accessibility switch in Settings, then start `rdwm-decebal-session x11`.
- Impact: X11 side effects inside the script still reach the X server (`xrandr`, `setxkbmap`, `xset`, `xinput`, `xrdb -merge`), because those talk to the display. Exported preferences do not enter the session, so `rdwm-terminal` ignores the chosen terminal, toolkit clients keep the earlier `XCURSOR_SIZE`, and `DECEBAL_LOCK_*` never appears for an idle locker. `tests/e2e/session.sh` lines 107–111 only check that the script runs.
- Acceptance: a private Xvfb session whose `session-settings.sh` exports `RDWM_TERMINAL` and `XCURSOR_SIZE` shows those values in the environment of `rdwm` and a supervised panel. A Wayland run records the same limit in the log instead of dropping the file silently. The existing session e2e gains this assertion.

### 2. Accessibility switches do not drive the desktop

Verified defect. Sourcing the session file would not fix this.

- Where: `src/settings/system/access.rs` lines 151–160 say reduce motion affects Decebal apps, text size scales Decebal text, and high contrast asks the theme for its high-contrast variant. `session.rs` lines 102–118 only emit `DECEBAL_REDUCE_MOTION`, `DECEBAL_FONT_SCALE`, and `DECEBAL_HIGH_CONTRAST`. No Rust reader exists in Decebal or rdwm. Theme text size is a different control (`src/settings/themepane.rs` font scale → `theme::live::font_scale`). `themes/high-contrast/theme.toml` is a normal package and is not selected by the toggle. `tests/e2e/decsettings_system.sh` lines 249–252 grep the shell text for cursor size and reduce motion.
- Trigger: enable Reduce motion, move Text size, or enable High contrast, then log in again.
- Impact: animations, shell text, and the active theme stay as they were. The pane reports success ("Saved").
- Acceptance: a private unit test sets each switch and observes a real consumer: motion reads the flag before an animation, text size changes the scale Decebal paints with, and high contrast applies the `high-contrast` package or the row is removed. Labels match that behavior. The e2e asserts the effect, not only the export line.

### 3. Sample-profile screenshot keys skip the capture path that just passed

Verified defect in the default keymap. The Spotlight path is verified working.

- Where: `rdwm` `contrib/config-macos.toml` lines 25 and 353–403 spawn inline `sh -c` for Super+Ctrl+Shift+3/4/5. `docs/getting-started.md` line 24 and `docs/launcher-capture.md` lines 1–22 describe Super+Space → `decutil screenshot` (`src/capture.rs`). The keymap always writes `$HOME/Pictures`, ignores `OMARCHY_SCREENSHOT_DIR`, skips the clipboard and Tensaku, and ends with `exit 0` when every tool is missing. The Wayland branch of key 5 (lines 395–396) selects a region with `slurp` instead of the active window. `rdwm-decebal-session` uses this file when the user has no `config.toml`.
- Trigger: a first X11 or Wayland login, then the cheat-sheet chords at the top of `config-macos.toml`.
- Impact: newcomers following the sample profile get a second capture implementation. Failure is silent. Region shots do not match the cancellation, clipboard, and editor behavior the e2e just passed for Spotlight.
- Acceptance: those three binds call `decutil screenshot` with explicit fullscreen, region, and window modes. A private Xvfb test drives Super+Ctrl+Shift+4 and asserts the same PNG, spaced directory, clipboard, and Escape rules as `tests/e2e/launcher-screenshot.sh`. Missing tools notify and return non-zero.

### 4. A theme apply warns when btop is installed and not running

Verified defect.

- Where: `src/theme/system/omarchy.rs` lines 302–304 and 338–339 call `omarchy-restart-btop` whenever `Env::has("btop")` is true (executable on `PATH`, `mod.rs` lines 337–338). `run` (lines 124–133) records a warning on any non-zero status. The installed helper `/usr/bin/omarchy-restart-btop` is `pkill -SIGUSR2 btop`. `pkill` exits 1 when nothing matches. Settings prints every warning (`src/settings/systempane.rs` lines 37–38). The unit test `apply_stages_the_theme_then_reloads_terminals_and_btop_without_signals` only asserts the helper was called.
- Trigger: apply a theme, or turn on app integration, with `btop` installed and no `btop` process.
- Impact: Appearance → All apps shows a failure-style warning for a reload that had nothing to reload. A real helper failure is indistinguishable from absence.
- Acceptance: a fake-runner test covers installed-but-not-running, running, missing executable, and a non-zero helper status while a process exists. Absence adds no warning. A genuine failure still does. The test must not signal a live `btop`.

### 5. Legacy GTK links are rewritten in the shared style file

Verified behavior, recovery hazard.

- Where: `src/theme/system/gtk.rs` lines 205–236 treat a `gtk.css` symlink into `omarchy/desktop-styles` as a whole-file edit. `src/theme/system/files.rs` lines 600–609 write through symlinks. The test `legacy_linked_full_theme_is_replaced_and_undo_restores_it` (`gtk.rs` lines 341–358) expects the symlink to remain and the target bytes to change, then expects `system off` to restore them.
- Trigger: apply app theming while `~/.config/gtk-4.0/gtk.css` is a symlink at a desktop style under `~/.config/omarchy/desktop-styles`.
- Impact: the style pack itself becomes the Decebal palette until `decthemectl system off`. Anything else that reads that pack changes with it. Undo depends on the system-theme backup, which is separate from `theme undo`.
- Acceptance: after apply, the style-pack file’s bytes are unchanged and `gtk.css` is a regular file whose managed block holds the palette. `system off` removes that file or restores the previous symlink. The existing legacy test is updated to that contract.

### 6. The first-session guide is not part of the session

Untested product gap, plus a doc contradiction with finding 3.

- Where: `docs/getting-started.md` (added in `c26d52f`) is a repository document. First login in `rdwm-decebal-session` lines 106–114 only sets `org.gnome.desktop.interface color-scheme` when it is still `default`, then writes `$XDG_STATE_HOME/rdwm/first-run`. No Decebal code opens the guide. Shortcuts in Settings are available once rdwm is running (`src/settings/panes.rs` shortcut pane).
- Trigger: first `rdwm-decebal-session` login with no user config.
- Impact: the useful table (Spotlight, Settings, widgets, undo) is visible only to someone who already has the git tree. The sample profile’s own cheat sheet still documents the other screenshot chords.
- Acceptance: the installed session can open the same table from Spotlight or Settings without a clone of the repo, and the table’s screenshot row names the keys that actually run `decutil screenshot`.

### 7. A busy dock reconcile is reported as success

Verified code path. The sequential lifecycle e2e passed, so this is not a failure of that test.

- Where: `src/dock_lifecycle.rs` lines 9–10 return `Ok(())` when `single::hold("decdock-lifecycle")` fails. Hold is a non-blocking flock (`src/single.rs` lines 40–48).
- Trigger: two theme applies overlap, or Settings and `decthemectl` reconcile at the same time.
- Impact: the loser skips both the supervisor `decdock.off` update and the start/stop. The dock can stay up on a rail-only theme, or stay down after switching back to macOS, until a later reconcile wins the lock.
- Acceptance: overlapping private reconciles leave exactly one dock when the latest theme wants one, and zero when it does not. A lost lock retries or returns an error the caller surfaces.

### 8. App-theme and capture limits the repo already names

Untested gaps. Do not treat them as newly failed tests.

- Qt pixels: `docs/systemtheming/app-propagation.md` line 39. This machine’s note there already says no Qt demo was installed. Adapter unit tests exist; a window was not captured.
- Open GTK apps keep their old custom CSS until restart (`app-propagation.md` lines 11–12 and 48–49). Light/dark can follow a running Files process. That boundary is documented and easy to miss from the Settings sentence at `systempane.rs` lines 33.
- Live st colours cannot be rolled back to the previous runtime OSC state (`app-propagation.md` line 23). `system off` returns compiled defaults.
- Wayland region capture is implemented in `src/capture.rs` lines 62–78 and has no private-compositor e2e. `docs/launcher-capture.md` claims the X11 script only.
- Screen reader: `rdwm` `docs/ECOSYSTEM.md` line 95 says GNOME `at-spi` autostart entries are skipped via `OnlyShowIn`. Decebal has no Orca or AT-SPI hook.

Acceptance for this item is a written limit, not a new subsystem: the All apps pane repeats the GTK reopen rule; Qt and Wayland capture stay listed as unchecked until a private harness exists; a screen reader is out of the current release gate.

## What already held

- Spotlight/Control Center capture on X11, including cancellation (`launcher-screenshot.sh`).
- Dock process follows theme class: macOS and Windows on, vertical rail and explicit `dock.enabled = false` off (`theme-dock-lifecycle.sh`, `docs/dock-style.md` lines 71–80).
- Theme undo restores shell files and reconciles the dock (`src/theme/engine/apply.rs` lines 688–716). App files roll back through `decthemectl system off`, not through that shell undo record.
- Gallery references are specified in `docs/theme-previews.md` and `tests/e2e/theme-preview-cache.sh`. This pass did not re-run that script.
- Electron GUI launches clear inherited `ELECTRON_RUN_AS_NODE` (`src/launch.rs` lines 382–383) with a unit test in that file.

## Roadmap

First (settings do what they say):

1. Source `session-settings.sh` in the X11 session, or apply an equivalent, and cover it in `tests/e2e/session.sh`.
2. Make the three accessibility controls real, or change the labels and stop exporting unread variables.
3. Point the sample-profile screenshot chords at `decutil screenshot`.
4. Treat “btop not running” as a successful reload no-op.

Next:

5. Stop writing through legacy GTK style-pack symlinks; keep backup/undo.
6. Surface `docs/getting-started.md` from the installed session and align it with the keymap.
7. Retry a dock reconcile that loses `decdock-lifecycle`.
8. Run `tests/e2e/theme-preview-cache.sh` on the release binaries as part of smoke.

Later, only when the harness exists:

- Qt widget pixels in a private HOME, D-Bus, and Xvfb.
- Wayland `grim`/`slurp` capture on a private compositor.
- AT-SPI, if a screen reader becomes a goal.
- Recovering st’s previous runtime OSC colours. Today’s rollback to compiled defaults can stay until then.

## Release-readiness gate

Achievable on this tree. A red item blocks the desktop call; a listed non-goal does not.

1. `tests/e2e/session.sh` shows `RDWM_TERMINAL` and `XCURSOR_SIZE` in the session process after a private X11 login script.
2. Accessibility unit tests match the labels, or the labels match the smaller behavior.
3. `tests/e2e/launcher-screenshot.sh` passes, and a chord test on another private display passes the same PNG and Escape checks.
4. Omarchy fake-runner test: absent btop is silent; a failed reload while btop is running is a warning.
5. GTK legacy test: style-pack bytes unchanged.
6. `bash tests/e2e/theme-dock-lifecycle.sh` and `bash tests/e2e/theme-preview-cache.sh` pass on free displays (the new occupancy guard exits 77 when the display is taken).
7. `make e2e-desktop-smoke` is the bundled command once those scripts are the ones it runs. It rebuilds via `all`; run the scripts directly when a rebuild is not wanted.

Explicitly outside this gate: a new screenshot application, Qt demo installation, a Wayland compositor e2e, Orca, live wallpaper, and more theme packages.
