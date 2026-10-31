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

## One write protocol for every writer (`io`)

* **Per-file lock**, process wide and re-entrant per thread (`io::with_file_lock`): the worker, the theme engine
  (`io::write_atomic`), preset `fsx`, the System panes and the Wallhaven store all take it from their read to their
  rename, so two writers in this process never interleave on one file.
* **Unique temp file** per write (`.<name>.decsettings-<pid>-<n>.tmp`, `create_new`); the old per-PID name let two
  writers truncate or rename each other's temp.
* **Identity check at commit**, right before the rename and after the (possibly seconds-long) fsync: a file saved by
  another process meanwhile is never replaced (`io::CHANGED_ON_DISK`). TOML jobs then fail ("changed on disk", the
  window reloads their version); `io::json_update` reads the file again and re-applies its one key, so their keys and
  ours both survive, and refuses when someone else changed that same key (the window's `base` value differs and the
  file is not exactly as our own last write left it). A file that is not a JSON object is never overwritten.
* `io::write_private`: the same for secrets and caches, created 0600 from the start, no `.bak-decsettings` copy.
* rdwm reload: any committed config.toml change marks a reload due; it is sent once no TOML/theme/task job is left,
  whatever the last result was.

## Families (all through the one worker)

| Family | Job | Notes |
| --- | --- | --- |
| Controls on rdwm `config.toml` | `Toml` key patch | wallpaper changes stop a pending "match wallpaper" photo first, in the same job |
| `dock-settings.json`, `decebal.json`, pinned | `Json` one key, per-key base | Control Center and "Apply to all apps" merge into their objects (unknown keys kept); the apply request runs after the commit |
| Theme packages (selection, card, widget answer, reset, style) | `Theme` (planned in the worker) | shown selection = `ThemeRequest::planned_selection`, pinned to the planner by a test |
| Theme cards: duplicate, export, delete, install, snapshot, timed preview | `Task` | preview applies and puts back as one job |
| Config & Presets editor save / revert | `Task` | rdwmctl validation in the worker; typing during a save stays unsaved; a changed file is refused once, the next save overwrites |
| Presets apply / undo / save / import / duplicate / rename / delete / export | `Task` | the store's restore point and rollback are the transaction |
| General: Restore original, create file before Open | `Task` | |
| System panes: session-settings (display scale, keep layout, input, accessibility) | `Handle` from the runner thread | snapshot + enqueue under the `latest` lock: the newest snapshot is written last |
| notifications.toml | `Handle` + callback | shown at once, taken back with an error if the write fails |
| Wallhaven API key, key removal, favourites, filters, offline search cache | `Task` | the key moves in as a `Secret` (redacted Debug), 0600, no backup, never in a result or log line; cache writes are quiet |
| Match wallpaper: save as palette | `Task`, then a `Theme` job | |

`Model::run_task(label, writes, run, done)` is the API for owners (also for the asset lane's async downloads:
download on their own thread, then hand the durable install step to `run_task`, or use `persist::Handle` from that
thread). `run` gets owned, validated input; its `Value` result must not carry secrets; `done` shows the outcome.

Left on their own threads by design: Wallhaven thumbnail cache (download worker; now on the shared protocol),
theme `wallpaper_follow` photo commits (its own process and lock), and the panel pane's new writes (root adapts them
after merge, as agreed).

## Transactions and late results (second review round)

* **Theme packages**: every planned file carries the identity captured when planning started, before anything was
  read; each commits with `io::write_if_unchanged`. A file saved by someone else while an earlier file of the
  transaction sat in fsync (or during its own fsync) fails the transaction. Rollback puts back, newest first, only
  files still exactly as our write left them, conditionally; anything else is left as they saved it, the recovery
  record is kept and the error names what was restored, what was not and where the previous contents are.
  "nothing was changed" only when every file is back.
* **Config & Presets editor**: the identity the buffer was loaded from (or the one seen when the user chose to
  overwrite) holds through the slow `check-config` to `fsx::save_with_backup_if`, a conditional commit; no backup is
  left for a write that did not happen. `io::validate` uses one owned scratch file per check.
* **Launcher pane** and `config::write_setting` (other programs) use the worker / the shared protocol.
* **Newer intent wins**: a palette saved while another theme or palette was chosen is saved, not selected (a mode
  change meanwhile stays and the palette goes on top of it); session and notifications saves carry generations, so a
  failed older save never resets a newer pending change; a revert does not replace typing done while it was written.
* **Completions never block**: they update the model or enqueue work. "Apply to all apps" requests (`request` reads
  the file and writes a marker; `DECEBAL_SYSTEM_SYNC=1` applies inline) run as quiet tasks on the worker.

## Accessibility and visible state

* An action's reply means *accepted* (validated and queued, shown pending), never *durable*. A synchronous refusal
  (invalid value, full queue) is still the action's error.
