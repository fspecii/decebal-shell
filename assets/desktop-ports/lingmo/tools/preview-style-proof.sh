#!/bin/sh
# Shows which window style the native preview hands to its decoration helper in each mode, with no window manager involved.
# usage: preview-style-proof.sh DECTHEMECTL WORK_DIR        (WORK_DIR must not exist; private HOME/XDG/config; nothing live is touched)
# The helper is a recording stand-in (DECEBAL_DECO_RENDERER): it logs the `[decoration] style` of the rdwm config it is given and writes a 1x1 PNG.
set -eu
DT=$1; W=$2
HERE=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$HERE/../../../.." && pwd)
[ ! -e "$W" ] || { echo "refusing to reuse existing work directory: $W" >&2; exit 2; }
mkdir -p "$W/home" "$W/cfg" "$W/tmp"
cat > "$W/helper.sh" <<'HELPER'
#!/bin/sh
# usage: helper theme <rdwm config> <out.png>
style=$(grep -E '^style *=' "$2" | head -n 1)
echo "$style" >> "$DECO_PROOF_LOG"
magick -size 1x1 xc:black "$3"
HELPER
chmod 755 "$W/helper.sh"
for mode in dark light; do
  : > "$W/log-$mode"
  env -i PATH="$PATH" HOME="$W/home-$mode" XDG_CONFIG_HOME="$W/home-$mode/.config" XDG_CACHE_HOME="$W/home-$mode/.cache" XDG_DATA_HOME="$W/home-$mode/.local/share" TMPDIR="$W/tmp" \
    DECEBAL_THEMES="$ROOT/themes" DECEBAL_CONFIG="$W/cfg-$mode" DECEBAL_DECO_RENDERER="$W/helper.sh" DECO_PROOF_LOG="$W/log-$mode" \
    "$DT" theme render lingmo --mode "$mode" --png "$W/preview-$mode.png" >/dev/null
  echo "preview mode=$mode -> helper was given: $(cat "$W/log-$mode")"
done
