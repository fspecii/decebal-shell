# App colour propagation

Theme changes apply to supported apps automatically. An explicit `system.enabled = false` in `decebal.json` opts out; `decthemectl system off` restores saved configuration. Targets can be disabled independently in Appearance → All apps or with `decthemectl system target NAME off`. Package install, privilege prompts and restarting/terminating applications are not part of the adapter pipeline.

For an isolated process or non-app desktop test, `DECEBAL_SYSTEM_AUTOMATIC=0` suppresses automatic propagation before configuration reads, state writes or child processes. It does not change the saved preference or normal default. Explicit `apply-system` and `system on` remain intentional app-integration commands. Desktop E2Es set this override in their headers/common environment. App-theming E2Es enable automatic propagation only after private HOME/XDG paths and private D-Bus activation are established; a private HOME alone does not isolate a session bus inherited from the live desktop.

## Paths and actual behaviour

| Target | Configuration | Running applications |
| --- | --- | --- |
| GTK 3 | Generated theme `$XDG_DATA_HOME/themes/Decebal[-Alt]/gtk-3.0/gtk.css` (imports GTK's own Adwaita, then recolours controls and states), selected by `settings.ini` and gsettings `gtk-theme`; managed `gtk-3.0/gtk.css` contains no palette definitions while the generated theme is selected | New processes get everything. Running apps reload the theme when `gtk-theme` changes and the session delivers that change (Wayland settings portal / gsettings, or an XSETTINGS daemon on X11) |
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
* GDK shows `:backdrop` only under a window manager advertising `_NET_WM_STATE_FOCUSED`. rdwm's X11 backend gained it in rdwm
  ba28454 / 0b2cc71, which are proved by rdwm's own tests. Older rdwm builds never make GTK windows look inactive. Wayland
  compositors signal activation
  through xdg-shell.

## Live GTK settings on X11 (XSETTINGS manager)

GTK on X11 reads `settings.ini` only at startup. Afterwards, running GTK 3 / GTK 4 apps change their theme, icon theme, cursor,
font or button layout only through an XSETTINGS manager (freedesktop XSETTINGS 0.5; GDK `gdk/x11/gdksettings.c`). This machine
has none installed (no xsettingsd, xfsettingsd or gsd-xsettings).

After a successful GTK publication, the GTK target makes sure Decebal's own small manager runs, unless any of these holds:

* there is no `DISPLAY`;
* the session is Wayland;
* this is a unit test;
* `DECEBAL_XSETTINGS=0` is set;
* the option "Live GTK settings on X11" (`gtk.xsettings`) is off.

The manager is `decthemectl system xsettings serve --display D --ini PATH`, so there is no new binary or packaging change.

* **Values:** read from the generated `gtk-3.0/settings.ini`.

  | settings.ini key | XSETTINGS name |
  | --- | --- |
  | `gtk-theme-name` | `Net/ThemeName` |
  | `gtk-icon-theme-name` | `Net/IconThemeName` |
  | `gtk-cursor-theme-name` | `Gtk/CursorThemeName` |
  | `gtk-cursor-theme-size` | `Gtk/CursorThemeSize` |
  | `gtk-font-name` | `Gtk/FontName` |
  | `gtk-decoration-layout` | `Gtk/DecorationLayout` |

  GDK has no XSETTINGS name for the dark preference. Dark or light reaches plain GTK through the theme name, because the generated
  `Decebal` / `Decebal-Alt` slot itself imports the dark or light sheet, and the slots alternate on every colour change.
* **Updates:** inotify on the `gtk-3.0` directory, so the transaction's atomic rename is seen. The serial advances once per
  change, and only changed settings carry the new last-change serial.
* **Ownership:** the `_XSETTINGS_S<screen>` selection is the singleton.
  - An existing owner is never replaced, whether another daemon or Decebal's own.
  - The owner check and the claim happen together under a brief server grab: GetSelectionOwner, SetSelectionOwner (server
    timestamp taken beforehand), then GetSelectionOwner again. A manager that starts at the same moment can't be stolen in the
    gap. The grab covers no file I/O or waiting and is released on every path, errors included.
  - A successful claim is announced with `MANAGER`. On SelectionClear the manager ends quietly.
* **Lifetime:** the start is observed. The new process must be seen owning the selection, with its marker
  `_DECEBAL_XSETTINGS_OWNER` = "pid start-time", within 2 s; otherwise the apply reports a warning.
  - A serving child is handed to a reaper thread.
  - A child that lost to a concurrent start, or that timed out or failed, gets 2 s to end by itself and is then killed. That is the
    only signal ever sent, and only to this exact, unreaped child. The inotify descriptor is an `OwnedFd`, closed on every exit.
  - Turning "Live GTK settings on X11" off stops Decebal's manager at the next GTK apply; another manager is left alone.
  - `decthemectl system xsettings status|stop`, and turning GTK integration off, send `_DECEBAL_XSETTINGS_QUIT` with that pid and
    start time to the owner window.
  - The manager checks the pid and start time, releases the selection and exits. No process is signalled.
* **GTK 3 user block:**
  - GTK 3 resolves `@name` colour references across style providers by priority, and the user `gtk-3.0/gtk.css` (user priority,
    loaded once per process) outranks the theme.
  - In the private native run (f549151), GTK 4 changed live 2/2 while GTK 3 stayed on the old palette 2/2. The likely cause:
    the old block's `@define-color` names kept a theme reloaded through XSETTINGS on the old values. This is a hypothesis, not
    yet proved natively.
  - So while the generated theme is selected, the GTK 3 managed block defines no colours (a comment only); the generated theme
    carries `theme_*` / `decebal_*` itself.
  - With "Theme GTK 3 and GTK 4 controls" off, the user keeps their own GTK theme and the block carries the named colours as
    before. User CSS outside the block, the Files profile (GTK 4 block) and libadwaita names are unchanged.
  - The probe now logs `theme-initial`, `theme-notify` and `theme-settled`. Each entry records the received `gtk-theme-name` and
    the colours the cascade resolves (and, on GTK 3, the window background-color), so a run can tell "name received" from
    "style changed".
* **Limits:**
  - libadwaita apps (Files, ...) ignore GTK themes and read the user `gtk.css` palette only at startup, so a new palette still
    needs reopening there. Dark/light follows `color-scheme`.
  - Wayland GTK apps use the settings portal, not this manager.

**Icon-only publications run the same step.** The icon / cursor publication (`publish_assets`, Assets intent) runs the same
post-commit decision as a full GTK apply (`gtk::after_gtk_publication`), after its icon / cursor writes and before
`last-result.json`. A first icon-only selection therefore no longer leaves running GTK apps without a manager until a later
theme apply.

| Condition | Decision |
| --- | --- |
| GTK actually published, X11 display, not Wayland, no `DECEBAL_XSETTINGS=0`, `gtk.xsettings` on | Ensure Decebal's manager |
| Same, but `gtk.xsettings` off | Stop Decebal's own manager |
| GTK not published, or no display, Wayland, or suppressed | Skip: a running manager is left as it is |

* A foreign manager is never touched.
* The asset writes stay icon / cursor keys only.
* The manager publishes the `settings.ini` values GTK already reads at startup, so apps see no theme, colour or font change from
  it.
* The decision is recorded in the report (`Report::xsettings`).

**Producers.** Manual icon choices are real producers of that publication, not only the asset follow job.

* `decthemectl icons set NAME|auto` and `decthemectl icons reset` call `icons::ctl::publish_selection` once the choice, and its
  ownership record for `set`, is on disk. The command prints the outcome as a second line: `apps: GTK / Qt icon publication
  queued` or `apps: not published (reason)`.
* The Icons pane's "Use" and "Reset" do the same as a quiet `icons-publish` task on the settings persistence worker. That task
  is queued from the after-save hook, so it runs ordered after the save and never on the UI thread.
* Size, tint and fallback are shell-only and do not publish.
* `auto` (and a reset) names no theme, so it publishes no icon key. GTK keeps what it has, matching the shell's "follow the
  desktop" meaning.

Native scenario for this path (UNRUN; real CLI producer, no injected ticket):

```sh
# Private session: owned Xvfb on :7xx (lock pid + both sockets verified), private HOME/XDG_* and bus, keyfile GSettings,
# DECEBAL_SYSTEM_AUTOMATIC and DECEBAL_XSETTINGS unset (either =0 suppresses), no XSETTINGS owner on that display,
# no earlier full apply in this HOME.
# 1. Two fixture icon themes with one distinctive icon each (solid colour squares are enough):
for t in QA-Red:ff0000 QA-Blue:0000ff; do n=${t%%:*}; c=${t##*:}
  mkdir -p "$XDG_DATA_HOME/icons/$n/scalable/places"
  printf '[Icon Theme]\nName=%s\nDirectories=scalable/places\n[scalable/places]\nSize=48\nType=Scalable\nMinSize=8\nMaxSize=512\n' "$n" > "$XDG_DATA_HOME/icons/$n/index.theme"
  printf '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48"><rect width="48" height="48" fill="#%s"/></svg>' "$c" > "$XDG_DATA_HOME/icons/$n/scalable/places/folder.svg"
done
# 2. Open Files on a folder with a subfolder (folder icons visible), and a GTK 3 window showing "folder".
# 3. The real producer:
decthemectl icons set QA-Red          # line 2: "apps: GTK / Qt icon publication queued"
# 4. Wait until the queue is idle: no state/decebal/system/requests-*.json; last-result.json intent "assets", no error.
# Expect:
#   gtk-3.0/settings.ini and gtk-4.0/settings.ini gain gtk-icon-theme-name=QA-Red and nothing else (no theme, colour or font key);
#   gsettings get org.gnome.desktop.interface icon-theme  -> 'QA-Red'
#   decthemectl system xsettings status  -> Decebal's manager, pid shown
#   the open GTK 3 window's folder icon turns red in place (XSETTINGS Net/IconThemeName);
#   Files (libadwaita, GTK 4) follows when its process reads the setting.
decthemectl icons set QA-Blue         # the same windows turn blue without reopening
decthemectl icons reset               # "apps: ..." line; no icon key changes (auto names nothing)
# 5. Opt-outs: `decthemectl system target gtk off` (and qt off or absent), then `icons set QA-Red` prints
#    "apps: not published (the GTK and Qt app targets are switched off)"; with "Live GTK settings on X11" off
#    (system set: option gtk.xsettings), a publication stops Decebal's manager and leaves a foreign one alone.
```

The same steps through the Settings Icons pane (pick, Use, then Reset) must give the same files, gsettings value and
manager state.

**Acceptance commands (individual evidence below; ownership runtime checks remain outstanding):**

```sh
# 1. unit: protocol layout, serials, ownership classification, grab-wrapped claim order and ungrab on every error
cargo test --lib theme::system::xsettings
# 2. private X11 live acceptance (owned Xvfb, private HOME/XDG; no other manager on that display): with the build's decthemectl
#    and DECEBAL_XSETTINGS unset, run the native harness strict for plain GTK:
DECTHEMECTL=target/debug/decthemectl python3 tests/systemtheming/gtk-native.py OUT --themes nord,macos --modes dark --kits gtk3,gtk4,adw --strict-live gtk3,gtk4
#    expected: gtk3/gtk4 live, adw stale (palette) - recorded, not counted as live. The GTK 3 probe's events file must show
#    theme-settled with the new gtk-theme-name AND theme_bg_color / window background-color of the new palette.
# 3. ownership: start a foreign owner of _XSETTINGS_S0 first (any test client holding the selection) -> apply reports "another
#    XSETTINGS manager", the foreign owner keeps it; then `decthemectl system off` -> Decebal's manager (if any) releases, the
#    foreign one is untouched; `system xsettings status` before/after.
# 4. race: two real contenders (two `decthemectl system xsettings serve` on one private display, and a foreign client
#    claiming _XSETTINGS_S0 while ours starts): exactly one owner, the loser exits, the foreign owner is never replaced,
#    and no child process is left behind.
```

Native gate 2 needs one harness change before it can pass: its private `apply()` passes `DECEBAL_SYSTEM_AUTOMATIC=0`, which does
not suppress the manager. It must also run without a pre-existing XSETTINGS owner on its own Xvfb. The current root harness is
not edited here.

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

`DECTHEMECTL=/path/to/decthemectl python3 tests/systemtheming/gtk-native.py OUT_DIR [--themes all] [--modes dark,light] [--scales 1,2] [--states] [--live | --strict-live gtk3,adw]`
applies through the real binary and launches the probe apps (`tests/systemtheming/gtk_probe.py`, same controls in GTK 3, plain
GTK 4 and libadwaita) and Nautilus.

**Privacy and cleanup.**

* HOME/XDG live under `OUT_DIR`; GSettings uses a keyfile backend; the private bus has no activatable services.
* Xvfb runs on an owned display of 700 or above. Its lock pid and both sockets are verified before every input burst.
* Every child leads its own process group and carries a private run token. Cleanup signals only token-carrying members of those
  groups, proved from `/proc`, never matched by name.
* Bus, Xvfb and helper startup run inside the same `try/finally` as the cases. Helper reads and startup waits are bounded.

**Backdrop is synthetic.** With no window manager, `ewmh_focus.py` advertises and toggles `_NET_WM_STATE_FOCUSED` itself. That
proves the themes' `:backdrop` styles, not rdwm.

**Checks** are on captured pixels against `decthemectl system palette --json`:

* role surfaces;
* text contrast measured from glyph pixels;
* hover, disabled, focus ring and backdrop;
* real clicks and toggles arriving in the app.

`--live` starts the apps on theme A, applies theme B while they keep running, and classifies each app against B's window colour.

| Outcome | Meaning |
| --- | --- |
| `live` | Changed to B in place |
| `stale` | Still showing A |
| `other` | Neither A nor B |
| `indistinguishable` | A and B share a window colour, so nothing can be concluded |
| `invalid` | Not the same process and window, or not on A before the change |

* Only `live` counts as a live change. `--strict-live KITS` fails unless those kits reach it.
* A reopened process is checked separately. That proves new processes get B, never that running ones changed.
* This fixture states its expectation up front, and `results.json` records it next to each outcome:
  - GTK 3 / GTK 4: live changes require an XSETTINGS provider; the product may start its own after apply (see the apply report).
  - libadwaita and Files: dark/light at most, because user CSS is read only at startup.
* Proving live GTK 3 / GTK 4 reloads needs a session that delivers `gtk-theme` changes: the Wayland settings portal or an
  XSETTINGS daemon.

`python3 tests/systemtheming/gtk-native.py --self-test` runs the harness's own failure paths with no X server, GTK or decthemectl:

* bounded helper reads;
* group + token cleanup of descendants;
* a process without the token left alone;
* cleanup after a startup failure.

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


## Root native acceptance checkpoint — 2026-10-31

The integrated production source at `b674ba3` built as a private dev-profile CLI
(debug info disabled), SHA-256 `aa6f830eeff6357d0a001e1267251f8194d532401b139a248c2916b6751d4906`.
The real GTK fixture passed 316/316 checks for Nord and Windows 11, dark/light,
GTK3, GTK4 and libadwaita at 100%. It checked role colours, text contrast, hover,
disabled controls, keyboard focus, real clicks and synthetic EWMH backdrop state.
The root reviewed the Windows 11 dark GTK4 capture visually. No leftover processes
were reported. The controller verified stable source, no pressure abort and a
restored shared build pause.

Evidence: `/var/tmp/decebal-gtk-native-acceptance-control/result.json` and
`/var/tmp/decebal-gtk-native-acceptance-pixels/results.json`, with per-case PNGs.
The X display was an owned Xvfb; the backdrop hint came from the synthetic fixture,
not rdwm. This is not optimized-artifact, actual-rdwm, Qt, Files, high-DPI,
all-theme or already-running-application acceptance. Those remain separate gates.
The corrected harness self-test also passed, including partial helper replies and
PIDFD-targeted cleanup. No live desktop configuration or installed binary changed.


### High-DPI rendering and live-update characterization

The corrected 200% fixture completed 348 checks for Nord and Windows 11 in both
modes, including GTK3, GTK4, libadwaita and Files. New/reopened apps used the
selected palette. Both the fixture and outer token audit reported no leftover
processes. Evidence: `/var/tmp/decebal-gtk-hidpi-live-control-v2/result.json` and
`/var/tmp/decebal-gtk-hidpi-live-pixels-v2/results.json`.

**Live palette updates did not pass:** all eight running-app comparisons remained
stale (four toolkits, theme change and dark/light change). Their identities stayed
valid. That historical fixture had no XSETTINGS service; implementing the native X11
notification bridge is the next product task. Libadwaita user-CSS reload remains
an independent limitation. The 348 successful fixture checks must not be cited as
eight successful live updates.

The first run is retained as a failed/interrupted fixture: its Files header oracle
omitted the configured titlebar role, covered GTK3 windows produced black Xvfb
captures, and reopening reused stale JSON window ids. The corrected harness uses
scale-aware header sampling with the configured roles, raises the owned window
before capture, bounds capture waits, and gives every probe fresh metadata/logs.
The interrupted run's remaining private bus and X server were stopped through
run-token-checked pidfds; the corrected run needed no outer cleanup rescue.


### Native Qt6 palette and icon checkpoint

`tests/systemtheming/qt-native.py` and `qt_probe.cpp` exercise a real Qt6 widget
application through qt6ct in a private offscreen session. On Qt 6.12.0, all 40
checks passed for Nord/Windows 11, dark/light: active window/base/button/text,
highlight and selected text, disabled text, selected fixture icon theme and icon
resolution, loaded platform-plugin mapping, and painted window background.
The Windows 11 dark capture was also inspected visually. The initial check used
Qt debug logs to prove plugin loading; that build emitted no logs, so the corrected
probe checks its own memory mappings instead. No palette check was relaxed.

The host has no qt6ct installed. A private unpacked Arch qt6ct 0.11-9 package was
verified with the installed Arch keyring (Antonio Rojas signature). Archive SHA-256:
`28432106949664ba59a0aa6614356a28bf60b6c3d5ec0476a97773c1a0d9920c`.
Only the private fixture uses its libraries; nothing was installed system-wide.
Evidence: `/var/tmp/decebal-qt-native-control-v2/result.json` and
`/var/tmp/decebal-qt-native-pixels-v2/result.json`. The Qt widget probe is compiled
with the installed Qt6Widgets development files, under the shared low-priority
job runner. Set `QT_PROBE`, `QT6CT_ROOT` (an unpacked package root), and
`DECTHEMECTL`, then run `python3 tests/systemtheming/qt-native.py FRESH_OUT_DIR`.

This covers newly opened Qt widgets through qt6ct, not GTK's platform plugin,
KDE/KColorScheme applications, DTK, live updates, physical displays or the release
artifact. The runtime still requires an available and selected Qt platform theme.

### X11 live palette correction: candidate acceptance

Candidate `05d9b14` passed 93 focused system/appearance tests, formatting, strict all-target Clippy and a dev CLI build (`/var/tmp/decebal-xsettings-gtk3-correction-gate/result.json`). Its immutable dev artifact then passed 228/228 private-Xvfb checks. Both GTK3 and GTK4 changed in the same open process/window for Nord dark → Windows11 dark and Nord dark → Nord light: four live, zero stale/invalid. Reopened processes were checked separately. No fixture rescue/leftovers or I/O-pressure abort occurred. Evidence: `/var/tmp/decebal-xsettings-gtk3-corrected-live-control/result.json` and `/var/tmp/decebal-xsettings-gtk3-corrected-live-pixels/results.json`. The GTK3 after-live light capture was visually reviewed. This run used the root pixel harness before the additional theme-notify probe logging was integrated; it proves rendered colour changes, not those extra event records.

This is candidate dev evidence, not final main/release or hardware acceptance. Main additionally installs the settings-file watch before the initial read, closing a startup replacement gap. Combined-source validation and native ownership checks remain required. libadwaita/Files live custom palettes, Wayland and physical display acceptance remain open.

The integrated Rust source (`384b74f`, including watch-before-snapshot ordering) passed 93 focused tests, formatting, strict all-target Clippy and a dev CLI build: `/var/tmp/decebal-xsettings-main-integration-gate/result.json`. The immutable artifact's private-X11 ownership gate passed all 11 recorded checks: `/var/tmp/decebal-xsettings-main-ownership/result.json`. Reproduce with `python3 tests/systemtheming/xsettings-native.py /absolute/path/to/decthemectl FRESH_OUTPUT_DIR`; it uses an owned high-display Xvfb and fixture-owned selection windows only. The checks cover foreign-owner preservation, concurrent direct manager starts, initial/atomic-replacement settings publication, SelectionClear exit, and typed Stop. Automatic competing full applies and hardware are separate acceptance.

Final integrated dev artifact (`384b74f` Rust source, native harness `e5d7dea`, SHA256 `ac063395b4b2d131c947a12e61d257ec5e68299ddadfa32a85f9382beaa726c0`) passed the combined ownership + live run: 11 ownership checks and 228/228 toolkit checks, four live changes, zero stale/invalid, no leftover/rescued processes and no I/O abort. GTK3 event logs show `theme-notify` and `theme-settled` with the new slot and matching background roles for both switches. Evidence: `/var/tmp/decebal-xsettings-main-native-control/result.json` and `/var/tmp/decebal-xsettings-main-native-pixels/results.json`. No installed binary or live desktop config changed. Optimized artifact and physical-session acceptance remain outstanding.

### Icons-only post-publication hook: source and regression gate

Main candidate `539c57b` (specialist `a2576fb`) passed 95 focused tests, formatting, strict all-target Clippy and a dev CLI build: `/var/tmp/decebal-icons-xsettings-hook-source-gate/result.json`. The immutable dev CLI passed 228 native GTK regression checks, four live recolouring cases and 11 XSETTINGS ownership checks, with zero stale/invalid windows, leftovers or fixture rescue: `/var/tmp/decebal-icons-hook-native-regression-control/result.json`. This proves the refactored full GTK path retains its accepted behaviour.

The real first-manual-choice workflow remains unproven: inspection found that `icons set` and the Icons pane do not yet enqueue the narrow publication. The specialist is adding those post-commit producers in a separate source-only increment. No injected queue ticket is accepted as sole proof of that user workflow. Neither this hook nor the pending producer changes are installed; the running desktop remains on the qualified a82/b261 combination.


### First manual icon choice: CLI and Settings native acceptance

Candidate `29fd95c` wires both CLI and Settings selection producers after the
persisted selection/ownership commit. It passed 91 focused tests; the first
combined gate was interrupted by I/O pressure during lint, so that gate is not
reported as a pass. A separate serialized gate passed formatting, strict
all-target Clippy and dev builds of Settings and the CLI.

`tests/systemtheming/icons-native.py` then passed 32 native checks against the
immutable candidate binaries. Two real GTK3/GTK4 windows started with independent
red icon preferences, before any Decebal theme apply or XSETTINGS owner. The real
CLI selected Blue; real Settings Icons-pane row and Use clicks selected Red then
Blue. Both existing processes/windows repainted their icons, with unchanged GTK
theme/font, byte-preserved CSS and completed assets-only publication. Tests also
cover the XSETTINGS opt-out, foreign owner preservation and master/GTK-target
opt-outs. No leftover processes, cleanup rescue or resource abort occurred.
The GTK4 Red icon capture was visually inspected. Evidence:
`/var/tmp/decebal-manual-icons-native-gate-v6/result.json` and
`/var/tmp/decebal-manual-icons-native-evidence-v6/result.json`.

The same fixture against sealed release `b261c13` passed initial red rendering
but failed at first manual manager startup, as expected for the missing producer:
`/var/tmp/decebal-manual-icons-native-negative-evidence-v2/result.json`.
Earlier fixture attempts are preserved: they failed before publication because
isolated MIME/Glycin descriptors were missing, or because window identification
required an unpublished PID property. Active GTK image-loader helpers are checked
for leaks after their owning probes close, rather than while images remain open.

Reproduce with system Python (GI GTK3/GTK4, python-xlib, Pillow, Xvfb and XTEST):
`python3 tests/systemtheming/icons-native.py --bin-dir ABSOLUTE_BIN_DIR --out FRESH_OUTPUT_DIR`.
The fixture uses an owned high-display Xvfb and private HOME/XDG paths, including
private copies of system MIME and Glycin loader descriptors. Current Settings
omits `_NET_WM_PID`; identification requires the one matching class on this owned
server and a live fixture-owned Settings process, checking PID when available.

This is dev-candidate X11 evidence. Optimized-build and physical-session acceptance,
Qt live updates, Wayland, and reset restoring a prior external icon selection are
not proven by this test. The installed desktop remains on a82/b261.


### Optimized manual-icon acceptance and installation

Release source `858c69bc6cb8ab38fcdd55feeea8549e1b004681` built the Settings/CLI
pair in 318 seconds under the serialized low-priority job runner. The sealed
manifest (`33eff13b382b35ad9709be389a86c81acdc9f51794e0ebbb2d5ccdcd6e966d8f`)
passed the publisher's provenance and integrity verifier. Those exact binaries
passed 32 manual-icon checks (CLI and real Settings controls), preview cache
corruption/recovery plus browsing/reopen checks, 228 GTK checks with four live
colour changes, and 11 XSETTINGS ownership checks. No pressure abort, fixture
rescue or leftover processes occurred.

Evidence: `/var/tmp/decebal-manual-icons-optimized-native-gate/result.json`,
`/var/tmp/decebal-manual-icons-preview-release-native-control/result.json`, and
`/var/tmp/decebal-manual-icons-release-gtk-regression-control/result.json`.

The managed publisher installed both pair members in both managed runtime
folders; all four file hashes and unchanged watched configuration were verified.
Journal: `/home/ambsd/.local/state/decebal/deployments/manual-icons-858c69b-20261031-200200`.
Open Settings processes/drafts and desktop daemons were preserved; newly opened
Settings uses this release. The rest of the shell remains the previously
qualified a82 runtime. This is isolated X11 release acceptance, not physical
hardware, Wayland, Qt live-update or full desktop-replica proof.
