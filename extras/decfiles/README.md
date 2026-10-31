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
- Palette and profile follow the shell's existing GTK integration, without reading
  Decebal configuration. GTK reads the user's `gtk-4.0/gtk.css` once at startup.
  A second bounded monitor watches that folder (or its parent until it exists),
  reads the file asynchronously through GIO, and reapplies it at USER+1 priority,
  so palette changes apply live.
  The shell's `/* Files visual profile: explorer|finder (native file operations) */`
  marker, or an explicit `--profile=explorer|finder|native`, selects:
  - window-scoped `decfiles-explorer` / `decfiles-finder` surfaces, using only the
    palette's CSS variables (`--sidebar-bg-color`, `--sidebar-fg-color`) with
    neutral fallbacks and no geometry changes;
  - each platform's column vocabulary (Kind/Date Modified vs Type/Date modified);
  - the default layout (Finder: columns, Explorer: details).

  An explicit layout (the selector or `--columns`) is never changed by palette or
  profile updates; programmatic selector changes do not count as explicit. No
  marker, or no file, means Native. A unit test reads the shell's own
  `src/theme/system/file_manager.rs` and fails if its marker format or profile ids
  change. GTK's built-in theme does not read `--window-bg-color`, so only
  decfiles-owned surfaces take palette colours; the window background stays
  GTK's. Icons follow the GTK icon theme in effect at startup (`settings.ini`,
  also written by the shell); the icon theme is not live-reloaded.
- Search: the toolbar's native search entry (Ctrl+F) searches the current folder
  recursively for names containing the text (case-insensitive; non-UTF8 bytes match
  as U+FFFD while results keep the exact path). One `decfiles-search` worker per
  query never enters symlinks and is bounded to 32 levels, 200,000 visited entries
  and 2,000 results; at most four search threads may be alive, even if cancelled
  ones are still leaving a blocked syscall. Results stream incrementally into a
  Name/Folder/Type list in discovery order. A newer query, Escape, navigation or a
  tab switch cancels it, and stale results are ignored by search id. Activating a
  folder result opens it; a file result opens its folder with the file selected.
  File operations are unavailable while results are shown. Results are a
  snapshot and are not live-updated; there is no content search or index.
- Tabs: Ctrl+T opens the current folder in a new tab (at most 16). Each tab keeps its
  own navigation model (history, columns, selection) and explicit layout; the
  sort choice is window-wide. Ctrl+Tab/Ctrl+Shift+Tab and Ctrl+Page Down/Up
  switch, and Ctrl+W closes the active tab, or the window when it is the last
  one. The tab bar appears only with two or more tabs; it has equal-width
  ellipsized tabs, each with a close button, plus a New tab button. Switching
  happens only when no operation, dialog or clipboard read is active. A
  switched-to tab is re-watched and refreshed, because it may have missed changes
  while inactive. User actions supersede (cancel and re-arm) an in-flight
  background refresh instead of being ignored.
- The current folder is monitored: one `decfiles-monitor` inotify worker thread per
  window watches only the committed current physical folder (the watch's path
  lookup happens off the UI thread). Changes arrive latest-only over a wake
  socket. After a 300 ms debounce, an idle window refreshes quietly; while a
  dialog, operation, navigation or clipboard read is active, the refresh waits.
  A refresh that changes nothing visible keeps the existing widgets. Otherwise the
  selection survives if the item still exists, and keyboard focus stays in the
  file list. Ancestor columns are not watched.
- Selection signals from a view being torn down are ignored (per-render
  generation), and operations only target a selection validated by the model,
  so an earlier disposed view can never redirect Rename, Trash or Copy.
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
Copy. Names are encoded as URI bytes, so non-UTF8 names survive the round trip.

While the clipboard is read, navigation and other operations are refused, and the
whole read is limited to 5 seconds. When the data arrives, the paste also checks
that the folder and its location generation are unchanged, otherwise nothing is
pasted. A completed cut is consumed, as in GNOME Files, only when the content
read was our own provider object **and** that provider still owns the clipboard.
Newer content from any other owner is never cleared; ownership is checked by
provider identity, and our local cut state is dropped when another owner takes over.

Paste copies or moves into the current folder on one `decfiles-transfer` worker
thread per operation (one operation at a time). The UI thread never touches the
filesystem: it polls a latest-only snapshot every 50 ms for the progress row
(current item, bytes and items) and answers conflict prompts. Copies are counted
first and refused before any change above 100,000 items or 64 nested levels.

All work is relative to owned directory FDs and never follows the selected leaf.
Each selected item is **staged privately, then published atomically**:

- Files are written to an anonymous `O_TMPFILE` (or, where unsupported, an
  `O_CREAT|O_EXCL` file with a hidden, unpredictable `.decfiles-copy-…` name). The
  source is opened `O_NOFOLLOW|O_NONBLOCK` and must stay the same regular inode with
  the same size and modification time. The copy gets the permission bits
  (set-user/group-ID dropped) and access/modification times.
