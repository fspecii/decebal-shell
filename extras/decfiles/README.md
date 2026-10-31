# Decebal Files — optional read-only foundation

This is a functional browser prototype, **not a complete Explorer/Finder port**
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
  navigate, Ctrl+R refreshes and Ctrl+W closes the window.
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

The current evidence directory is `/var/tmp/decfiles-native-evidence`:
`result.json` records the exact binary SHA and Rust/Cargo source hashes;
`windows-details.png`, `macos-columns-root.png` and
`macos-columns-sibling.png` are actual native captures. Ten tests cover actual
GIO batches, cancellation, real symlink-parent resolution, non-UTF8 identity,
failed/stale requests, repeated pending history navigation, selection, sibling
columns and bounded state. The native fixture covers real-control details
activation, back/forward/up, missing/unreadable folder recovery, physical
symlink-parent address semantics, child/sibling column navigation, selection and
graceful close. Logs are `/var/tmp/decfiles-{build,test,clippy}.log`.

GTK reported software-Xvfb DRI3/frame-timing warnings during the fixture; these
are retained in `app.log`. No physical GPU/frame-rate claim is made. An early
harness used `xdotool windowclose`, which forcibly destroyed the X window and
caused GTK BadDrawable; the final harness uses the app's Ctrl+W close and requires
exit status 0.

## Remaining work before a real file manager

No copy/move/trash/delete/restore/rename/create, progress/undo/conflict handling,
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