* Notices follow the batch (idle to idle). While a window job is pending the toast says "Saving…" and is `busy`; a
  completion's error is shown at once (an alert on the page, the control shows the saved value again, newer accepted
  edits stay) and remains the batch's notice: later successes in the same batch do not hide it, and new submissions
  do not cover it with "Saving…". A success is announced only when nothing the user asked for is pending. Quiet
  housekeeping (offline caches, apply requests) counts for neither.
* System pane actions (commands plus session file) report when they finish, as before ("Saved" or the error); the
  control shows the new state at once.

## Third review round: held races (2146cb5)

* A write returns the identity of the file it put in place (from the open temp file, before the rename); rollback
  and the worker's own-write bookkeeping use it (and a per-thread commit log), never a later look at the path.
* Temp and scratch files: `io::create_owned` (create_new; a collision takes the next number; only our files are
  removed). `config::write_setting` is one protocol update (read, merge, commit under the lock).
* Returning to Notifications/Privacy/Accessibility does not re-read a file whose save of ours is pending; palette
  save uses explicit theme-intent generations; Revert keeps cursor, selection and scroll.
* Fail-before/pass-after: the nine `race_*` regressions, identical code, on c9ef5e7 (+ the test-only after-commit
  hook, `race-proofs/old-tree-hook-only.diff`) 0/9, on 2146cb5 9/9, each old failure being its race
  (`~/.cache/decebal-settings-io-evidence/race-proofs/`). Unit suite 998/998. Native 66/66 on binary `8f799c84…`
  (`full-candidate5b`); an earlier run on the same binary at load average ~13 from other builds had the editor save
  and Restore time out after the gate opened (the following fsync took over 15 s); re-runs pass.

## Asset packs (root 423c869)

The icon and cursor choice and its ownership record (`asset_packs`, `desktop_assets::policy`) are one atomic write:
`Model::edit_shell_doc` applies `policy::choose` to the shown `decebal.json` at once and, on the worker, to the file
as it is at commit (`io::json_update_doc`, compare-and-swap; a save made meanwhile is read again and the change
applied on top). Cursor choice/reset, "follow the theme" (both slots in one write), the icons pane's theme pick,
size, tint, fallback and reset, and the System pane's cursor field go through it; the icon cache poll and apply
requests are quiet tasks. While a change is pending the two panes show the shown document instead of the file.
Native: a cursor picked through AT-SPI answers in 118 ms while held; an editor's save of another key during the
held fsync is kept; the file then holds the cursor and `{"user": true}` together (66/66 checks, binary `b2be977f…`,
`full-candidate4`).

## Native evidence, concurrent cases (candidate `ffd8b40`, binary `8d6e25f8…`, 2026-10-31)

All 64 checks pass (`~/.cache/decebal-settings-io-evidence/full-candidate3/results.json`), the 54 below plus: a
package applied while decebal.json is held in fsync and config.toml is saved by hand meanwhile (the log shows
decebal.json and dock-settings.json committed, config.toml refused, both put back newest first; the hand-saved file
kept; "nothing was changed"), the launcher style answering in 114 ms while held (other `launcher` keys kept), and a
config.toml save during a slow `check-config` refused with the hand-saved file kept.

## Native evidence, every family (`settings_slow_disk.py`, candidate `379c618`+tests, 2026-10-31)

All 54 checks pass on the candidate (`~/.cache/decebal-settings-io-evidence/full-candidate2/results.json`). Replies
while every fsync is held: JSON value 196 ms, TOML toggle 118 ms, theme mode 513 ms (tab switch included), editor
Save 175 ms, preset Apply 173 ms, Restore 110 ms, session toggle 77 ms, notifications toggle 87 ms, Wallhaven key
177 ms. Their file saved during our held fsync: both keys kept (JSON) / their file kept and the change refused
visibly (TOML, same JSON key). Injected EIO: the preset apply rolls every file back byte for byte; notifications are
taken back with an alert. Close with the disk never returning: window gone in 2.3 ms, the process gives up after the
30 s drain limit (30.06 s) and logs the change as not written. The same family run on the baseline
(7f4faf0 + hook) stops at the first held write: the AT request is handled on the UI thread, which sits in the held
fsync (`full-baseline3/root/apps/json-merge/logs`), the client times out and libatspi 2.62 aborts on that timeout.

## Coordination notes

Files outside `src/settings/io*` / `persist.rs` changed narrowly (first round): `model.rs` (persistence hooks), `themepane.rs`,
`packages.rs`, `widgetchoice.rs`, `appearance.rs` (route theme transactions to `Model::apply_theme`),
`wallhaven/cli.rs` (headless inline writes), `tomlcfg.rs` (`array_move` fix), `app/a11y.rs` (toast `Busy` state and
one test), `Cargo.toml` (`settings-io-fault` feature). Pre-existing, unrelated: clippy lints in `a11y/atspi.rs` and
`a11y/bridge.rs`; `theme::engine::cli::tests::the_whole_workflow_from_new_to_apply_and_undo` fails in this private
environment on the baseline (7f4faf0 + fault hook only) exactly as on the candidate (`cli.rs:383`, `validate`).
