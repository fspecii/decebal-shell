# Native widget-creator gate (source-only, not yet run)

**Status: the native run is UNRUN.** `tests/e2e/widget-creator-native.py` has been reviewed against the Settings source (`src/settings/widgetbuilder.rs`, `widgetspane.rs`, `model.rs`, `io.rs`) and checked with `py_compile` and the pure-Python self-test `tests/e2e/widget-creator-native-test.py` (argument validation, the config/TOML verifiers, token scan, pidfd shutdown, signal unwinding). Nothing in this repository claims it passes against a binary. The first native pass may need selector or timing adjustments; those are fixture bugs to report, not product results.

It replaces `tests/e2e/widget-creator-icons.sh` for the creator flow. That script kills the process named in the host lock file (a stale or recycled PID can be someone else's) and uses the old shared isolation; **do not run it**.

## Root commands

Root owns the one heavy job. The gate never builds, installs, pushes or touches a compositor. Run as the unprivileged user (the gate refuses uid 0: the failure injection relies on permission bits).

```sh
# DECEBAL_QA_BIN: absolute, immutable directory holding the candidate decsettings and decwidgets
# (not a symlink, not writable by group/others; the gate hashes it before and after).
python3 -I tests/e2e/widget-creator-native.py --bin-dir "$DECEBAL_QA_BIN" --out /var/tmp/dwc-<sha7>-<HHMMSS>
```

* `--out` must not exist (nothing is ever deleted or reused) and must be short: Settings' navigation socket lives under `<out>/r/run/` and a path beyond the unix-socket limit makes Settings fall back to `/tmp`, outside the private tree. The gate refuses an `--out` that is too long.
* `--extra-bin NAME` copies another binary of `--bin-dir` if the candidate uses a separate renderer. The current source has none, so normally it is not needed.
* Wrap it in `tools/dev/desktop_job.py --` like the other native fixtures if root's pause/slot policy applies.
* Required host tools: `Xvfb`, `xdotool`, ImageMagick `import`, `fc-list`, `fc-match`, Python with Pillow. No window manager, no python-xlib.
* Exit status: 0 pass, 1 failed check or cleanup problem, 2 invalid arguments (nothing started), 128+n interrupted by signal n. Evidence: `<out>/widget-creator-native.json` (steps, hashes, host reports, failure notice, cleanup ledger), `<out>/shots/*.png`, `<out>/logs/`.

## What it drives and proves

Settings is started with no pane; the Widgets sidebar entry is clicked, `Create widget…` opened, and every control is found from the real rectangles `DECSETTINGS_DEBUG_LAYOUT` writes after each paint. Input is xdotool/XTEST only (pointer, wheel, keys, typing).

1. **Form and preview.** Default draft (`clock`, `My clock`, Create enabled). The preview area above the form must hold painted pixels and must change when the clock options change and when the type changes to a note. An empty note disables Create and shows the validation text.
2. **Clock.** Name `Desk Clock`, 24-hour and date off. `desk-clock.toml` parses as TOML with the typed name, `{clock:%I:%M %p}`, no date node and no command keys; `decebal.json` gains exactly one card at the anchor/x/y the form displayed; `decwidgets status --json` becomes `ready` with no broken card or definition error.
3. **Preservation.** The seeded document has unknown top-level keys, other sections, unknown `widgets` keys, a card with unknown fields and options, and a disabled card. After every Create they must be equal as JSON; only `widgets.items` (appended) and `widgets.enabled` (now true) differ.
4. **Note and the async transaction regression.** Type `Static note` (type chosen through the popup by type-ahead), name `Field Note`, a note, size Large, nudged offset. Then the private config directory is made mode `0500` while the widgets directory stays writable, and Create is pressed. The worker writes `field-note.toml` and then fails the `decebal.json` commit. The gate **waits for the visible `Could not create widget…` notice containing `Permission denied` first** (that notice is the evidence the worker ran past the TOML write; nothing is asserted before it), waits another second, and requires: the widgets directory is exactly as before (the new TOML was removed), `decebal.json` is byte-identical, no temp/backup file appeared in the config directory, and the form is still open with the typed draft intact. A background watcher records any file that ever appeared in the widgets directory (`transient_files_seen`; informational because a fast worker can be missed). The mode is restored and Create pressed again: it must succeed under the **same** file name `field-note` (not `field-note-2`), proving the rollback freed it, and the card, TOML (size Large ⇒ a 338 px node) and host readiness are verified as above.
5. No fake action tool (`xdg-open gio peony notify-send loginctl systemctl`) may have been invoked.

## Isolation and cleanup

Owned Xvfb on a random high display (`private_x11.py`, ≥ 700; the filesystem and the abstract socket must both belong to this run's Xvfb child before **every** X command, and the display never comes from `:0`), no window manager, both D-Bus buses dead, no audio (`PULSE_SERVER`/`PIPEWIRE_RUNTIME_DIR` point nowhere, no `pactl`/`wpctl`), no rdwm, HOME/XDG/`DECEBAL_*` inside `<out>/r`, a `PATH` holding only the copied binaries, the fake action tools and read-only `fc-list`/`fc-match` links, host `DISPLAY`/`WAYLAND_DISPLAY`/`XAUTHORITY` never inherited. Every child (including the `decwidgets` host that Settings spawns from its own directory) carries a random run token.

Shutdown goes through pidfds: Settings, then every token process (the host is identified by token **and** by `/proc/<pid>/exe` being the private copy; the lock-file PID is never used), then Xvfb. SIGTERM and SIGINT are turned into an exception that unwinds through the same cleanup. **Any SIGKILL, leftover token process, unexpected token process, or change of the source binaries fails the run.** Only the private config directory's mode is toggled, and it is restored on every path.

## Limits

* One X11 session at 1280x800, scale 1, no compositor, no window manager. Focus relies on `xdotool windowfocus` and pointer position. If Settings does not map without a window manager that is a result to report; the gate does not add one.
* The failure is a permission failure on the private config directory, not a disk-full or crash fault; it needs uid ≠ 0. It exercises the production rollback path (`create_transaction`), not a race.
* The preview check compares overlapping pixels in a band derived from the enabled toggle and Type control at the pinned 1x scale, so it proves the preview reacts to the form, not that the rendering is correct. Host readiness is `decwidgets status --json`, not a pixel check of the card on the desktop.
* Wayland, multi-monitor placement, other templates (analog, metrics, quick note, timer) and keyboard-only operation are not covered. Source unit tests in `widgetbuilder.rs` cover the templates and the synchronous/asynchronous rollback.
* The notice text (`Could not create widget`) and the rectangle ids (`wg:new:*`) are taken from the current source; a rename needs a matching change here.

## Review correction checkpoint

The source review found that the original crop covered page headings rather than
the widget preview and that Xvfb inherited the token used by host cleanup. The
fixture now derives a fresh preview band for every shot, compares the common
height when the template changes, excludes the owned server from host scanning,
and continues cleanup after individual verification errors. All 16 helper tests
pass, including different-height preview and cleanup-error regressions.

The first corrected native attempt against optimized `9d663b4` was interrupted by
the global I/O guard before widget creation. It is **not native acceptance**.
The supervisor's group interruption left its separately grouped Xvfb alive; root
verified its exact start time, executable and private configuration environment
and stopped that owned orphan using a pidfd. Native fixture completion and its
preview band still need validation during a quiet test window.
