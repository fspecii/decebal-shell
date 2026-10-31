# Decebal Files — optional preview

This is a functional browser prototype with create, rename, Trash, copy and move, **not a complete Explorer/Finder port**
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
  the selection to Trash. In the file list, Ctrl+C, Ctrl+X and Ctrl+V copy, cut and
  paste; the address entry keeps its ordinary text clipboard keys.
- Details has real Name/Type/Size/Date modified columns and keyboard/pointer
  selection. Clicking a header sorts natively (again to reverse); folders always
  stay first. The sort choice survives re-rendering, view switches, navigation and
  history. Names show the user's GTK icon theme icon for GIO's extension-based
  `fast-content-type` (contents are never sniffed while listing); Type uses GIO's
  description, such as "Plain text document".
  Enter/double click opens a folder. Columns display parent and child contents;
  clicking a directory appends its contents. Choosing a sibling in an ancestor
  column keeps that parent and replaces the columns to its right.
- Folder results start folders-first, then by case-insensitive name; ties in any
  header sort fall back to that name and then the exact path bytes.
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

## Copy, move and the clipboard

The footer's Cut, Copy and Paste buttons (and Ctrl+X/C/V in the file list) are real.
Cut and Copy only publish the selected item; nothing changes on disk until Paste.
The clipboard uses GNOME Files' `x-special/gnome-copied-files` (`copy`/`cut`, then
one file URI per line) plus RFC 2483 `text/uri-list` and plain text. Paste reads
those formats asynchronously from any owner (at most 1 MiB, 4,096 local `file://`
items; remote hosts and other schemes are refused). A plain URI list always means
Copy. A completed cut is consumed, as in GNOME Files; a partial one stays. Names
are encoded as URI bytes, so non-UTF8 names survive the round trip.

Paste copies or moves into the current folder on one `decfiles-transfer` worker
thread per operation (one operation at a time). The UI thread never touches the
filesystem: it polls a latest-only snapshot every 50 ms for the progress row
(current item, bytes and items) and answers conflict prompts. Copies are counted
first and refused before any change above 100,000 items or 64 nested levels.

All work is relative to owned directory FDs and never follows the selected leaf:

- Files are written to an anonymous `O_TMPFILE`, checked, given the source
  permission bits (set-user/group-ID dropped) and access/modification times, then
  published with `linkat`, which fails rather than replace an existing name. A
  cancelled, failed or source-changed copy publishes nothing. Without `O_TMPFILE`
  the fallback is `O_CREAT|O_EXCL`, removing only its own inode-confirmed partial.
- The source is opened `O_NOFOLLOW|O_NONBLOCK` and must remain the same regular
  inode; size and modification time are compared after copying. Changes are
  reported as failures, never published as a complete copy.
- Folders use `mkdirat` (no merge), are populated, then receive their mode/times
  last so read-only folders copy correctly. Symlinks are recreated with
  `symlinkat`, keeping their exact target, including dangling links. Devices,
  sockets and pipes are reported and never opened. Ownership, ACLs and extended
  attributes are not copied. Hard links are copied as separate files.
- Move uses `renameat2(RENAME_NOREPLACE)`, atomically, on one filesystem. On
  `EXDEV` it copies as above, then removes each original only if it is still the
  same unchanged item, and removes folders only when empty. Originals that cannot
  be removed (for example inside a read-only folder) are kept and reported. There
  is no Trash or permanent-delete fallback for failures. Moving into the same
  folder is skipped; moving or copying a folder into itself is refused.

A name conflict opens **Item already exists** with Stop, Skip (default) and Keep
Both, plus "Apply to all remaining conflicts". Escape or closing it means Stop.
Keep Both picks `name (copy).ext`, then `name (copy 2).ext`, each attempt still an
atomic no-replace create. Pasting into the source folder duplicates that way
without asking. There is **no Replace or Merge** choice yet. The early existence
check only decides when to ask; the atomic create is the guarantee, so a
destination created by another process during a copy is never overwritten.

Errors are per item: the rest continues, and the result names the first failure
and the count (at most 50 are retained). Cancellation stops between 256 KiB
chunks; items already finished remain and the message says so, including a
partially populated new folder. Cancel operation and the close confirmation work
as for the other operations; cancel-and-close does not wait for the worker. An
`O_TMPFILE` in progress then vanishes with the process. There is no undo, and no
power-loss durability guarantee (no `fsync`).

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

Copy/move evidence (default binary
`04c7182fae2ac626ecc4ba0c1faf1aa5a32adda8b5d1c01681fcd7d519b39e65`, fixture binary
`9741629367af6558d0a40c066a8bd6aaaaa73f9b99cf63762c15135c44e0b0e3`, both pinned with
11 source hashes in `/var/tmp/decfiles-transfer-artifacts/{default,fixture-delay}/`):

