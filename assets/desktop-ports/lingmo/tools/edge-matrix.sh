#!/bin/sh
# Renders the Lingmo package at every panel edge, light and dark, with the real native preview painter (decthemectl theme render).
# usage: edge-matrix.sh DECTHEMECTL OUT_DIR   -- private copies and a private HOME only; no live config, display or process is touched.
set -eu
DT=$1; OUT=$2; ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
[ ! -e "$OUT" ] || { echo "refusing to reuse existing output directory: $OUT" >&2; exit 2; }   # fresh directories only, nothing is deleted
mkdir -p "$OUT/home"
for edge in top bottom left right; do
  for mode in dark light; do
    d="$OUT/themes-$edge-$mode"; mkdir -p "$d"; cp -r "$ROOT/themes/lingmo" "$d/lingmo"
    # bar.position and dock.position are the only panel-edge keys of format 1; the bar and the dock share the edge here
    awk -v e="$edge" '/^\[/ {sec=$0} sec=="[bar]" || sec=="[dock]" { if ($0 ~ /^position = /) $0 = "position = \"" e "\"" } {print}' "$ROOT/themes/lingmo/theme.toml" > "$d/lingmo/theme.toml"
    env -i PATH="$PATH" HOME="$OUT/home" XDG_CONFIG_HOME="$OUT/home/.config" XDG_CACHE_HOME="$OUT/home/.cache" DECEBAL_THEMES="$d" \
      "$DT" theme render lingmo --mode "$mode" --png "$OUT/lingmo-$edge-$mode.png" >/dev/null || echo "render failed: $edge $mode"
  done
done
( cd "$OUT" && sha256sum *.png ) > "$OUT/SHA256SUMS"
