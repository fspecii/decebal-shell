#!/bin/bash
# The private capture matrix of docs/desktop-ports/deepin/evidence: at most two captures at a time, each one bounded by
# capture_panel.sh and an outer `timeout`, the whole run bounded too. Never touches a live display or config.
#
#   DECEBAL_BIN=... ICON_ROOT=... capture_matrix.sh OUT_DIR
#
# Cases (mode x edge): 100 % on 1280x720, 125 % on 1600x900, 200 % on 2560x1440 (1280x720 logical), and short windows with every
# applet on (1280x480 at 100 %, 800x480 at 100 %, 1280x720 at 200 % = 640x360 logical).
set -u
# translucency needs something behind the dock: a flat backdrop and a private compositor (override with BACKDROP= / COMPOSITOR=0)
export BACKDROP=${BACKDROP-#6f93c4} COMPOSITOR=${COMPOSITOR-1}
OUT=${1:?out dir}; mkdir -p "$OUT" || exit 70
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
WORK_ROOT=${CAPTURE_TMP:-/var/tmp/decebal-deepin}
mkdir -p "$WORK_ROOT" || exit 70
LIST=$(mktemp "$WORK_ROOT/matrix.XXXXXX") || exit 70
# the case list is the one file this script creates; remove only it, and only inside the owned work root
trap '[ "$(dirname "$(realpath "$LIST")")" = "$(realpath "$WORK_ROOT")" ] && rm -f -- "$LIST"' EXIT
while read -r name scale w h extra; do
  for m in dark light; do for e in bottom top left right; do
    # no trailing blank: xargs -L would join the next line to a line that ends in one
    echo "$m $e $scale $w $h $OUT/$name-$m-$e.png${extra:+ $extra}"
  done; done
done >"$LIST" <<'CASES'
d100 1 1280 720
d125 1.25 1600 900
d200 2 2560 1440
short-1280x480-100-all 1 1280 480 all-applets
short-800x480-100-all 1 800 480 all-applets
short-640x360-200-all 2 1280 720 all-applets
CASES
total=$(wc -l <"$LIST"); echo "$total captures"
export HERE
timeout 1500 xargs -P2 -L1 bash -c 'for try in 1 2; do timeout 150 "$HERE/capture_panel.sh" "$@" >/dev/null 2>"${6%.png}.log" && exit 0; done; echo "FAILED twice: $*"' _ <"$LIST"
ok=$(ls "$OUT"/*.png 2>/dev/null | wc -l); echo "$ok of $total captured"
[ "$ok" = "$total" ]
