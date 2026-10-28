#!/usr/bin/env bash
# Scroll benchmark for decsettings on a private Xvfb: N wheel notches over the content of the long panes, then the number of
# frames painted, the mean paint time and the CPU time decsettings used (from the layout dump's counters and /proc).
#   tests/e2e/scroll-perf.sh [display]      (N=300 notches by default; BENCH_N, BENCH_PANES override)
# Fails when the mean paint time of a pane exceeds BENCH_MAX_MS (default 25 ms) so a regression in the paint path is caught.
set -u
OLDHOME=$HOME; export OLDHOME
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
source tests/e2e/env.sh
trap e2e_down EXIT
e2e_up "${1:-:96}"
DUMP="$E2E_T/dump.json"; export DECSETTINGS_DEBUG_LAYOUT="$DUMP"
N=${BENCH_N:-320}; MAXMS=${BENCH_MAX_MS:-25}; PANES=${BENCH_PANES:-"theme appearance/colours appearance/window icons wallpaper desktop config"}
PASS=0; FAIL=0
for pane in $PANES; do
  decsettings --config "$E2E_CFG" "$pane" >"$E2E_T/ds.log" 2>&1 & pid=$!; E2E_PIDS+=($pid)
  sleep 3
  WID=$(xdotool search --sync --class DecSettings 2>/dev/null | head -1)
  read -r gw gh <<<"$(xdotool getwindowgeometry "$WID" | sed -n 's/.*Geometry: \([0-9]*\)x\([0-9]*\).*/\1 \2/p')"
  xdotool mousemove --window "$WID" $((gw * 2 / 3)) $((gh / 2)); sleep 0.3
  f0=$(jq .frames "$DUMP"); p0=$(jq .paint_us "$DUMP"); i0=$(jq .items_us "$DUMP")
  c0=$(awk '{print $14+$15}' /proc/$pid/stat); t0=$(date +%s%N)
  # down 40, up 40, ... so the long panes keep scrolling instead of sitting at their end
  for _ in $(seq 1 $((N / 80 + 1))); do
    xdotool click --window "$WID" --repeat 40 --delay 25 5; xdotool click --window "$WID" --repeat 40 --delay 25 4
  done
  # wait until the app has settled (no new frame for 0.4 s)
  last=-1; while :; do cur=$(jq .frames "$DUMP"); [ "$cur" = "$last" ] && break; last=$cur; sleep 0.4; done
  t1=$(date +%s%N); c1=$(awk '{print $14+$15}' /proc/$pid/stat)
  f1=$(jq .frames "$DUMP"); p1=$(jq .paint_us "$DUMP"); i1=$(jq .items_us "$DUMP")
  fr=$((f1 - f0)); [ $fr -lt 1 ] && fr=1
  mean=$(( (p1 - p0) / fr )); imean=$(( (i1 - i0) / fr ))
  rss=$(awk '/VmRSS/{print $2}' /proc/$pid/status)
  echo "$pane: wheel=$((N / 80 * 80 + 80)) frames=$fr mean_paint=${mean}us (items ${imean}us) cpu=$(( (c1 - c0) * 10 ))ms wall=$(( (t1 - t0) / 1000000 ))ms rss=${rss}kB"
  if [ $((mean / 1000)) -gt "$MAXMS" ]; then echo "  FAIL $pane mean paint ${mean}us > ${MAXMS}ms"; FAIL=$((FAIL+1)); else PASS=$((PASS+1)); fi
  kill $pid; wait $pid 2>/dev/null; sleep 0.3
done
echo "scroll-perf: $PASS ok, $FAIL failed"
[ $FAIL -eq 0 ]
