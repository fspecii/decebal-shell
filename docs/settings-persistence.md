# Settings persistence: durable writes off the UI thread

## Problem

Under disk load the AT-SPI native worker caught the Settings main thread inside `fsync` for more than three seconds.
The files were applied, but the semantic actions timed out: `Model::edit`, `Model::edit_json` and every theme package
transaction validated (`rdwmctl check-config`), wrote, fsynced and renamed on the UI thread. A root private serial suite
run also hung in `themepane::every_theme…` with a thread in `btrfs_btree_wait_writeback_range` and twenty leaked
`decsettings-watch` threads (no application crash was proven).

## Design (`src/settings/persist.rs`)

* **One owner.** `Persister` starts one thread, `decset-persist`, that runs every Settings job strictly in submission
  order and reports each result as `io::Ev::Persisted` on the window's existing event queue, then wakes the window.
  Submitting takes a mutex and pushes onto a bounded queue (256 jobs); it never waits for the disk.
* **Immutable deltas, not snapshots**, so a later job never undoes an earlier one that has not committed yet:
  * `Job::Toml`: a `TomlPatch` of the keys one edit changed, computed on the UI from the shown document
    (`toml_edit` diff; tables merged key by key, arrays of tables replaced whole). The worker applies it to the file as it
    is when the job runs, validates, writes atomically. A patch reproduces the edit byte for byte when nothing else
    changed (tested over every setter kind) and keeps keys another job (a theme) changed meanwhile.
  * `Job::Json`: set/remove one top-level key; every other key and the key order are kept (`io::json_set`).
  * `Job::Theme`: a package transaction **planned and executed in the worker** (`plan_selection` / `plan` /
    `plan_*_with_widgets`, then `execute`), so the plan sees the files as earlier jobs left them. The engine's undo record
    and rollback keep the multi-file change atomic. A shell style's dock/bar keys run in the same job, computed from the
    files the transaction wrote.
* **No clobbering.** The worker tracks the stamp (mtime, size, inode) it last wrote or the window last read
  (`Store::epoch`, bumped on every (re)read). A file changed by someone else fails the job with "changed on disk"; jobs
  computed before the window reloaded fail too; nothing is written.
* **Coalescing only where equivalent.** A queued JSON set of the same file and key, with no other job for that file in
  between, is replaced in place by the newer value (k=a then k=b leaves the file exactly as k=b). TOML patches and
  theme transactions are never merged: their validation and planning depend on what ran before them.
* **Pressure.** A full queue refuses the change visibly ("earlier changes are still being written to disk"); the change
  is not shown as made. Nothing accepted is ever dropped.
* **Stop/drain.** Dropping the `Persister` (the window is gone: the X11/Wayland loop returned and dropped its
  connection first) stops accepting jobs, lets the worker finish everything queued and joins it, bounded by
  `DRAIN_LIMIT` (30 s); a disk that never answers is logged and the process exits instead of hanging.
* **No skipped fsync.** `io::write_atomic` is unchanged: temp file, fsync, rename, one-time backup.

## Window side (`Model`)

* `Model::pending` holds accepted, uncommitted changes by ticket. The shown documents are the saved files plus the
  pending changes in order (`rebuild_cfg`, `json_shown`); a failure removes only its own change, newer accepted edits
  stay and are saved.
* The toast says "Saving…" (AT-SPI: `busy`) while anything is pending and does not expire; "Saved and applied" /
  "Saved (rdwm is not running)" / "Theme: …" only after the commit. rdwm reloads after the commit. An error notice stays
  visible when later jobs succeed.
* AT-SPI semantics: an action's reply means *accepted* (validated and queued, shown pending), never *durable*. A
  synchronous refusal (invalid value, full queue) is still the action's error; a later write failure appears as an
  alert on the page and the control shows the saved value again.
* Theme controls build on the newest accepted selection (`Model::theme_selection`), so a mode/palette picked while a
  theme is still saving is not lost.
