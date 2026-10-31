#!/bin/sh
# Native recipe for the Breeze icon themes (breeze, breeze-dark) into a PRIVATE prefix, from a pinned local breeze-icons clone.
# Reproduces icons/CMakeLists.txt without ECM: index.theme = breeze.theme.in + commonthemeinfo.theme.in; icon folders copied as
# is; breeze-dark overlays the output of upstream's own tools/generate-symbolic-dark.cpp, compiled here with g++ against Qt6Core.
# Nothing is installed outside PREFIX; no live config, session or process is touched.
# usage: plasma-icon-recipe.sh BREEZE_ICONS_CLONE PREFIX WORKDIR
set -eu
SRC=$1; PREFIX=$2; WORK=$3
case "$PREFIX" in "$HOME"/.cache/*|/var/tmp/*) ;; *) echo "refusing a prefix outside ~/.cache or /var/tmp: $PREFIX" >&2; exit 2;; esac
rm -rf "$PREFIX" "$WORK"; mkdir -p "$PREFIX/share/icons/breeze" "$PREFIX/share/icons/breeze-dark" "$WORK/gen-light" "$WORK/gen-dark"
nice -n10 g++ -std=c++20 -O1 -fPIC $(pkg-config --cflags Qt6Core) "$SRC/tools/generate-symbolic-dark.cpp" -o "$WORK/generate-symbolic-dark" $(pkg-config --libs Qt6Core)
# the two theme indices (icons/CMakeLists.txt lines 36-41)
cat "$SRC/icons/breeze.theme.in" "$SRC/icons/commonthemeinfo.theme.in" > "$WORK/gen-light/index.theme"
cat "$SRC/icons/breeze-dark.theme.in" "$SRC/icons/commonthemeinfo.theme.in" > "$WORK/gen-dark/index.theme"
# symbolic dark icons (WITH_ICON_GENERATION off: inputs = source icons + light gen dir, output = dark gen dir)
QT_QPA_PLATFORM=offscreen "$WORK/generate-symbolic-dark" "$SRC/icons" "$WORK/gen-light" "$WORK/gen-dark" > "$WORK/generate.log" 2>&1 || { tail -5 "$WORK/generate.log"; exit 3; }
for d in "$SRC"/icons/*/; do n=$(basename "$d"); for t in breeze breeze-dark; do cp -a "$d" "$PREFIX/share/icons/$t/$n"; done; done
cp "$WORK/gen-light/index.theme" "$PREFIX/share/icons/breeze/index.theme"
cp "$WORK/gen-dark/index.theme" "$PREFIX/share/icons/breeze-dark/index.theme"
cp -a "$WORK/gen-dark"/. "$PREFIX/share/icons/breeze-dark/"
for t in breeze breeze-dark; do mkdir -p "$PREFIX/share/icons/$t/LICENSES"; cp "$SRC/COPYING.LIB" "$SRC/COPYING-ICONS" "$PREFIX/share/icons/$t/LICENSES/"; done
echo "built: $(find "$PREFIX/share/icons/breeze" -type f | wc -l) files in breeze, $(find "$PREFIX/share/icons/breeze-dark" -type f | wc -l) in breeze-dark"
