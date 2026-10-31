# Packaging

| Target | How | Status |
|---|---|---|
| Debian / Ubuntu | `deb/build.sh [outdir]` after `make` | verified on Ubuntu 24.04 (before the stage.py switch) |
| Arch / Omarchy | `../PKGBUILD` | maintained |

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
  hashes every file before the first write (a bad source fails with nothing written) and refuses up front when any
  target folder is not writable. It installs into `$DESTDIR$PREFIX`: the 23 binaries, 18 `mac*` compatibility
  wrappers, 9 user units (`/usr/bin/` rewritten to `$PREFIX/bin/`), 5 application entries, the optional D-Bus service
  file (`$PREFIX/share/decebal/dbus/`, `Exec` rewritten; not activated), every file under `themes/` (fonts, wallpapers,
  widgets, licences; any file name except newlines), README and LICENSE; plus 9 autostart entries in
  `$DESTDIR/etc/xdg/autostart`. Each file is written to a temp name and renamed into place (a symlink at the target is
  replaced, never followed). Top-level `widgets/*.toml`, `assets/desktop-ports/*/asset-packs.toml` and
  `data/wallhaven-curated.json` are compiled into the binaries and are not installed.
* `make install` also writes `$PREFIX/share/decebal/install-inventory.json` (every installed path with its sha256 and
  the build's commit). On reinstall, files the new version no longer ships are removed only if unchanged since they
  were installed; `make uninstall` removes only inventory files that are unchanged and keeps (and reports) edited ones.
  Without an inventory, `make uninstall` removes nothing. Package builds (`--no-inventory`) leave the file list to
  pacman/dpkg, so `make uninstall` cannot delete package-owned files.
* `PREFIX` must be absolute with only `A-Za-z0-9_./+-` (it is written into unit and D-Bus files); `DESTDIR` may contain
  spaces. Values are passed through the environment, never interpolated into shell recipes.
* It never runs `systemctl`, `daemon-reload`, `update-desktop-database` or anything under `$HOME`, and enables nothing.
  Regressions: `python3 tests/packaging-stage.py` (fake cargo, private DESTDIR/HOME, no real build).

Not changed by this layer (runtime assets owned elsewhere): the autostart entries start their programs at login for
every user of a non-rdwm desktop (`NotShowIn=`/`OnlyShowIn=`/`TryExec=` as written in `contrib/`); autostart and
application entries use bare `Exec=` names, so a `PREFIX` whose `bin` is not on `PATH` needs PATH set up; theme
packages under a non-default `PREFIX` are only found when `$PREFIX/share` is in `XDG_DATA_DIRS`.