* File-change events that arrive while our own writes land are checked again once they are done.
* Unit tests drive the model inline (`Persister::inline()`, same jobs and result handling, no thread); worker-thread
  behaviour is tested with a gated fsync (`persist::tests`, `persist::model_tests`, `app::a11y` test).

Also fixed on the way: `Cfg::array_move` (rules/binds "Move up/down") reordered tables in memory only — `toml_edit`
renders parsed tables by their old file position, so the file never changed while the old window showed the new order.
The io watcher thread now ends when its window's queue is gone.

## Fault injection (test only)

`--features settings-io-fault` (off by default, never for packages) makes `io::write_atomic` wait before each fsync
while `$DECSETTINGS_FAULT_SYNC_GATE` exists (max 120 s), add `$DECSETTINGS_FAULT_SYNC_MS`, and fail with EIO for file
names containing `$DECSETTINGS_FAULT_SYNC_FAIL`. Unit tests use a thread-local hook instead.

## Native evidence (private Xvfb, private session bus, AT-SPI)

`tests/e2e/settings_slow_disk.py` (reserved display 700–899, verified ours by lock pid plus filesystem and abstract
socket before any input; private HOME/XDG/dbus-run-session; at-spi2-registryd on the private bus). The disk is held for
6 s after the first edit. Debug builds; the baseline is 7f4faf0 plus only the fault hook.

Run 2026-10-31, `~/.cache/decebal-settings-io-evidence/`:

| | baseline 7f4faf0 + hook (`253f60c0…`) | candidate 316bff0 (`247dace8…`) |
| --- | --- | --- |
| UI thread while the write is held | `hrtimer_nanosleep` (inside the write) | `poll_schedule_timeout` (idle); `decset-persist` holds the write |
| pointer click → painted | 6052 ms | 105 ms |
| AT `increment` ×2 replied | only after the hold (6.2 s elapsed) | 58 ms max, `True`, while held |
| AT toast while held | already "Saved" (after blocking) | "Saving…", `busy`; nothing written yet |
| key/search/hover probes | ran after the hold | all within 1.14 s of the hold start |
| newest edits on disk exactly | yes | yes (unknown key kept) |
| failed fsync | action refused synchronously | action accepted; alert "Not saved: … Input/output error"; control reverted; file byte-identical |
| theme transaction failing on config.toml | all files byte-identical | all files byte-identical, no undo record, no temp file |
| Ctrl+W while held | window stays until the disk returns | window gone in 2 ms; process drains, exits 0 ≈1 s after the disk returns; edit on disk |
| threads | — | one `decset-persist`, one watcher |

No hardware, Wayland-compositor or Orca claims: this is Xvfb + libatspi 2.62.

## Not yet routed through the worker (still synchronous on the UI thread)

`ccpane` (control_center JSON), `configpane` save/presets, `panes` "Restore original"/create file, `packages`
duplicate/export/delete/install/snapshot, `system::session` and `system::notifications` files, `system` display scale,
`wallhaven::store` (prefs, API key with mode 0600, favourites), `wallpapercolors` engine writes,
`wallpaper_follow::cancel`, `Source::write_selection` callers. They remain correct (the worker detects their writes as
external changes and never overwrites them) but can still block under disk load. Next step: a `Job::Run` variant for
these owners, keeping the wallhaven credential file's 0600 mode and redaction.

## Coordination notes

Files outside `src/settings/io*` / `persist.rs` changed narrowly: `model.rs` (persistence hooks), `themepane.rs`,
`packages.rs`, `widgetchoice.rs`, `appearance.rs` (route theme transactions to `Model::apply_theme`),
`wallhaven/cli.rs` (headless inline writes), `tomlcfg.rs` (`array_move` fix), `app/a11y.rs` (toast `Busy` state and
one test), `Cargo.toml` (`settings-io-fault` feature). Pre-existing, unrelated: clippy lints in `a11y/atspi.rs` and
`a11y/bridge.rs`; `theme::engine::cli::tests::the_whole_workflow_from_new_to_apply_and_undo` fails in this private
environment on the foundation tree as well.
