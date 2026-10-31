#!/bin/sh
# Build decebal_<version>_<arch>.deb from an already-built tree.
# Usage: packaging/deb/build.sh [outdir]      (run `make` first: it records the exact binaries to package)
set -eu
cd "$(dirname "$0")/../.."
out=${1:-dist}
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(dpkg --print-architecture)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
# Same file list as `make install`; no install inventory, dpkg owns the file list.
python3 packaging/stage.py install --prefix /usr --destdir "$stage" --no-inventory >/dev/null
mkdir -p "$stage/DEBIAN" "$out"
size=$(du -sk "$stage" | cut -f1)
cat > "$stage/DEBIAN/control" <<CTL
Package: decebal
Version: $ver
Architecture: $arch
Maintainer: Decebal contributors <noreply@example.invalid>
Installed-Size: $size
Section: x11
Priority: optional
Depends: libc6, libxkbcommon0, libpam0g
Recommends: rdwm
Suggests: fastfetch
Conflicts: macshell
Replaces: macshell
Provides: macshell
Homepage: https://github.com/fspecii/decebal-shell
Description: Decebal desktop shell: bar, dock, launcher, notifications, settings
 A lightweight, themeable desktop shell for X11 window managers such as rdwm.
 Ships decbar, decdock, decspot, decnotify, decsettings, decthemectl and more,
 with a data-only theme engine for ricing.
CTL
dpkg-deb --root-owner-group --build "$stage" "$out/decebal_${ver}_${arch}.deb" >/dev/null
echo "$out/decebal_${ver}_${arch}.deb"
