#!/usr/bin/env bash
# Native Settings choice flow on a disposable X11 desktop.
set -eu
export DECEBAL_SYSTEM_AUTOMATIC=0
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
PROFILE=${PROFILE:-release}
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/$PROFILE}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:223}
case "$N" in :0|:1) exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] || exit 2
trap e2e_down EXIT
e2e_up "$N"
export XDG_DATA_HOME="$E2E_T/data" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_DIRS="$E2E_T/nodata" DECEBAL_SYSTEM_THEMES="$E2E_T/nosystem"
mkdir -p "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_DATA_DIRS"
export DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
CFG="$DECEBAL_CONFIG/decebal.json"
cat > "$CFG" <<'JSON'
{"widgets":{"enabled":true,"allow_exec":false,"items":[{"use":"builtin:clock-card","x":21,"options":{"sentinel":"keep"}}]}}
JSON
decthemectl theme install "$HERE/themes/nothing-light" --force >"$E2E_T/install.log" 2>&1
PASS=0
check() { local label=$1; shift; for i in $(seq 1 80); do if "$@" >/dev/null 2>&1; then echo "  ok   $label"; PASS=$((PASS+1)); return; fi; sleep .1; done; echo "FAIL $label"; cat "$E2E_T/decsettings.log" "$CFG"; jq '[.ctls[].id]' "$DECSETTINGS_DEBUG_LAYOUT"; exit 1; }
control() { jq -e --arg id "$1" 'any(.ctls[];.id==$id)' "$DECSETTINGS_DEBUG_LAYOUT"; }
click() { local xy; xy=$(jq -r --arg id "$1" '[.ctls[]|select(.id==$id)][0].rect|"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)"' "$DECSETTINGS_DEBUG_LAYOUT"); xdotool mousemove --window "$WID" $xy click 1; sleep .25; }
json_is() { jq -e "$1" "$CFG"; }
e2e_start_app theme
WID=$(rdwmctl state | jq -r '.state.windows[]|select(.class=="DecSettings")|.id' | head -1)
xdotool windowactivate --sync "$WID"
check 'installed theme Apply button is visible' control l:tcard:apply:nothing-light
BEFORE=$(cat "$CFG")
click l:tcard:apply:nothing-light
check 'custom layout raises replacement question' control wc:replace
check 'question leaves config untouched' test "$BEFORE" = "$(cat "$CFG")"
click wc:cancel
check 'cancel leaves config untouched' test "$BEFORE" = "$(cat "$CFG")"
click l:tcard:apply:nothing-light
check 'question can be reopened' control wc:keep
click wc:keep
check 'Keep applies theme and preserves custom positions' json_is '.theme.name=="nothing-light" and .widgets.items[0].x==21 and .widgets.items[0].options.sentinel=="keep"'
decthemectl theme apply nord >"$E2E_T/switch-before-replace.log" 2>&1
check 'theme Apply becomes available after switching away' control l:tcard:apply:nothing-light
click l:tcard:apply:nothing-light
check 'selecting again still asks before replacing custom layout' control wc:replace
sleep 1
if [ -n "${WIDGET_CHOICE_SHOT:-}" ]; then import -window "$WID" "$WIDGET_CHOICE_SHOT"; fi
click wc:replace
check 'Replace applies theme widget layout' json_is '.widgets.enabled==true and (.widgets.items|length)==3 and .widgets.theme_baseline.items==.widgets.items and .widgets.allow_exec==false'
# An untouched theme layout switches without asking; no-widget themes hide it.
decthemectl theme apply nord >"$E2E_T/no-widgets.log" 2>&1
check 'widgetless theme hides but retains saved collection' json_is '.widgets.enabled==false and (.widgets.items|length)==3'
cp -r "$HERE/themes/nothing-light" "$E2E_T/widgets-off"
python3 - "$E2E_T/widgets-off/theme.toml" <<'PYFIX'
import sys,pathlib
p=pathlib.Path(sys.argv[1]); s=p.read_text().replace('id = "nothing-light"','id = "widgets-off"').replace('[[widgets]]','[[widgets]]\nenabled = false'); p.write_text(s)
PYFIX
decthemectl theme install "$E2E_T/widgets-off" >"$E2E_T/install-off.log" 2>&1
jq '.widgets.items[0].x=77 | .widgets.enabled=true' "$CFG" >"$CFG.new"; mv "$CFG.new" "$CFG"
decthemectl theme apply widgets-off >"$E2E_T/apply-off.log" 2>&1
check 'all-disabled theme hides custom widgets without replacement prompt' json_is '.widgets.enabled==false and .widgets.items[0].x==77'
echo "passed $PASS, failed 0"