- `/var/tmp/decfiles-transfers-native-evidence/result.json` (`tests/transfers-native.py`):
  feature-free binary, 10 actual-control cases. They cover the Copy button
  publishing both clipboard formats (read back with `xclip` on the private X
  server), Enter = Skip at a conflict, Keep Both with verified bytes, mode and
  nanosecond mtime, keyboard folder copy with a relative symlink, non-UTF8 names,
  Cut of a selected symlink (moved, target untouched, cut consumed), and an
  external GNOME-format `cut` owner pasted as a move. An external `text/uri-list`
  owner with Escape = Stop, a denied destination and same-folder duplication are
  also covered. Captures: `conflict-dialog.png`, `denied-paste.png`,
  `duplicate-selected.png`.
- The same binary passed the earlier 13 operation and eight navigation cases:
  `/var/tmp/decfiles-transfer-{operations,navigation}-evidence/result.json`.

Sorting checkpoint: `tests/gates.py ARTIFACTS PREFIX` runs every native suite
against binaries pinned by `tests/pin.py` and checks every result's binary and
source hashes. With default `ea0797103300b8ffa6dfa3fece32b9bb095af0427e7a6035f6b850fa520f05fb`
and fixture `b03b59004e5bbfcee57e49c26e02c7274cbaf9d281992072aec96abfffb9ff6f`
(`/var/tmp/decfiles-views-artifacts/`), all 45 cases passed. They are navigation 8,
operations 13, transfers 10 and the new `views-native.py` 6 (default sort, GIO
types, Size ascending/descending keeping selection, Date modified, sort kept
across columns and back and across navigation/Back), plus close 4 and
transfers-cancel 4 on the fixture. Results are in `/var/tmp/decfiles-views-gate-*-evidence/`.
The gtk4 crate's `v4_10` feature is enabled for `ColumnViewSorter`. GtkDialog is
deprecated since GTK 4.10, so the two dialog modules allow that deprecation
until a reviewed AlertDialog migration preserves their tested default responses.
- `/var/tmp/decfiles-transfers-cancel-native-evidence/result.json`
  (`tests/transfers-cancel-native.py`, fixture binary only): a 150 ms delay per
  256 KiB chunk (`DECFILES_FIXTURE_TRANSFER_DELAY_MS`, capped at 5 s). It covers byte
  progress and the actual Cancel operation mid-file with nothing published, close
  confirmation Keep Open with the copy then completing verified, an appended
  source mid-copy reported with no destination file, and cancel-and-close exiting
  in about 0.26 s with no partial file. The earlier four close cases also pass
  on this binary: `/var/tmp/decfiles-transfer-close-native-evidence/result.json`.

The new harnesses accept a private Xvfb only after it answers, because other
sessions' Xvfb servers can make `-displayfd` fail ("Cannot write display number");
the older scripts may need a rerun when that happens.

There are 19 Rust tests. The five transfer tests run on private `/var/tmp` trees,
with a private `/dev/shm` folder as a real second filesystem for cross-device
moves. They check bytes, modes, nanosecond mtimes, relative/absolute/dangling and
selected symlinks, non-UTF8 names and read-only folders. Skip, Keep Both, Stop,
apply-to-all and cancellation while a prompt waits are exercised through the
public poll/answer interface. Also covered: denied sources and destinations,
FIFOs, mid-file cancellation, cancellation inside a folder, a source appended
mid-copy, and a destination created by another writer mid-copy (never
overwritten). Further tests cover kept originals in a read-only source folder,
Keep Both names and clipboard encoding/decoding.

Of the original 14 tests, the isolated operation test launches a fresh subprocess
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
DECFILES_BIN=/path/to/pinned/normal/decfiles \
  DECFILES_TRANSFERS_EVIDENCE=/var/tmp/decfiles-transfers-native-evidence \
  python3 extras/decfiles/tests/transfers-native.py
DECFILES_FIXTURE_BIN=/path/to/pinned/fixture/decfiles \
  DECFILES_TRANSFERS_CANCEL_EVIDENCE=/var/tmp/decfiles-transfers-cancel-native-evidence \
  python3 extras/decfiles/tests/transfers-cancel-native.py
# Pin a built binary with its exact source hashes (add feature names if any):
python3 extras/decfiles/tests/pin.py BINARY OUT_DIR [fixture-delay]
```

Run Cargo with an isolated environment: private HOME/XDG directories, no
`DISPLAY`/`WAYLAND_DISPLAY`/`XAUTHORITY`/session variables, invalid session and
system bus addresses, and an explicit `CARGO_HOME`. The unit tests need no display.

GTK reported software-Xvfb DRI3/frame-timing warnings during the fixture; these
are retained in `app.log`. No physical GPU/frame-rate claim is made. An early
harness used `xdotool windowclose`, which forcibly destroyed the X window and
caused GTK BadDrawable; the final harness uses the app's Ctrl+W close and requires
exit status 0.

## Remaining work before a real file manager

Copy/move/paste work on one selected item at a time (pasted clipboards may hold
many). There is no multi-selection, Replace/Merge conflict choice, Copy to/Move to
picker, permanent delete, restore, undo, drag/drop, tabs, icon view, Finder gallery, Explorer
command bar, bookmarks, mounts, remote backends, monitoring, search, opening or
open-with exists yet. There are no decorative buttons pretending to do these.
Profile styling, palette following,
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
