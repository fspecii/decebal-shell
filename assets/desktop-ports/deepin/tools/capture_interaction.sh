#!/bin/bash
# Hover, popup and keyboard evidence for the Deepin taskbar on all four edges, dark and light, on owned private displays (:700-:999).
# At most two captures at a time; each is bounded by capture_panel.sh and an outer `timeout`.
#
#   DECEBAL_BIN=... ICON_ROOT=... capture_interaction.sh OUT_DIR [SCALE W H]     (default 1 1280 720)
#
# Per case (OUT_DIR/<mode>-<edge>/): hover-start, hover-task (a pinned task), popup-clock (the calendar), kbd-1 (`decdock --focus`: ring on
# Start), kbd-2 (ring after Right, Right), final. Applets that need a service the fixture lacks (notifications, volume, network, ...) are
# absent, so they cannot be exercised here.
set -u
export BACKDROP=${BACKDROP-#6f93c4} COMPOSITOR=${COMPOSITOR-1}
OUT=${1:?out dir}; SCALE=${2:-1}; W=${3:-1280}; H=${4:-720}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$OUT" || exit 70
export HERE SCALE W H OUT
export STEPS="hover:start;shot:hover-start.png;hover:task:google-chrome;shot:hover-task.png;click:clock;shot:popup-clock.png;key:Escape;focus;shot:kbd-1.png;key:Right;key:Right;shot:kbd-2.png;key:Escape"
printf '%s\n' dark-bottom dark-top dark-left dark-right light-bottom light-top light-left light-right |
  timeout 1500 xargs -P2 -I{} bash -c 'c={}; m=${c%%-*}; e=${c##*-}; mkdir -p "$OUT/$c"
    for try in 1 2; do timeout 150 "$HERE/capture_panel.sh" "$m" "$e" "$SCALE" "$W" "$H" "$OUT/$c/final.png" >/dev/null 2>"$OUT/$c/log.txt" && exit 0; done; echo "FAILED twice: $c"'
n=$(ls "$OUT"/*/final.png 2>/dev/null | wc -l); echo "$n of 8 cases captured"
[ "$n" = 8 ]