- Folders are populated inside a new hidden `0700` folder with an unpredictable
  `.decfiles-folder-…` name, verified as ours (our uid, fresh and empty), and
  written only through its FD. Children are staged and published inside it.
  Restrictive modes/times are applied last.
- Symlinks are recreated with their exact target. The source link must be the
  same inode, with the same length, before and after `readlinkat`; it is re-read
  up to three times, then refused as "kept changing".
- Publication is `linkat`, `renameat2(RENAME_NOREPLACE)` or `symlinkat`, all of
  which fail rather than replace an existing name. Afterwards, the published name
  must resolve to the staged inode, otherwise success is never claimed. A conflict
  at publication reuses the complete staging for Keep Both, without copying again.
  Skip, Stop, cancellation and failures publish nothing and remove only our own
  staging, through its FD.
- Devices, sockets and pipes are reported and never opened. Ownership, ACLs and
  extended attributes are not copied. Hard links are copied as separate files.

Move uses `renameat2(RENAME_NOREPLACE)`, atomically, on one filesystem. On `EXDEV`
the item is staged and published as above. Only then, in a separate pass, is
each original removed, and only if all of the following hold:

1. Its copy is still the recorded inode at the destination.
2. It was atomically claimed by a rename to a private `.decfiles-claim-…` name.
3. The claimed item is the same unchanged inode (size and modification time)
   that was copied.

Folders are removed last, only when empty. Anything that fails a check is
restored to its own name (never replacing anything) and kept, and the message
says why. An original that changed or was replaced, a copy that changed or
disappeared, or a read-only source folder therefore keeps the original. Children
of a partly copied folder that were copied are moved; the folder and the rest
stay. There is no Trash or permanent-delete fallback. Moving into the same folder
is skipped; moving or copying a folder into itself is refused.

**Concurrency contract.** Publication never overwrites. Our private staging and
claim names are unpredictable and hidden, but other processes of the same user
can still interfere with them deliberately. That cannot be prevented without
kernel support, so it is outside the contract; such interference is detected
where it would cause false success or removal of the wrong item. After
publication, other processes may change or delete the copy; the move re-checks
the copy immediately before claiming each original, but this is not a global
compare-and-swap across both filesystems. A crash between claim and removal can
leave an original under its `.decfiles-claim-…` name, and a crash during staging
can leave a hidden `.decfiles-…` folder; nothing is lost in either case.

A name conflict opens **Item already exists** with Stop, Skip (default) and Keep
Both, plus "Apply to all remaining conflicts". Escape or closing it means Stop.
Keep Both picks `name (copy).ext`, then `name (copy 2).ext`. Pasting into the
source folder duplicates that way without asking. There is **no Replace or
Merge** choice yet. The early existence check only decides when to ask; atomic
publication is the guarantee, so a destination created during a copy is never
overwritten.

Errors are per item: the rest continues, and the result names the first failure
and the count (at most 50 are retained). Cancellation stops between 256 KiB
chunks. Completed items remain, and the message says so; the item in progress,
including a partly copied folder, is not published. Cancel operation and the
close confirmation work as for the other operations; cancel-and-close does not
wait for the worker. There is no undo, and no power-loss durability guarantee
(no `fsync`).

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

Profile checkpoint: 25 Rust tests (two new, including the shell-marker contract).

The first full gate run (default `2885674298f9…`, `/var/tmp/decfiles-profile-gate-*`) failed
in transfers. A Paste click landed while the monitor's quiet refresh was loading,
and `update_operation_controls` had disabled the button for any pending load.
Earlier gates had passed only by timing. A quiet refresh now never disables
controls: every action already supersedes it, as tab controls did.

The fixed tree passes with default
`4239bfcd75a041b844acbf4cf515ca4134dd7df4b1a10b25f65d99acd305ce2d` and fixture
`757cb39de940487f233237fb85e55b71291d1fbdd6d97008e8e68a23191d388c`
(`/var/tmp/decfiles-profile2-artifacts/`), with every gate in
`/var/tmp/decfiles-profile2-gate-*-evidence/`: navigation 8, operations 13,
transfers 10, views 12, tabs 6, search 7, close 4, transfers-cancel 4,
paste-races 2+1, search-races 2, and the new `profile-native.py` 4+1.

The profile cases rewrite a private `XDG_CONFIG_HOME/gtk-4.0/gtk.css` in the
shell's forms while the app runs. Checks are on rendered sidebar pixels:

