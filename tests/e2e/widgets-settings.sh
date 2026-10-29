#!/usr/bin/env bash
# Private Xvfb + rdwm; no live desktop paths, sockets or signals are used.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
PROFILE=${PROFILE:-debug}
export MS_BIN_DIR=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
if [ "${NO_BUILD:-0}" != 1 ]; then
  args=(); [ "$PROFILE" != release ] || args+=(--release)
  (cd "$HERE"; cargo build --offline "${args[@]}" --bin decwidgets --bin decsettings --bin decthemectl)
fi
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:219}
case "$N" in :0|:1) echo 'refusing live display'; exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] || { echo 'display already occupied'; exit 2; }
trap e2e_down EXIT
e2e_up "$N"
export XDG_DATA_HOME="$E2E_T/data" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_DIRS="$E2E_T/nodata" DECEBAL_SYSTEM_THEMES="$E2E_T/nosystem"
mkdir -p "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_DATA_DIRS"
export DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
CFG="$DECEBAL_CONFIG/decebal.json"
cat > "$CFG" <<'JSON'
{"widgets":{"enabled":true,"allow_exec":false,"future":{"preserve":123},"items":[{"use":"builtin:clock-card","enabled":true,"anchor":"top-left","x":21,"y":34,"options":{"sentinel":"preserved"}}]}}
JSON
PASS=0
check() { local label=$1; shift; local i; for i in $(seq 1 80); do if "$@" >/dev/null 2>&1; then echo "  ok   $label"; PASS=$((PASS+1)); return; fi; sleep .1; done; echo "FAIL $label"; cat "$E2E_T/decwidgets.log" 2>/dev/null; exit 1; }
count() { local ids; ids=$(xdotool search --class '^decwidgets$' 2>/dev/null || true); [ "$(printf '%s\n' "$ids" | sed '/^$/d' | wc -l)" = "$1" ]; }
edit() { jq "$1" "$CFG" > "$CFG.new"; mv "$CFG.new" "$CFG"; }
click() { local xy; xy=$(jq -r --arg id "$1" '[.ctls[]|select(.id==$id)][0].rect|"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)"' "$DECSETTINGS_DEBUG_LAYOUT"); xdotool mousemove --window "$WID" $xy click 1; }
json_is() { jq -e "$1" "$CFG"; }
decwidgets daemon >"$E2E_T/decwidgets.log" 2>&1 & HOST=$!; E2E_PIDS+=($HOST)
check 'desktop instance creates one window' count 1
BEFORE=$(jq -cS .widgets "$CFG")
decwidgets daemon >"$E2E_T/duplicate.log" 2>&1
check 'duplicate launch preserves original daemon' kill -0 "$HOST"
check 'duplicate launch does not duplicate windows' count 1
edit '.widgets.enabled=false'
check 'master off destroys desktop windows' count 0
check 'hidden host stays alive' kill -0 "$HOST"
edit '.widgets.enabled=true'
check 'master on recreates desktop windows' count 1
check 'master toggle preserves exact widget configuration' test "$BEFORE" = "$(jq -cS .widgets "$CFG")"
e2e_start_app widgets
WID=$(xdotool search --class DecSettings | head -1)
check 'Widgets settings pane opens' jq -e '.pane=="widgets"' "$DECSETTINGS_DEBUG_LAYOUT"
click wg:enabled
check 'UI master toggle persists false' json_is '.widgets.enabled==false'
check 'UI master toggle destroys windows' count 0
click wg:enabled
check 'UI master toggle persists true' json_is '.widgets.enabled==true'
check 'UI master toggle recreates windows' count 1
click wg:item:0:enabled
check 'UI individual toggle preserves disabled instance' json_is '.widgets.items|length==1 and .[0].enabled==false and .[0].options.sentinel=="preserved"'
check 'individual toggle destroys its window' count 0
check 'catalog exposes saved disabled widget action' jq -e 'any(.ctls[];.id=="wg:add:builtin.clock-card")' "$DECSETTINGS_DEBUG_LAYOUT"
click wg:add:builtin.clock-card
check 'catalog re-enables saved instance without duplicate' json_is '.widgets.items|length==1 and .[0].enabled==true and .[0].x==21'
check 'catalog enabled widget appears' count 1
BEFORE=$(jq -cS .widgets "$CFG")
for theme in gruvbox-dark everforest; do
 decthemectl theme install "$HERE/themes/$theme" >"$E2E_T/install-$theme.log" 2>&1
 decthemectl theme apply "$theme" >"$E2E_T/apply-$theme.log" 2>&1
 check "$theme preserves exact widgets JSON value" test "$BEFORE" = "$(jq -cS .widgets "$CFG")"
 check "$theme retains desktop widget" count 1
done
check 'host survives complete settings/theme lifecycle' kill -0 "$HOST"
echo "passed $PASS, failed 0"
