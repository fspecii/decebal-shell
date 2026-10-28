#!/usr/bin/env bash
# End-to-end test of the Configuration pane of macsettings on a private Xvfb with a private rdwm (x11 backend).
# Types into the text editor, saves, saves a preset, exports it, then imports it into a SECOND private HOME
# (own XDG dirs, own macsettings instance) and checks the import preview, the safety checks and the CLI.
# Requires: Xvfb xdotool jq python3 import(optional, screenshots). Build first: cargo build (macsettings, macthemectl).
# Usage: tests/e2e/configpane.sh [display]   (default :91; screenshots go to $SHOTS if set)
# Never touches the live desktop: private display, private HOME / XDG_* / MACSHELL_CONFIG, rdwm is only run, not modified.
set -u
OLDHOME=$HOME; export OLDHOME
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
HERE=$PWD
export MS_BIN_DIR=${MS_BIN_DIR:-${CARGO_TARGET_DIR:-$HERE/target}/${PROFILE:-debug}}
source tests/e2e/env.sh
DISP=${1:-:91}
[ "$DISP" = ":1" ] && { echo "refusing the live display"; exit 2; }
[ -e "/tmp/.X11-unix/X${DISP#:}" ] && { echo "display $DISP is in use"; exit 2; }
E2E_SCREEN=1600x900x24
trap e2e_down EXIT
trap 'e2e_down; exit 1' INT TERM
e2e_up "$DISP"
DUMP="$E2E_T/dump.json"
export MACSETTINGS_DEBUG_LAYOUT="$DUMP"
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok   $*"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL $*"; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
q() { jq -r "$1" "$DUMP" 2>/dev/null; }
wait_for() { local t=${3:-4} i=0; while [ $i -lt $((t*10)) ]; do [ "$(q "$2")" = "true" ] && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (waited ${t}s; notice: $(q .notice))"; return 1; }
wait_cond() { local t=${3:-4} i=0; while [ $i -lt $((t*10)) ]; do eval "$2" && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (notice: $(q .notice))"; return 1; }
center() { jq -r "[.ctls[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"; }
click_xy() { xdotool mousemove --window "$WID" "$1" "$2"; sleep 0.05; xdotool click --window "$WID" 1; sleep 0.3; }
# clicks a control, wheeling the page until it is on screen
click_id() {
  local c n=0 y
  c=$(center "$1"); { [ -z "$c" ] || [ "$c" = "null null" ]; } && { bad "no control $1"; return 1; }
  y=${c#* }
  while [ "$y" -gt 800 ] && [ $n -lt 25 ]; do
    xdotool mousemove --window "$WID" 1450 400; xdotool click --window "$WID" 5; sleep 0.15; n=$((n+1))
    c=$(center "$1"); y=${c#* }
  done
  click_xy $c
}
click_part() { local r; r=$(jq -r "[.ctls[]|select(.id==\"$1\")][0].parts[$2]|\"\(.[0]+.[2]*0.5|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"); click_xy $r; }
has_ctl() { [ "$(q "[.ctls[]|select(.id==\"$1\")]|length")" != "0" ]; }
shot() { [ -n "${SHOTS:-}" ] && { mkdir -p "$SHOTS"; e2e_shot "$SHOTS/$1.png"; }; }
# types text; xdotool type drops newlines for this app, so Return is sent as a key
typ() {
  local s=$1 line
  while :; do
    line=${s%%$'\n'*}
    [ -n "$line" ] && xdotool type --window "$WID" --delay 25 -- "$line"
    if [[ $s == *$'\n'* ]]; then xdotool key --window "$WID" Return; s=${s#*$'\n'}; else break; fi
  done
  sleep 0.3
}
key() { xdotool key --window "$WID" "$@"; sleep 0.3; }
settext() { click_id "$1"; key ctrl+u; typ "$2"; key Return; }

SHELL_JSON="$MACSHELL_CONFIG/macshell.json"
PRESETS="$XDG_CONFIG_HOME/macshell/presets"
printf '{\n  "workspaces": "auto",\n  "theme": {"name": "nord", "mode": "dark"},\n  "hot_corners": {"top_left": "dashboard"}\n}\n' > "$SHELL_JSON"
printf '{"appSpacing": 9, "style": "mac"}\n' > "$MACSHELL_CONFIG/dock-settings.json"

echo "== start on the Configuration pane"
e2e_start_app config
WID=$(xdotool search --sync --class MacSettings 2>/dev/null | head -1)
check "window found" test -n "$WID"
wait_for "dump written, pane config" '.pane=="config"' 5
wait_for "the editor is on screen" '[.ctls[]|select(.id=="cfg:editor")]|length==1'
shot cfg-1-editor

echo "== typing, saving, validation"
click_id cfg:editor
key ctrl+a
typ $'{\n"workspaces": "off",\n"clockHour12": true\n}'
shot cfg-2-typed
key ctrl+s
wait_cond "valid text was written" "grep -q clockHour12 '$SHELL_JSON'"
check "the old file is kept as a timestamped backup" bash -c "ls '$MACSHELL_CONFIG'/macshell.json.bak.* >/dev/null 2>&1 && grep -q nord '$MACSHELL_CONFIG'/macshell.json.bak.*"
wait_for "the toast says Saved" '.notice=="Saved"' 3
click_id cfg:editor
key ctrl+End
typ $',\n"broken":'
key ctrl+s
sleep 0.4
check "an invalid file is never written" bash -c "! grep -q broken '$SHELL_JSON'"
wait_for "the failure is reported" '(.notice // "") | test("Not saved")' 3
shot cfg-3-error
click_id cfg:revert
wait_cond "revert restores the previous version" "grep -q nord '$SHELL_JSON'"
shot cfg-4-reverted

echo "== another tab and the overview"
click_part cfg:tab 1
sleep 0.5
shot cfg-5-dock
click_part cfg:tab 4
sleep 0.6
shot cfg-6-all

echo "== presets: save, export"
click_part cfg:view 1
wait_for "presets view shows" '[.ctls[]|select(.id=="cfg:new")]|length==1' 3
shot cfg-7-presets
click_id cfg:new
sleep 0.5
settext cfg:form-name "E2E Cosy"
settext cfg:form-desc "made in the end to end test"
shot cfg-8-save-form
click_id cfg:form-save
wait_cond "the preset folder exists" "test -f '$PRESETS/e2e-cosy/manifest.toml'"
check "it holds the shell and dock files" bash -c "test -f '$PRESETS/e2e-cosy/macshell.json' && test -f '$PRESETS/e2e-cosy/dock-settings.json'"
check "the manifest names the parts" grep -q 'parts = \["shell"' "$PRESETS/e2e-cosy/manifest.toml"
sleep 0.4
shot cfg-9-saved
OUT="$E2E_T/share"; mkdir -p "$OUT"
settext cfg:export-path "$OUT/"
click_id cfg:export
wait_cond "the bundle was exported" "test -s '$OUT/e2e-cosy.macshell-preset'"
check "it is a gzip tar with the manifest first" bash -c "tar tzf '$OUT/e2e-cosy.macshell-preset' | head -1 | grep -qx manifest.toml"

echo "== apply with a preview and undo"
printf '{"appSpacing": 40, "style": "windows"}\n' > "$MACSHELL_CONFIG/dock-settings.json"
sleep 1
click_id "cfg:apply:builtin/caelestia" || true
wait_for "the apply preview shows" '[.ctls[]|select(.id=="cfg:apply-do")]|length==1' 3
shot cfg-10-apply-preview
click_id cfg:apply-cancel
check "cancel changed nothing" grep -q windows "$MACSHELL_CONFIG/dock-settings.json"
click_id "cfg:apply:builtin/caelestia"
click_id cfg:apply-do
wait_cond "the built-in preset was applied" "grep -q caelestia '$SHELL_JSON'"
check "the dock style was reset to mac" grep -q '"style": "mac"' "$MACSHELL_CONFIG/dock-settings.json"
check "the restore point exists" test -f "$PRESETS/before-caelestia/manifest.toml"
shot cfg-11-applied
click_id cfg:undo
wait_cond "undo restores the earlier files" "grep -q windows '$MACSHELL_CONFIG/dock-settings.json'"
kill "$E2E_APP" 2>/dev/null; sleep 0.5

echo "== a second private HOME imports the bundle"
T2=$(mktemp -d /tmp/macsettings-e2e2.XXXXXX)
mkdir -p "$T2/run" "$T2/home" "$T2/cfg/rdwm" "$T2/cfg/omarchy"; chmod 700 "$T2/run"
printf '[general]\nborder_px = 1\n' > "$T2/cfg/rdwm/config.toml"
printf '{"workspaces": "off"}\n' > "$T2/cfg/omarchy/macshell.json"
env2() { env HOME="$T2/home" XDG_CONFIG_HOME="$T2/cfg" XDG_RUNTIME_DIR="$T2/run" MACSHELL_CONFIG="$T2/cfg/omarchy" MACSETTINGS_CONFIG="$T2/cfg/rdwm/config.toml" RDWM_CONFIG="$T2/cfg/rdwm/config.toml" "$@"; }
DUMP2="$T2/dump.json"
env2 MACSETTINGS_DEBUG_LAYOUT="$DUMP2" macsettings --config "$T2/cfg/rdwm/config.toml" config >"$T2/app.log" 2>&1 &
APP2=$!; E2E_PIDS+=($APP2)
sleep 3
WID=$(xdotool search --sync --class MacSettings 2>/dev/null | head -1)
DUMP="$DUMP2"
check "second instance is up" test -n "$WID"
click_part cfg:view 1
click_id cfg:import
sleep 0.4
settext cfg:import-path "$OUT/e2e-cosy.macshell-preset"
click_id cfg:import-read
wait_for "the import preview shows" '[.ctls[]|select(.id=="cfg:import-do")]|length==1' 3
shot cfg-12-import-preview
click_id cfg:import-do
P2="$T2/cfg/macshell/presets"
wait_cond "the imported preset is in the second HOME" "test -f '$P2/e2e-cosy/manifest.toml'"
check "it is marked imported" grep -q 'origin = "imported"' "$P2/e2e-cosy/manifest.toml"
check "the first HOME is untouched by the second" test ! -e "$T2/cfg/omarchy/dock-settings.json"
sleep 0.5
shot cfg-13-imported
click_id "cfg:apply:e2e-cosy"
wait_for "apply preview for the imported preset" '[.ctls[]|select(.id=="cfg:apply-do")]|length==1' 3
shot cfg-14-apply-imported
click_id cfg:apply-do
wait_cond "applied in the second HOME" "grep -q nord '$T2/cfg/omarchy/macshell.json'"
kill $APP2 2>/dev/null; sleep 0.3

echo "== hostile bundles are refused by the CLI"
EVIL="$T2/evil"; mkdir -p "$EVIL"
python3 -I - "$EVIL" <<'PY'
import io, sys, tarfile, time
out = sys.argv[1]
man = b'format = 1\nname = "evil"\nparts = ["shell"]\n'
def build(name, entries):
    with tarfile.open(f"{out}/{name}.macshell-preset", "w:gz") as t:
        for n, data, kind in entries:
            ti = tarfile.TarInfo(n); ti.mtime = 0
            if kind == "link":
                ti.type = tarfile.SYMTYPE; ti.linkname = "/etc/passwd"; t.addfile(ti)
            else:
                ti.size = len(data); t.addfile(ti, io.BytesIO(data))
build("traversal", [("manifest.toml", man, "f"), ("../../evil.json", b"{}", "f")])
build("symlink", [("manifest.toml", man, "f"), ("macshell.json", b"", "link")])
build("unknown", [("manifest.toml", man, "f"), ("run.sh", b"echo hi", "f")])
build("huge", [("manifest.toml", man, "f"), ("macshell.json", b"{" + b" " * (2 * 1024 * 1024) + b"}", "f")])
build("commands", [("manifest.toml", b'format = 1\nname = "cmds"\nparts = ["autostart"]\n', "f"), ("rdwm.toml", b'[[autostart]]\ncommand = "touch /tmp/pwned-by-e2e"\n', "f")])
PY
for n in traversal symlink unknown huge; do
  out=$(env2 macthemectl profile import "$EVIL/$n.macshell-preset" 2>&1); rc=$?
  check "import of the $n bundle is refused ($(echo "$out" | head -1 | cut -c1-70))" test $rc -ne 0
done
check "nothing was extracted outside" test ! -e "$T2/evil.json" -a ! -e "$T2/../evil.json"
out=$(env2 macthemectl profile import "$EVIL/commands.macshell-preset" 2>&1); rc=$?
check "a preset with commands is refused without a decision" test $rc -ne 0
check "...and lists the command" bash -c "echo '$out' | grep -q 'touch /tmp/pwned-by-e2e'"
env2 macthemectl profile import "$EVIL/commands.macshell-preset" --strip-commands >/dev/null 2>&1
check "--strip-commands imports it without the command" bash -c "! grep -rq pwned '$P2/' 2>/dev/null"
check "the command never ran" test ! -e /tmp/pwned-by-e2e
check "macthemectl profile list shows built-ins and ours" bash -c "env HOME='$T2/home' XDG_CONFIG_HOME='$T2/cfg' macthemectl profile list | grep -q 'Minimal tiling' && env HOME='$T2/home' XDG_CONFIG_HOME='$T2/cfg' macthemectl profile list | grep -q 'E2E Cosy'"

echo
echo "passed $PASS, failed $FAIL"
rm -rf "$T2"
[ "$FAIL" -eq 0 ]
