# App colour propagation

Theme changes apply to supported apps automatically. An explicit `system.enabled = false` in `decebal.json` opts out; `decthemectl system off` restores saved configuration. Targets can be disabled independently in Appearance → All apps or with `decthemectl system target NAME off`. Package install, privilege prompts and restarting/terminating applications are not part of the adapter pipeline.

For an isolated process or non-app desktop test, `DECEBAL_SYSTEM_AUTOMATIC=0` suppresses automatic propagation before configuration reads, state writes or child processes. It does not change the saved preference or normal default. Explicit `apply-system` and `system on` remain intentional app-integration commands. Desktop E2Es set this override in their headers/common environment. App-theming E2Es enable automatic propagation only after private HOME/XDG paths and private D-Bus activation are established; a private HOME alone does not isolate a session bus inherited from the live desktop.

## Paths and actual behaviour

| Target | Configuration | Running applications |
| --- | --- | --- |
| GTK 3 | Managed `gtk-3.0/gtk.css`, selected keys in `settings.ini`, GNOME settings | Light/dark preference follows supported settings notifications; custom CSS is read at process startup |
| GTK 4 / libadwaita / Files | Managed `gtk-4.0/gtk.css` with modern CSS variables and legacy named colours, GNOME settings | Existing Files follows light/dark; a new process reads the full custom palette. Existing processes cannot be forced to reload arbitrary user CSS through a supported external API |
| st / st-256color | Scoped OSC colour updates and a guarded managed Bash/Zsh startup hook | Same UID and DISPLAY only; ANSI, foreground, background and cursor update without signals or restarting. New Bash/Zsh st windows load the generated palette. Other shell startup systems need their own integration |
| Alacritty | Colour tables merged using the TOML editor, preserving comments and unrelated values | Alacritty's normal configuration watcher reloads when enabled; malformed configuration is preserved and reported |
| Kitty / Ghostty | Marked colour block preserving other configuration | Omarchy remote/reload helpers when enabled; otherwise next launch or the application's own reload |
| Foot | Selected colour keys preserving other INI sections | Omarchy OSC helper when enabled; otherwise next launch |
| X11 | Generated Xresources included from the user's file, merged with xrdb | Resources are read by compatible clients; unpatched st uses the separate OSC adapter |
| Qt | qt5ct/qt6ct palette and selected configuration keys | Requires the appropriate Qt platform theme plugin in the session; arbitrary existing Qt processes cannot have their startup environment changed |
| Omarchy | Generated user theme `~/.config/omarchy/themes/decebal/colors.toml` | Uses installed Omarchy templates in headless/background-free mode; optional browser, editor and user-hook targets remain explicit choices |

Native terminal adapters also work when Omarchy is absent and ensure an existing terminal configuration does not need an Omarchy include. The Omarchy bridge additionally covers its generated btop, Helix, Neovim and other application templates. Optional editor and hook targets are disabled by default; browser integration is explicit opt-in and retains the passwordless-helper gate. Native adapters do not write browser profiles.

Every managed file participates in the existing staged file transaction, original-byte backup manifest and rollback. Managed blocks are removed if the user edits other content after applying. Entire TOML and INI files are restored exactly when untouched; if subsequently edited, they are preserved and reported instead of discarding user edits. No package-owned `/usr/share/omarchy` file is changed. Live st reset on opt-out returns the terminal's compiled/default OSC colours; its previous arbitrary runtime OSC state cannot be recovered.

## Completion reporting

`system::completion(&Env)` exposes `pending`, `error` and `warnings`. A queued asynchronous apply is not labelled complete. Results are saved in the system state directory's `last-result.json`; errors clear the current debounce marker, and an older completed job cannot remove a newer pending request. Settings shows failures and warnings. GTK reopen and terminal reload boundaries are visible in the pane.

## Primary references

Read-only Omarchy reference commit `573b518be16ec47547d50fe6197bc7f53e982340` (MIT): `AGENTS.md`, `docs/theming.md`, `bin/omarchy-theme-set`, `bin/omarchy-theme-set-templates`, `bin/omarchy-theme-set-gnome`, `bin/omarchy-theme-set-foot`, `bin/omarchy-restart-terminal`, and `default/themed/*.tpl`. Its pipeline stages a palette, renders templates, atomically swaps the current theme, then runs scoped retint helpers and hooks. Decebal keeps its existing safe user-theme bridge rather than overwriting Omarchy defaults.

Modern libadwaita uses native CSS properties such as `--window-bg-color` and `--sidebar-bg-color`; old `@define-color` compatibility aliases alone do not recolour current widgets. See [libadwaita CSS variables](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/css-variables.html).

## Runtime verification

Run `DECTHEMECTL=/path/to/decthemectl python3 tests/systemtheming/apps-runtime.py`. The test creates its own HOME/config/state/runtime directories **before** starting its private D-Bus activation environment, Xvfb and rdwm. It launches actual Nautilus and the installed unpatched st, applies Nord, captures the running windows and checks exact palette pixels. It confirms existing st changes live, new st Bash sessions load the palette from their guarded startup hook, existing Files follows dark mode, and a separately launched Files process adopts the custom palette without closing the first instance. st replaces HOME from the passwd database internally, so the test explicitly restores the private HOME in its child command before launching Bash.

Verified artifacts: `/tmp/decebal-system-apps-final-qa/run-itxqenxx/terminal-after.png`, `terminal-next-launch.png`, `files-after.png`, `files-next-launch.png`, and `apply.json`. Nautilus 1.10 rendered 402,821 exact Nord background pixels in the new-process capture; the running st background changed to `#2e3440`. The machine has no Qt demo application installed, so Qt runtime rendering was not exercised; its existing palette/adapter unit checks remain the available validation.

`DECTHEMECTL=/path/to/decthemectl python3 tests/systemtheming/automatic-guard.py` checks the actual binary with a fake inherited bus and logging gsettings: disabled synchronous/asynchronous automatic requests produce no system files or helper calls, explicit dry-run still works, and removing the override retains default automatic behaviour. The fake helper prevents any real session access even if the guard regresses.
