# Decebal Files — optional preview

This is a functional browser prototype with three single-item operations, **not a complete Explorer/Finder port**
and not a production file manager. It uses one navigation model with two views:
Windows-inspired details and macOS-inspired hierarchical columns. It does not
copy either product's artwork or claim matching appearance. Nothing installs a
handler, changes the default file manager, or adds GTK to the main shell build.

## Build and try

This directory is an independent Cargo workspace. Dependencies are pinned in
`Cargo.toml` and `Cargo.lock`: GTK4 0.10.3, GIO/GLib 0.21.5. System GTK4/GIO and
Rust are required; the validation host had GTK4 4.24.1, GIO 2.90.1 and Rust 1.99.1.
No libadwaita dependency is used.

```sh
CARGO_TARGET_DIR="$HOME/.cache/decebal-files-target" TMPDIR=/var/tmp \
  cargo build --offline -j2 --manifest-path extras/decfiles/Cargo.toml
"$HOME/.cache/decebal-files-target/debug/decfiles" /path/to/folder
"$HOME/.cache/decebal-files-target/debug/decfiles" --columns /path/to/folder
```

The `--columns` switch only chooses the initial view. The in-window selector
switches views without clearing navigation or valid selection. This prototype
does not save preferences or read/write Decebal configuration.

## Implemented behavior

- Editable local-folder address; relative paths and `~` resolve from the current
  folder and home. Back, Forward, Up, Home, Documents, Downloads and File system
  controls navigate actual folders. Ctrl+L focuses location, Alt+Left/Right/Up
  navigate, Ctrl+R refreshes and Ctrl+W closes the window. Ctrl+Shift+N creates a
  folder, F2 renames the selected item, and Delete from the file list asks to move
  the selection to Trash.
- Details has real Name/Kind/Size columns and keyboard/pointer selection.
  Enter/double click opens a folder. Columns display parent and child contents;
  clicking a directory appends its contents. Choosing a sibling in an ancestor
  column keeps that parent and replaces the columns to its right.
- Directory results are sorted folders-first, then by case-insensitive name.
  File selection keeps the actual Unix path even for non-UTF8 filenames; GIO
  provides a readable display name. Files are **not opened or executed**.
- Missing, unreadable or oversized folders show an error and preserve the prior
  committed location and history. Stale results cannot replace a newer request.
- One path-resolution thread has one replaceable pending request and one result
  slot. Physical resolution happens before constructing GLocalFile, because GLib
  otherwise collapses `link/..` lexically. The address shows the resolved physical
  identity (symlink alias spelling is not retained). Resolution never runs in a
  paint or input handler. A blocked filesystem syscall cannot be force-cancelled;
  the UI remains responsive, a newer request replaces the pending one, and close
  does not join a blocked worker on the UI thread. The process exits normally.
- GIO reads directory metadata asynchronously in 128-entry batches, with explicit
  cancellation and request revisions. A folder is published only on complete
  success. There are at most 8,192 entries per folder, 16 retained columns and
  64 history paths; larger folders report the limit rather than display partial
  results as a complete folder. There is no global file index.

## Create, rename and Trash

The footer's New folder, Rename and Move to Trash buttons operate on real local
items. Name dialogs validate one literal leaf name (no empty name, slash, NUL,
`.` or `..`); a rename cannot reuse the original name. Conflicts return an error
without overwriting an existing file, folder or dangling symlink. A successful
create/rename refreshes the affected folder and selects the resulting item. A
failed operation preserves the prior location and selection.

Trash always asks for explicit confirmation naming the item and its location.
Cancel is the default response/focus; Enter and Escape cancel. GIO Trash is the
only implementation: there is **no permanent-delete fallback** when Trash is
unavailable. The selected symlink leaf is renamed/trashed, never its target.
Non-UTF8 source names use their original Unix path bytes; replacement names are
UTF-8 entered through the native dialog.

