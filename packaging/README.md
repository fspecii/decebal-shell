# Packaging

| Target | How | Status |
|---|---|---|
| Debian / Ubuntu | `deb/build.sh [outdir]` after `make` (needs `python3`, `dpkg-deb`) | verified on Ubuntu 24.04 (before the stage.py switch) |
| Arch / Omarchy | `../PKGBUILD` (makedepends `cargo`, `python`) | maintained |

Installed-package smoke test (Ubuntu 24.04 container): the rdwm and decebal `.deb` files build and install with
`dpkg -i`; rdwm, decbar, decdock and decnotify start and stay up on a private X server, and `rdwmctl` and `decthemectl`
answer. The full e2e suite runs against a source build, not against an installed package.

## Install contract (`stage.py`)

`make`, `make install`, `make uninstall`, the PKGBUILD and `deb/build.sh` all use `packaging/stage.py`, which holds the
one file list:

* `build` runs `cargo build --release --locked --message-format=json-render-diagnostics` and writes
  `BUILD_MANIFEST` (default `target/decebal-build-manifest.json`) with the path and sha256 of each of the 23 binaries
  **as cargo reported them for that build**. `CARGO_TARGET_DIR` or cargo config may put them anywhere; a stale
  `./target/release` is never read. A binary missing from cargo's report fails the build.
* `install` re-hashes every recorded binary (a file changed since the build is refused), then reads, rewrites and
  hashes every file before the first write. Sources are refused when `fstat` reports more than the size cap or when
  more bytes than the cap are actually read (no silent truncation). It installs into `$DESTDIR$PREFIX`: the 23
  binaries, 18 `mac*` compatibility wrappers, 9 user units (`/usr/bin/` rewritten to `$PREFIX/bin/`), 5 application
  entries, the optional D-Bus service file (`$PREFIX/share/decebal/dbus/`, `Exec` rewritten; not activated), every file
  under `themes/` (fonts, wallpapers, widgets, licences; any file name except newlines), README and LICENSE; plus 9
  autostart entries in `$DESTDIR/etc/xdg/autostart`. Top-level `widgets/*.toml`,
  `assets/desktop-ports/*/asset-packs.toml` and `data/wallhaven-curated.json` are compiled into the binaries.
* **Confinement.** `DESTDIR` (or `/`) is the one trusted anchor: it is opened once, and every folder below it is
  reached one component at a time with `openat(O_DIRECTORY|O_NOFOLLOW)` (created with `mkdirat` when missing). Files
  are created, replaced (temp file + `renameat` in the same held folder), read and unlinked only through those held
  folder descriptors. A symlinked or non-directory intermediate folder is refused, in a preflight that runs before the
  first write and again on every later step, so a folder swapped for a symlink mid-run is refused, not followed. A
  symlink at the final name is replaced, never written through; a directory there is refused up front. DESTDIR itself
  may be a symlink (it is the caller's own staging root).
* `make install` also writes `$PREFIX/share/decebal/install-inventory.json` (every installed path with its sha256 and
  the build's commit). It is read only as a bounded regular file reached without symlinks, only for the exact
  requested `PREFIX`, and every path in it must be in this package's own namespace (`$PREFIX/bin/{dec,mac}*`,
  `$PREFIX/lib/systemd/user/decebal-*.service`, `/etc/xdg/autostart/decebal-*.desktop`,
  `$PREFIX/share/applications/dec*.desktop`, `$PREFIX/share/{decebal,doc/decebal,licenses/decebal}/...`); anything
  else makes install and uninstall refuse. On reinstall, files the new version no longer ships are removed only if
  still regular and unchanged; `make uninstall` removes only unchanged inventory files and keeps (and reports) edited
  ones. Without an inventory, `make uninstall` removes nothing.
* Package builds (`--no-inventory`) stage no inventory. That only keeps the staged package clean: if a manual
  `make install` with the same `PREFIX` left an inventory behind and a package later installs over it, that old
  inventory still lists the (now package-owned) paths, and `make uninstall` would remove those whose content still
  matches. No migration guard exists; delete the old inventory (or run `make uninstall` first) before switching to a
  package.
* `PREFIX` must be absolute with only `A-Za-z0-9_./+-` (it is written into unit and D-Bus files); `PREFIX=/` is
  refused. `DESTDIR` may contain spaces. Values are passed through the environment, never interpolated into shell
  recipes.
* It never runs `systemctl`, `daemon-reload`, `update-desktop-database` or anything under `$HOME`, and enables nothing.
  Regressions: `python3 tests/packaging-stage.py` (fake cargo, private DESTDIR/HOME, no real build).
* Limits: the install is not atomic across files (a write failure part-way leaves a mix; rerunning repairs it). Between
  the digest check and the unlink of a file being removed, a process that can write that installed folder could
  replace the file; folders are root-owned for real installs.

Not changed by this layer (runtime assets owned elsewhere): the autostart entries start their programs at login for
every user of a non-rdwm desktop (`NotShowIn=`/`OnlyShowIn=`/`TryExec=` as written in `contrib/`); autostart and
application entries use bare `Exec=` names, so a `PREFIX` whose `bin` is not on `PATH` needs PATH set up; theme
packages under a non-default `PREFIX` are only found when `$PREFIX/share` is in `XDG_DATA_DIRS`.
