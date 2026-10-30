# Launcher screenshot capture

Super+Space → Screenshot and Control Center → Screenshot share
`decutil screenshot`. It waits for Spotlight to release its input grab before
starting selection. X11 uses `maim` and a real destination filename, with
Flameshot/gnome-screenshot as interactive fallbacks. Wayland uses grim/slurp.
An installed Hyprland helper is not selected merely because it is on PATH.

Images save to the XDG Pictures folder (`OMARCHY_SCREENSHOT_DIR` overrides it).
X11 copies PNG data with xclip; Wayland uses wl-copy. Tensaku opens the saved
image when installed, with the clipboard command matching the active backend.
Escape cancels selection; errors produce a notification. `--fullscreen` skips
selection.

Spotlight's argv commands use the shared launch path for logging and immediate
startup error reporting. GUI Electron programs clear inherited
`ELECTRON_RUN_AS_NODE`, which would otherwise run Obsidian and similar apps as
Node instead of opening their windows. Explicit per-app launch rules still win.

`tests/e2e/launcher-screenshot.sh` exercises the real Super+Space shortcut and
X11 region selection on a private desktop, checking PNG dimensions, paths with
spaces, clipboard data, editor arguments and cancellation.