There is only one in-flight operation and one modal dialog. New operations and
navigation are blocked while that operation runs, but the event loop stays
responsive and Cancel operation requests cancellation. Cancellation before
dispatch changes nothing. Cancellation racing a GIO create/trash may arrive
after a filesystem change; that case refreshes the folder and asks the user to
check it, rather than claiming rollback. A known successful atomic rename stays
successful even when cancellation arrives before its UI completion callback.
There is no undo implementation.

Closing during an operation opens a native confirmation, defaulting to Keep
Open. Cancel operation and close requests cancellation, then permits normal
window/process shutdown **without waiting or joining the filesystem worker**. A
change that already finished may remain. This escape also works if a local
filesystem operation is stuck; it cannot roll back an operation or promise
cleanup after a syscall already entered the kernel. The private timing test uses
a controlled queued delay, not an actually hung physical filesystem.

Create uses typed `GFile.make_directory_async`; Trash uses typed
`GFile.trash_async`. Rename is a deliberately narrow Linux exception: it runs
`renameat2(RENAME_NOREPLACE)` relative to one owned parent directory FD on GIO's
blocking worker pool. Inspection of the exact installed
[GLib 2.90.1 local backend](https://raw.githubusercontent.com/GNOME/glib/2.90.1/gio/glocalfile.c)
showed both `set_display_name` and `move` doing a destination check followed by
`g_rename`, which can overwrite a destination created in between. The prototype
therefore does not use that sequence or a check-then-rename workaround. Unsupported
Linux/filesystem atomic-rename support returns an error, with no overwriting or
copy/delete fallback. There is no power-loss durability guarantee.

## Validation and evidence

Run from the shell repository root:

```sh
CARGO_TARGET_DIR="$HOME/.cache/decebal-files-target" TMPDIR=/var/tmp \
  cargo test --offline -j2 --manifest-path extras/decfiles/Cargo.toml
CARGO_TARGET_DIR="$HOME/.cache/decebal-files-target" TMPDIR=/var/tmp \
  cargo clippy --offline -j2 --manifest-path extras/decfiles/Cargo.toml \
  --all-targets -- -D warnings
cargo fmt --manifest-path extras/decfiles/Cargo.toml --check
DECFILES_EVIDENCE=/var/tmp/decfiles-native-evidence \
  python3 extras/decfiles/tests/native.py
```

`DECFILES_BIN` chooses a different dev binary. The native script owns its Xvfb,
private session bus, HOME/XDG directories and fixture files. It never uses the
live display/bus or changes real files. It drives actual GTK controls using
xdotool and checks the committed model through an explicit test-only snapshot.
The snapshot is disabled unless `DECFILES_EVIDENCE_STATE` names an output file;
it includes display strings **and original Unix bytes** for path identity. It
must not be enabled for a normal/private user session without consent.

Current evidence is split by scope and build features:

- `/var/tmp/decfiles-navigation-operations-evidence/result.json`: feature-free
  binary; eight existing real-control navigation/selection regression cases.
- `/var/tmp/decfiles-operations-native-evidence/result.json`: feature-free
  binary; 13 actual-control operation cases, including name validation, create and
  rename conflicts/success, preserved contents and selection, default Cancel and
  Escape, real folder/symlink Trash, non-UTF8 source rename, denied creation,
  `.trashinfo` paths and actual Trash payloads. Captures include
  `invalid-name.png`, `rename-conflict.png` and `trash-confirmation.png`.
- `/var/tmp/decfiles-close-native-evidence/result.json`: **only** the separate
  `fixture-delay` binary; actual Cancel operation before dispatch, Keep Open by
  default Enter/Escape, and explicit cancel-and-close without waiting. It uses a
  controlled 5,000 ms pre-dispatch delay, not a blocked physical filesystem. The
  fixture verifies unchanged files after cancellation/close and normal exit.
- `/var/tmp/decfiles-operations-artifacts/{default,fixture-delay}/manifest.json`:
  exact binary hashes, build features, commands and source hashes. The binaries
  are pinned separately so the fixture build cannot replace the accepted normal
  binary. `result.json` in each native directory also records exact hashes.

There are 14 Rust tests. The isolated operation test launches a fresh subprocess
with private HOME/XDG **before GLib initializes its cached Trash paths**, verifies
bytes/metadata/permissions, dangling and selected symlinks, non-UTF8 paths and
cancellation before start. Thirty-two pairs of simultaneous rename contenders
prove one winner and one Exists, with the loser's source intact. A separate test
pauses UI completion dispatch, waits for an actual atomic rename to commit, then
cancels: the later callback must still report its known success. GIO cancellation
errors instead retain an explicit uncertain-outcome state. Logs are
`/var/tmp/decfiles-operations-{build,fixture-build,tests,clippy}.log`.

The ordinary build has **no delay hook**. Only an explicit Cargo
`--features fixture-delay` build compiles handling of
`DECFILES_FIXTURE_DELAY_MS`, capped at 30,000 ms and cancellable before dispatch.
Do not install or distribute that fixture artifact. Re-run timing evidence with:

```sh
# First pin the normal dev binary; build/pin the fixture separately.
CARGO_TARGET_DIR="$HOME/.cache/decebal-files-target" TMPDIR=/var/tmp \
  cargo build --offline -j2 --manifest-path extras/decfiles/Cargo.toml \
  --features fixture-delay
DECFILES_FIXTURE_BIN=/path/to/pinned/fixture/decfiles \
  DECFILES_CLOSE_EVIDENCE=/var/tmp/decfiles-close-native-evidence \
  python3 extras/decfiles/tests/close-native.py
DECFILES_BIN=/path/to/pinned/normal/decfiles \
  DECFILES_OPERATIONS_EVIDENCE=/var/tmp/decfiles-operations-native-evidence \
  python3 extras/decfiles/tests/operations-native.py
```

GTK reported software-Xvfb DRI3/frame-timing warnings during the fixture; these
are retained in `app.log`. No physical GPU/frame-rate claim is made. An early
harness used `xdotool windowclose`, which forcibly destroyed the X window and
caused GTK BadDrawable; the final harness uses the app's Ctrl+W close and requires
exit status 0.

## Remaining work before a real file manager

No general copy/move, permanent delete, restore, undo, overwrite/merge choices,
clipboard file formats, drag/drop, tabs, icon view, Finder gallery, Explorer
command bar, bookmarks, mounts, remote backends, monitoring, search, opening or
open-with exists yet. There are no decorative buttons pretending to do these.
Sorting is fixed rather than header-driven. Profile styling, palette/icons,
notifications, default-handler opt-in and shell launch/reveal integration are
also pending. The address entry cannot type arbitrary non-UTF8 bytes; pointer
navigation/selection preserves them losslessly.

Standard GTK widgets supply focus and accessibility infrastructure, **not proof
of Orca acceptance**. Real screen-reader workflows, high contrast, reduced motion,
100/125/200% UI/text, short/narrow windows, Wayland, physical input/GPU, large/slow
folders and resource benchmarks still need acceptance. There is no installer,
.desktop registration or release build. Existing Nautilus remains untouched.

## Primary implementation references

- GIO [`GFile`](https://docs.gtk.org/gio/iface.File.html),
  [`enumerate_children_async`](https://docs.gtk.org/gio/method.File.enumerate_children_async.html),
  [`GFileEnumerator.next_files_async`](https://docs.gtk.org/gio/method.FileEnumerator.next_files_async.html)
  and [`GCancellable`](https://docs.gtk.org/gio/class.Cancellable.html).
- GTK [`ColumnView`](https://docs.gtk.org/gtk4/class.ColumnView.html),
  [`ListView`](https://docs.gtk.org/gtk4/class.ListView.html) and
  [`SingleSelection`](https://docs.gtk.org/gtk4/class.SingleSelection.html).
- The pinned gtk-rs/GIO sources in the local Cargo registry were used to verify
  exact callback signatures. No external file-manager source or artwork was
  copied.
