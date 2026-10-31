#!/bin/sh
# Renders every Plasma package at every panel edge with the real native preview painter (decthemectl theme render).
# Usage: plasma-edge-matrix.sh DECTHEMECTL OUT_DIR   -- private copies only; no live config, display or process is touched.
set -eu
DT=$1; OUT=$2; ROOT=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$OUT/home" "$OUT/themes"
for id in plasma-breeze plasma-layan; do
  for edge in top bottom left right; do
    for mode in dark light; do
      d="$OUT/themes-$id-$edge"; rm -rf "$d"; mkdir -p "$d"; cp -r "$ROOT/themes/$id" "$d/$id"
      # dock.position and bar.position are the only panel-edge keys of format 1
      sed -i "s/^position = \"[a-z]*\"/position = \"$edge\"/" "$d/$id/theme.toml"
      HOME="$OUT/home" XDG_CONFIG_HOME="$OUT/home/.config" DECEBAL_THEMES="$d" \
        "$DT" theme render "$id" --mode "$mode" --png "$OUT/$id-$edge-$mode.png" >/dev/null 2>&1 || echo "render failed: $id $edge $mode"
    done
  done
done
( cd "$OUT" && sha256sum *.png ) > "$OUT/SHA256SUMS"