- the Finder marker selects Finder surfaces and default columns, with the
  sidebar rendered in the palette colour (#2a5a8a);
- a live rewrite switches to Explorer, details and the new palette (#c86432)
  without restart;
- an explicitly chosen layout survives palette and profile rewrites (#3c8c50);
- no marker or no file means Native;
- `--profile=finder` overrides the file's marker (`…-profile-evidence-cli/`).

Captures: `finder-start.png`, `explorer-live.png`.

Search checkpoint: 23 Rust tests (two new: nested/raw/link-free/bounded results,
and cancellation plus the live-worker bound). Default
`8b450524f150597dbc370b10d554d856014c495da0f7011936d27e93819abf3e` and fixture
`7aff7cf4aa653f90c6b7b325dba22ffa795a1117e4e1be3138f7053951cee026`
(`/var/tmp/decfiles-search-artifacts/`) pass every gate in `/var/tmp/decfiles-search-gate-*-evidence/`:
navigation 8, operations 13, transfers 10, views 12, tabs 6, close 4,
transfers-cancel 4 and paste-races 2+1, plus the new `search-native.py` 7 and,
on the fixture binary, `search-races-native.py` 2.

- **search-native.py:** results, revealing file and folder results, Escape,
  a background refresh during search, navigation leaving search, and an empty
  result.
- **search-races-native.py:** a newer query replacing a slow search with no
  stale results, and navigation cancelling mid-walk. It uses a capped
  `DECFILES_FIXTURE_SEARCH_DELAY_MS` per folder.

Tabs checkpoint: default `b5201c3b546410b1324774d6a14ec54a5496b665bd6720953a26fa86cdf100f2`
and fixture `702b12f27e75f9507d19a97f7735a6c11d14850e7072e37a61b683723001940a`
(`/var/tmp/decfiles-tabs-artifacts/`) pass every gate in `/var/tmp/decfiles-tabs-gate-*-evidence/`
(21 Rust tests): navigation 8, operations 13, transfers 10, views 12, close 4,
transfers-cancel 4, paste-races 2+1 and the new `tabs-native.py` 6. The tabs cases
cover Ctrl+T, per-tab history/layout across Ctrl+Tab/Ctrl+Shift+Tab, copy in one
tab and paste in another via actual tab buttons, monitoring following the active
tab and refreshing a switched-to tab, Ctrl+W closing a tab rather than the window,
and the close button.

Monitoring checkpoint: 21 Rust tests, including the monitor's revision-scoped
create, rename and watch-replacement test. Default
`9e588cf013bdfbc80658195b971edc50ffed1541cb61fecd4ad9531a94d63ac9` and fixture
`a01698e6c8a80bc84920e6cca7b1d37acde2604da070623a3c6e55623573e771`
(`/var/tmp/decfiles-monitor-artifacts/`) pass every gate in
`/var/tmp/decfiles-monitor-gate-*-evidence/` on reserved high private displays
(`tests/xserver.py`): navigation 8, operations 13, transfers 10, views 12, close 4,
transfers-cancel 4, paste-races 2+1. The six new views cases cover:

- an external create shown and the selection kept;
- focus kept in the list (the Down key moves the selection);
- an attribute-only change refreshed without re-rendering;
- an external rename/delete clearing a vanished selection;
- a refresh deferred while a dialog is open;
- the watch following navigation.

Safety checkpoint (staged publication, claimed cross-device removal, guarded paste
and owned clipboard clearing): there are 20 Rust tests. The new
`deterministic_races_never_remove_or_publish_the_wrong_item` drives test-only hooks
(compiled only under `cfg(test)`):

- an original replaced just before its claim is kept, with no claim name left;
- a published copy replaced or deleted before the original's removal keeps the original;
- a link swapped once while it is read is re-read, and one that keeps changing is refused;
- a private staging folder swapped before publication is never published, and the
  impostor is left alone;
- a destination created at publication reuses the staging for Keep Both (staged
  once), and Skip discards it;
- the `O_EXCL` fallback publishes, loses to a contender without leaving anything,
  and cancels cleanly.

Pinned default `3a6b6227ffdff087ab2b3d5b240535a8ece50d7e5179a557c36867c88765eac5`
and fixture `d046a9ef138a9b133a2ac400866bfdb160f99621f5112616d39db84516579472`
(`/var/tmp/decfiles-safety-artifacts/`) pass all gates in
`/var/tmp/decfiles-safety-gate-*-evidence/`: navigation 8, operations 13,
transfers 10 and views 6 on the default binary; close 4, transfers-cancel 4 and
the new `paste-races-native.py` 3 on the fixture binary. The paste races are:
navigation (Up, Back, sidebar) refused during a held clipboard read, with the
paste landing in the original folder; a cut moved across filesystems to a private
`/dev/shm` folder while another X owner takes the clipboard (that newer content
survives; verified bytes, original removed); and a 7 s read timing out at 5 s,
pasting nothing and releasing navigation (`…-paste-races-evidence-timeout/`).
The fixture-only `DECFILES_FIXTURE_CLIPBOARD_DELAY_MS` (capped at 30 s) holds the
read open.

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
picker, permanent delete, restore, undo, drag/drop, icon view, Finder gallery, Explorer
command bar, bookmarks, mounts, remote backends, content search/indexing, opening or
open-with exists yet. There are no decorative buttons pretending to do these.
Window background palette colours (GTK's built-in theme ignores them),
live icon-theme reload, notifications, default-handler opt-in and shell
launch/reveal integration are
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
