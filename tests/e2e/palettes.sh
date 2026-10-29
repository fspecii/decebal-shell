#!/usr/bin/env bash
# Themes/palettes separation on a private Xvfb, rdwm, HOME and session bus.
# Build decsettings, decthemectl, decbar first. MS_BIN_DIR may point at target/debug.
set -eu
OLDHOME=$HOME; export OLDHOME
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
source tests/e2e/env.sh
trap 'set +e; e2e_down' EXIT
e2e_up "${1:-:98}"
export XDG_STATE_HOME="$E2E_T/state" DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
export DECEBAL_SYSTEM_THEMES="$E2E_T/no-system-themes"
DUMP=$DECSETTINGS_DEBUG_LAYOUT
PASS=0
check() { local label=$1; shift; "$@" || { echo "FAIL: $label"; exit 1; }; PASS=$((PASS+1)); echo "ok: $label"; }
wait_dump() { for _ in $(seq 1 80); do [ -s "$DUMP" ] && return; sleep 0.1; done; cat "$E2E_T/decsettings.log"; return 1; }
start() {
  rm -f "$DUMP"
  e2e_start_app "$1"
  wait_dump
  WID=$(xdotool search --sync --class DecSettings | head -1)
}
stop() { kill "$E2E_APP"; wait "$E2E_APP" 2>/dev/null || true; sleep 0.3; }
shot() { if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; e2e_shot "$SHOTS/$1.png"; fi; }
click() {
  local coords
  coords=$(jq -r --arg id "$1" '.ctls[]|select(.id==$id)|.rect|"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)"' "$DUMP" | head -1)
  [ -n "$coords" ] || { echo "Missing control $1"; exit 1; }
  xdotool mousemove --window "$WID" $coords click 1
  sleep 0.7
}

echo '== Themes is a single design gallery'
start appearance
check 'Themes tab opens' jq -e '.tab=="theme"' "$DUMP"
check 'no competing Desktop style control' jq -e '[.ctls[]|select(.id=="th:style")]|length==0' "$DUMP"
check 'macOS, Windows and Caelestia have design cards' jq -e '[.ctls[].id] as $ids | ["macos","windows11","caelestia"] | all(. as $id | $ids | any(.==("l:tcard:apply:"+$id) or .==("l:tcard:export:"+$id)))' "$DUMP"
check 'colour variants are absent from design cards' jq -e '[.ctls[].id|select(test("^l:tcard:.*:(nord|dracula|tokyo-night|caelestia-mocha)$"))]|length==0' "$DUMP"
shot themes
stop

echo '== Named palette replaces generated colours via the UI'
# Simulate an existing wallpaper-derived colour selection, entirely in the sandbox.
cat > "$DECEBAL_CONFIG/decebal.json" <<'JSON'
{"theme":{"name":"macos","mode":"dark","accent":"#ff0000","dynamic":{"enabled":true,"source":"wallpaper","surfaces":"full","strength":1}}}
JSON
start palettes
check 'Palettes tab opens' jq -e '.tab=="palettes"' "$DUMP"
check 'colour tiles are present' jq -e '[.ctls[]|select(.id=="l:palette:nord")]|length==1' "$DUMP"
check 'design cards are absent from Palettes' jq -e '[.ctls[]|select(.id|startswith("l:tcard:"))]|length==0' "$DUMP"
shot palettes-before
click l:palette:nord
check 'click selects Nord without changing the theme' jq -e '.theme.name=="macos" and .theme.palette=="nord" and (.theme.accent==null) and (.theme.dynamic.enabled!=true)' "$DECEBAL_CONFIG/decebal.json"
shot palettes-nord
stop

echo '== Every design preserves the palette, on the real rdwm'
for design in windows11 caelestia macos; do
  start appearance
  click "l:tcard:apply:$design"
  shot "theme-$design-nord"
  stop
  check "$design keeps Nord" jq -e --arg design "$design" '.theme.name==$design and .theme.palette=="nord"' "$DECEBAL_CONFIG/decebal.json"
  check "$design uses Nord colours" test "$(decthemectl get --json | jq -r .colors.background)" = '#2e3440'
  check "$design rdwm borders use Nord" test "$(rdwmctl config | jq -r .config.colors.focused)" = '#88c0d0'
  decbar --png "$E2E_T/$design-nord.png" --width 1200 --height 800
  check "$design renders with Nord" test -s "$E2E_T/$design-nord.png"
  if [ -n "${SHOTS:-}" ]; then cp "$E2E_T/$design-nord.png" "$SHOTS/$design-nord.png"; fi
done
# Check the second source direction too: enabling generated colours releases the named palette.
decthemectl dynamic on --seed '#4488aa'
check 'generated colours replace the named palette' jq -e '.theme.palette==null and .theme.dynamic.enabled==true' "$DECEBAL_CONFIG/decebal.json"
decthemectl palette dracula
check 'CLI palette selection also turns off generated colours' jq -e '.theme.palette=="dracula" and .theme.dynamic.enabled!=true' "$DECEBAL_CONFIG/decebal.json"
echo "palettes: $PASS checks passed"
