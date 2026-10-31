# App colour propagation

Theme changes apply to supported apps automatically. An explicit `system.enabled = false` in `decebal.json` opts out; `decthemectl system off` restores saved configuration. Targets can be disabled independently in Appearance → All apps or with `decthemectl system target NAME off`. Package install, privilege prompts and restarting/terminating applications are not part of the adapter pipeline.

For an isolated process or non-app desktop test, `DECEBAL_SYSTEM_AUTOMATIC=0` suppresses automatic propagation before configuration reads, state writes or child processes. It does not change the saved preference or normal default. Explicit `apply-system` and `system on` remain intentional app-integration commands. Desktop E2Es set this override in their headers/common environment. App-theming E2Es enable automatic propagation only after private HOME/XDG paths and private D-Bus activation are established; a private HOME alone does not isolate a session bus inherited from the live desktop.

## Paths and actual behaviour

| Target | Configuration | Running applications |
| --- | --- | --- |
| GTK 3 | Generated theme `$XDG_DATA_HOME/themes/Decebal[-Alt]/gtk-3.0/gtk.css` (imports GTK's own Adwaita, then recolours controls and states), selected by `settings.ini` and gsettings `gtk-theme`; named colours in a managed block of `gtk-3.0/gtk.css` | New processes get everything. Running apps reload the theme when `gtk-theme` changes and the session delivers that change (Wayland settings portal / gsettings, or an XSETTINGS daemon on X11) |
| GTK 4 (plain) | Generated theme `.../gtk-4.0/gtk.css` (imports GTK's Default sheet), same selection | As GTK 3 |
| libadwaita / Files | Managed `gtk-4.0/gtk.css`: libadwaita's named colours, `:root` variables, `--dim-opacity`, backdrop fade, Files profile | Existing apps follow light/dark; a new process reads the full palette. Existing processes cannot be forced to reload user CSS through a supported external API |
| st / st-256color | Scoped OSC colour updates and a guarded managed Bash/Zsh startup hook | Same UID and DISPLAY only; ANSI, foreground, background and cursor update without signals or restarting. New Bash/Zsh st windows load the generated palette. Other shell startup systems need their own integration |
| Alacritty | Colour tables merged using the TOML editor, preserving comments and unrelated values | Alacritty's normal configuration watcher reloads when enabled; malformed configuration is preserved and reported |
| Kitty / Ghostty | Marked colour block preserving other configuration | Omarchy remote/reload helpers when enabled; otherwise next launch or the application's own reload |
| Foot | Selected colour keys preserving other INI sections | Omarchy OSC helper when enabled; otherwise next launch |
| X11 | Generated Xresources included from the user's file, merged with xrdb | Resources are read by compatible clients; unpatched st uses the separate OSC adapter |
| Qt | qt5ct/qt6ct palette and selected configuration keys | Requires the appropriate Qt platform theme plugin in the session; arbitrary existing Qt processes cannot have their startup environment changed |
| Omarchy | Generated user theme `~/.config/omarchy/themes/decebal/colors.toml` | Uses installed Omarchy templates in headless/background-free mode; optional browser, editor and user-hook targets remain explicit choices |

Native terminal adapters also work when Omarchy is absent and ensure an existing terminal configuration does not need an Omarchy include. The Omarchy bridge additionally covers its generated btop, Helix, Neovim and other application templates. Supported editor targets are enabled by default; browser integration is enabled when its passwordless helper is available. Explicit per-target opt-outs remain respected. Hooks remain opt-in. Browser and editor helpers run even when the same palette is already staged. Native adapters do not write browser profiles.

Every managed file participates in the existing staged file transaction, original-byte backup manifest and rollback. Managed blocks are removed if the user edits other content after applying. Entire TOML and INI files are restored exactly when untouched; if subsequently edited, they are preserved and reported instead of discarding user edits. No package-owned `/usr/share/omarchy` file is changed. Live st reset on opt-out returns the terminal's compiled/default OSC colours; its previous arbitrary runtime OSC state cannot be recovered.

## GTK toolkit model

What each toolkit actually reads (verified against GTK 3.24.52, GTK 4.24.1, libadwaita 1.10.0 and Nautilus 51.1 by extracting their
compiled stylesheets):

* libadwaita forces `gtk-theme-name = Adwaita-empty` and ignores GTK themes. Its `:root` variables are defined from its named colours
  (`--window-bg-color: @window_bg_color`), so the user `gtk-4.0/gtk.css` block (user priority) redefines both. Names keep libadwaita's
  meaning: `headerbar_border_color` is a foreground mixed at `--border-opacity`; `accent_color`, `destructive_color` and the other
  `*_color` names are standalone text colours; `sidebar_border_color`, `--active-toggle-*` and `--overview-*` are set as well.
  libadwaita's own design is kept: top bars in a toolbar view are flat over the window, and destructive buttons are tinted.
* GTK 3 Adwaita and the GTK 4 Default theme paint with literal colours, so named colours alone recolour almost nothing. The
  generated `Decebal` theme imports the toolkit's sheet for the mode and recolours colours only: sizes, paddings and radii stay
  native, so hit targets do not move. Every rule has a `:backdrop` twin, so focus loss keeps the theme's colours. Being a theme
  (not user CSS), it lets an application's own CSS win.
* Running GTK 3 / GTK 4 apps reload a theme only when its *name* changes, so new colours are written to the inactive slot
  (`Decebal` / `Decebal-Alt`) and that slot is selected. Reapplying the same colours changes nothing.

Readability is computed, not assumed. Every role is checked for all 44 shipped themes in both modes (unit test):

| Text | Minimum contrast |
| --- | --- |
| Body, view, control (at rest, hover and press), header, backdrop header, sidebar, dim, popover and status text | 4.5:1 |
| Text on accent and destructive fills (libadwaita's own accent with white is 3.8:1) | 3:1 |
| Check, radio and switch outlines; focus rings | 3:1 |
| Disabled text | Visibly weaker than normal, never under 1.4:1 |

libadwaita-specific adjustments:

* Dim text uses `--dim-opacity`, raised per theme only as far as 4.5:1 needs.
* The backdrop header fade is limited the same way, and its "do not fade twice" rule is restated.
* Destructive text stays readable on its own 15–30% tint.

Translucency: only menus, popovers and tooltips use the theme's `popup_alpha`, and their text is checked over black and white
behind. Windows, views and dialogs stay opaque. Alpha needs a compositor, and blur is the compositor's, not GTK's.

Options (Settings → Appearance → All apps, or `decthemectl system set`):

* `gtk-theme off` keeps the user's own GTK theme; libadwaita colours still apply.
* `gtk-font off` keeps the user's fonts. Otherwise the first installed family of the theme's interface chain becomes
  `gtk-font-name`/`font-name`, and of its monospace chain `monospace-font-name`.
* A configured icon or cursor theme that is not installed is reported, not published.

Boundaries:

* GTK does not theme Qt or DTK (Deepin) applications. Those go through qt5ct/qt6ct or a platform theme. Deepin's legacy
  `deepin-gtk-theme` colours are not used; GTK follows the theme package tokens.
* GDK shows `:backdrop` only under a window manager advertising `_NET_WM_STATE_FOCUSED`. rdwm's X11 backend does not list it
  (checked in its `_NET_SUPPORTED`), so GTK windows under rdwm on X11 never look inactive. Wayland compositors signal activation
  through xdg-shell.

## Icon and cursor publication (contract with the asset workflow)

`theme::system::publish_assets(shell_json) -> Publication` (`Requested` | `Off(reason)`) is the entry point for an accepted icon /
cursor selection change (`icons.theme`, `system.iconTheme`, `system.cursorTheme`). It never blocks.

**Exact writes (Assets intent)**

| Toolkit | What is written |
| --- | --- |
| GTK | `gtk-icon-theme-name` and `gtk-cursor-theme-name` in `gtk-3.0/settings.ini` and `gtk-4.0/settings.ini`; gsettings `org.gnome.desktop.interface icon-theme` / `cursor-theme` |
| Qt | `[Appearance] icon_theme` in `qt5ct.conf` / `qt6ct.conf` |

Only installed themes are published. Nothing else is touched: no CSS, colour, font, `gtk-theme-name`, button layout, Qt colour
scheme or style. Omarchy, browser, editors, hooks, terminals, Kvantum and X11 never run. Writes go through the same backup
transaction, so `off` restores the original keys.

**Opt-outs.** An explicit `system.enabled = false` (unset means on), or both `gtk` and `qt` off or unavailable, gives `Off(reason)`
and writes nothing. An explicit `system.iconTheme` wins over `icons.theme`. Per-slot user and custom ownership is decided by the
asset policy before calling.

**Request ownership protocol (shared by every producer).**

* **Queues:** one per configuration file, `requests-<hash of the shell_json path>.json` with `pending`, `running` and `retry`, plus
  the literal path. Each changes only by a read-modify-write under `requests-<hash>.lock`, never held during an apply.
  - `flock` failures are checked: `EINTR` is retried, and any other error is reported with nothing read or written.
  - A queue file naming another configuration is never adopted.
* **Producers:** `request()` asks for `Full` and `publish_assets()` for `Assets`. Neither does queue I/O on the caller's thread.
  - Both merge into an in-process outbox, one slot per configuration with the wider intent, so it stays bounded and coalescing
    never loses `Full`.
  - A single dispatcher thread drains the outbox. A short-lived process drains it at exit, waiting at most 3 s.
* **Enqueue:** the new ticket keeps the widest intent still owed: the pending ticket, a failed spawn (`retry`), and a running
  ticket whose worker is gone. A worker is identified by pid plus `/proc` start time, so a reused pid is not "alive".
* **Worker start:** the worker is started with `--shell-json PATH`, the literal producer path, never rediscovered.
* **Worker run:** after the debounce it takes the system apply lock and claims its ticket only if it is still that configuration's
  pending one. It reads configuration and palette only after claiming, and `finish` clears `running` only for its own ticket.
* **Failures and off:** a failed spawn moves its ticket's intent to `retry`. Settings then shows the error rather than endless
  pending work, and the next request inherits the intent. `off` drops pending and retry tickets.

Deterministic tests (UNRUN, build pause) cover:

* every producer order;
* root's race (a full request between an asset read and its write);
* claim and finish ownership;
* a failed spawn keeping an inherited `Full`;
* a dead or pid-reused worker handing back its intent;
* a refused lock never entering the read-modify-write;
* per-configuration isolation and the collision guard;
* eight producer threads through the real lock;
* outbox coalescing;
* opt-outs;
* key-exact assets output.

**Remaining native gates (not yet run for this protocol):**

1. A real `decthemectl` producer burst (two configurations included, to prove the `--shell-json` hand-off): `request` and `publish_assets` interleaved from two processes. Assert one worker runs the
   full apply and the others exit superseded.
2. An Assets-only run against real GTK 3 / GTK 4 / libadwaita / Qt clients. Icons change in new processes; a running GTK app
   follows the gsettings `icon-theme` change only through the settings portal or an XSETTINGS daemon.
3. Spawn failure (missing `decthemectl`) surfaces in `completion()` and leaves no stuck pending state.

## Completion reporting

`system::completion(&Env)` exposes `pending`, `error` and `warnings`. A queued asynchronous apply is not labelled complete. Results are saved in the system state directory's `last-result.json`; errors clear the current debounce marker, and an older completed job cannot remove a newer pending request. A debounced job also re-checks its request under the system lock: one superseded while waiting for the lock writes nothing (`superseded by a newer theme change`), so an older palette never lands after a newer one. Settings shows failures and warnings. GTK reopen and terminal reload boundaries are visible in the pane.

## Primary references

Read-only Omarchy reference commit `573b518be16ec47547d50fe6197bc7f53e982340` (MIT): `AGENTS.md`, `docs/theming.md`, `bin/omarchy-theme-set`, `bin/omarchy-theme-set-templates`, `bin/omarchy-theme-set-gnome`, `bin/omarchy-theme-set-foot`, `bin/omarchy-restart-terminal`, and `default/themed/*.tpl`. Its pipeline stages a palette, renders templates, atomically swaps the current theme, then runs scoped retint helpers and hooks. Decebal keeps its existing safe user-theme bridge rather than overwriting Omarchy defaults.

Modern libadwaita uses native CSS properties such as `--window-bg-color` and `--sidebar-bg-color`; old `@define-color` compatibility aliases alone do not recolour current widgets. See [libadwaita CSS variables](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/css-variables.html).

## Runtime verification

`DECTHEMECTL=/path/to/decthemectl python3 tests/systemtheming/gtk-native.py OUT_DIR [--themes all] [--modes dark,light] [--scales 1,2] [--states]`
applies through the real binary and launches the probe apps (`tests/systemtheming/gtk_probe.py`, same controls in GTK 3, plain
GTK 4 and libadwaita) and Nautilus. It runs privately:

* HOME/XDG under `OUT_DIR`, GSettings in a keyfile backend and a private bus with no activatable services;
* an owned Xvfb on a display of 700 or above, whose lock pid and both sockets are verified before every input burst;
* a minimal `_NET_WM_STATE_FOCUSED` helper (`ewmh_focus.py`), so backdrop is real GTK state.

Checks are on captured pixels against `decthemectl system palette --json`:

* role surfaces;
* text contrast measured from glyph pixels;
* hover, disabled, focus ring and backdrop;
* real clicks and toggles arriving in the app.


Run `DECTHEMECTL=/path/to/decthemectl python3 tests/systemtheming/apps-runtime.py`. The test creates its own HOME/config/state/runtime directories **before** starting its private D-Bus activation environment, Xvfb and rdwm. It launches actual Nautilus and the installed unpatched st, applies Nord, captures the running windows and checks exact palette pixels. It confirms existing st changes live, new st Bash sessions load the palette from their guarded startup hook, existing Files follows dark mode, and a separately launched Files process adopts the custom palette without closing the first instance. st replaces HOME from the passwd database internally, so the test explicitly restores the private HOME in its child command before launching Bash.

Verified artifacts: `/tmp/decebal-system-apps-final-qa/run-itxqenxx/terminal-after.png`, `terminal-next-launch.png`, `files-after.png`, `files-next-launch.png`, and `apply.json`. Nautilus 1.10 rendered 402,821 exact Nord background pixels in the new-process capture; the running st background changed to `#2e3440`. The machine has no Qt demo application installed, so Qt runtime rendering was not exercised; its existing palette/adapter unit checks remain the available validation.

`DECTHEMECTL=/path/to/decthemectl python3 tests/systemtheming/automatic-guard.py` checks the actual binary with a fake inherited bus and logging gsettings: disabled synchronous/asynchronous automatic requests produce no system files or helper calls, explicit dry-run still works, and removing the override retains default automatic behaviour. The fake helper prevents any real session access even if the guard regresses.

Legacy GTK styles linked from `~/.config/omarchy/desktop-styles` contain literal
widget colours that override a palette appended later. For those known legacy
links only, the GTK adapter replaces the linked stylesheet through the normal
backup/undo transaction. Ordinary user CSS elsewhere retains managed-block
semantics. Explicit system icon overrides win; otherwise application GTK icons
follow the theme's `icons.theme` setting. Already-open GTK apps need reopening
to load a full custom palette; no automatic process termination is performed.
