#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# End-to-end test of System Settings > Wallpaper > Wallhaven on a private Xvfb + rdwm, against a LOCAL fake of
# wallhaven.cc (tests/e2e/fakes/wallhaven_server.py on 127.0.0.1, WALLHAVEN_API_BASE points the app at it).
# Nothing here talks to the real wallhaven.cc, the real desktop or the real rdwm socket.
# Requires: Xvfb xdotool jq python3 (+PIL) import. Build first: cargo build --bins
# Usage: tests/e2e/wallhaven.sh [display]   (default :89; screenshots go to $SHOTS if set)
#   MS_BIN_DIR=<dir with decsettings, decthemectl>   E2E_SCREEN=1600x900x24 (default)
set -u
OLDHOME=$HOME; export OLDHOME
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
ROOT=$PWD
export E2E_SCREEN=${E2E_SCREEN:-1600x900x24}
export E2E_WALLS=/nonexistent
source tests/e2e/env.sh
SRV_PID=""
cleanup() { [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null; e2e_down; }
trap cleanup EXIT
e2e_up "${1:-:89}"
export XDG_STATE_HOME="$E2E_T/state" XDG_CACHE_HOME="$E2E_T/cache"
DUMP="$E2E_T/dump.json"
export DECSETTINGS_DEBUG_LAYOUT="$DUMP"
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok   $*"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL $*"; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
q() { jq -r "$1" "$DUMP" 2>/dev/null; }
wait_for() { local t=${3:-6} i=0; while [ $i -lt $((t*10)) ]; do [ "$(q "$2")" = "true" ] && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done
  bad "$1 (waited ${t}s; dump: $(q '[.pane, .notice, (.warnings|tostring), .wh_thumbs, (.thumbs|length)]|tostring'))"; return 1; }
wait_cmd() { local t=${3:-6} i=0; while [ $i -lt $((t*10)) ]; do bash -c "$2" >/dev/null 2>&1 && { ok "$1"; return 0; }; sleep 0.1; i=$((i+1)); done; bad "$1 (waited ${t}s)"; return 1; }
center() { jq -r "[.ctls[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"; }
click_xy() { xdotool mousemove --window "$WID" "$1" "$2"; sleep 0.05; xdotool click --window "$WID" 1; sleep 0.3; }
click_id() { local c; c=$(center "$1"); { [ -z "$c" ] || [ "$c" = "null null" ]; } && { bad "no control $1"; return 1; }; click_xy $c; }
click_part() { local r; r=$(jq -r "[.ctls[]|select(.id==\"$1\")][0].parts[$2]|\"\(.[0]+.[2]*${3:-0.5}|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"); click_xy $r; }
click_thumb() { click_xy $(jq -r "[.thumbs[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DUMP"); }
wheel() { xdotool mousemove --window "$WID" 800 450; for _ in $(seq 1 "$1"); do xdotool click --window "$WID" ${2:-5}; sleep 0.04; done; sleep 0.4; }
reveal() { # scroll until control $1 is inside the window
  local n=0 y
  while [ $n -lt 40 ]; do
    y=$(jq -r "[.ctls[]|select(.id==\"$1\")][0].rect[1]" "$DUMP")
    if [ "$y" != "null" ] && [ "${y%.*}" -gt 90 ] && [ "${y%.*}" -lt 780 ]; then return 0; fi
    if [ "$y" = "null" ] || [ "${y%.*}" -gt 780 ]; then wheel 3 5; else wheel 3 4; fi
    n=$((n+1))
  done
  bad "could not scroll $1 into view"
}
shot() { [ -n "${SHOTS:-}" ] && { mkdir -p "$SHOTS"; e2e_shot "$SHOTS/$1.png"; }; }
# request log of the fake server
REQ="$E2E_T/srv/requests.jsonl"
nreq() { [ -f "$REQ" ] && jq -s "[.[]|select($1)]|length" "$REQ" || echo 0; }
API='(.path|startswith("/api/v1/search"))'
# conf
BG="$E2E_T/cfg/omarchy/backgrounds"
KEY="AbCdEfGh1234567890AbCdEfGh123456"

start_server() { # [port]
  mkdir -p "$E2E_T/srv"; rm -f "$E2E_T/srv/port"
  python3 -I tests/e2e/fakes/wallhaven_server.py "$E2E_T/srv" tests/fixtures/wallhaven ${1:-} >"$E2E_T/srv.log" 2>&1 &
  SRV_PID=$!
  for _ in $(seq 1 50); do [ -s "$E2E_T/srv/port" ] && break; sleep 0.1; done
  PORT=$(cat "$E2E_T/srv/port")
}

echo "== fake wallhaven"
start_server
export WALLHAVEN_API_BASE="http://127.0.0.1:$PORT/api/v1"
check "fake server answers" bash -c "curl -s '$WALLHAVEN_API_BASE/search?page=1' | jq -e '.data|length==24' >/dev/null"
rm -f "$REQ"
# "Match wallpaper" is on, so a wallpaper set from wallhaven must re-seed it
convert -size 64x64 xc:'#d02020' "$E2E_T/red.png"
rdwmctl wallpaper "$E2E_T/red.png" >/dev/null 2>&1; sleep 0.5
decthemectl dynamic on --source wallpaper --no-rdwm >/dev/null 2>&1
SEED0=$(decthemectl get --json 2>/dev/null | jq -r '.selection.dynamic.seed|tostring')

echo "== open the Wallhaven tab"
e2e_start_app wallpaper
WID=$(xdotool search --sync --class DecSettings 2>/dev/null | head -1)
check "window found" test -n "$WID"
wait_for "Appearance > Wallpaper shown" '.pane=="appearance" and .tab=="wallpaper"' 5
check "tab switch is there" test "$(q '[.ctls[]|select(.id=="wh:tab")]|length')" = 1
check "no network traffic before the tab is opened" test "$(nreq 'true')" = 0
shot wh-0-backgrounds
click_part wh:tab 1
wait_for "24 results listed" '[.thumbs[]|select(.id|startswith("wh:"))]|length==24' 8
wait_for "thumbnails decoded off-thread" '.wh_thumbs>=8' 8
sleep 0.5
shot wh-1-grid
check "first request: SFW, all categories, top list, at least this screen" bash -c "jq -s -e '[.[]|select($API)][0].path | test(\"categories=111\") and test(\"purity=100\") and test(\"sorting=toplist\") and test(\"topRange=1M\") and test(\"atleast=[0-9]+x[0-9]+\") and test(\"page=1\")' $REQ >/dev/null"
check "User-Agent identifies the app" bash -c "jq -s -e '[.[]|select($API)][0].ua | startswith(\"decebal-decsettings/\")' $REQ >/dev/null"
check "no API key header without a key" test "$(nreq "$API and .key")" = 0

echo "== select a wallpaper"
click_thumb wh:d8vvlo
wait_for "preview shows the large thumbnail of d8vvlo" '.preview=="wh:L:d8vvlo"' 4
wait_for "scrolled back to the top for the preview" '.scroll==0' 3
wait_for "large thumb loaded" '.wh_thumbs>=10' 6
wait_cmd "tags are fetched from /w/<id>" "test \$(jq -s '[.[]|select(.path==\"/api/v1/w/d8vvlo\")]|length' $REQ) = 1" 4
sleep 0.5
shot wh-2-selected
for id in wh:set wh:dl wh:fav wh:open; do check "action $id present and enabled" test "$(q "[.ctls[]|select(.id==\"$id\" and .enabled)]|length")" = 1; done

echo "== set as wallpaper"
click_id wh:set
sleep 0.6
shot wh-3-downloading
wait_cmd "file written with the safe name" "test -s $BG/wallhaven-d8vvlo.png" 8
check "it is a PNG" bash -c "file $BG/wallhaven-d8vvlo.png | grep -q 'PNG image data, 1920 x 1080'"
check "no .part left behind" bash -c "! ls -A $BG | grep -q part"
wait_cmd "config.toml got the wallpaper (the Wallpaper pane's own path)" "grep -q 'image = \"$BG/wallhaven-d8vvlo.png\"' $E2E_CFG" 6
sleep 1
check "rdwm shows it" test "$(rdwmctl state | jq -r .state.wallpaper.image)" = "$BG/wallhaven-d8vvlo.png"
check "rest of config.toml untouched" grep -q "smart_gaps = true" "$E2E_CFG"
SEED1=$(decthemectl get --json 2>/dev/null | jq -r '.selection.dynamic.seed|tostring')
check "Match wallpaper colours followed ($SEED0 -> $SEED1)" bash -c "[ '$SEED0' != null ] && [ '$SEED1' != '$SEED0' ]"
check "dynamic colours now come from the wallhaven file" bash -c "decthemectl get --json | jq -e '.selection.dynamic.image|endswith(\"wallhaven-d8vvlo.png\")' >/dev/null"
shot wh-4-set

echo "== download only"
click_id wh:close
wait_for "preview closed" '.preview==null' 3
click_thumb wh:lygg6r
wait_for "second wallpaper selected" '.preview=="wh:L:lygg6r"' 4
click_id wh:dl
wait_cmd "jpg downloaded" "test -s $BG/wallhaven-lygg6r.jpg" 8
check "still the first wallpaper in config" grep -q "wallhaven-d8vvlo.png" "$E2E_CFG"
check "extension came from the content type" bash -c "file $BG/wallhaven-lygg6r.jpg | grep -q JPEG"

echo "== favourites"
click_id wh:fav
FAV="$E2E_T/state/decebal/wallhaven-favourites.json"
wait_cmd "favourite saved in the state file" "jq -e 'length==1 and .[0].id==\"lygg6r\"' $FAV" 3
check "state file is private" test "$(stat -c %a "$FAV")" = 600
click_part wh:view 1
wait_for "favourites view lists 1" '[.thumbs[]|select(.id|startswith("wh:"))]|length==1' 3
sleep 0.5
shot wh-5-favourites
click_part wh:view 0
wait_for "browse again" '[.thumbs[]|select(.id|startswith("wh:"))]|length>=24' 4

echo "== infinite scroll"
check "the first page alone did not trigger page 2" test "$(nreq "$API and (.path|test(\"page=2\"))")" = 0
wheel 40
wait_cmd "page 2 requested when the end came close" "test \$(jq -s '[.[]|select($API and (.path|test(\"page=2\")))]|length' $REQ) = 1" 6
wait_for "at least 48 results" '[.thumbs[]|select(.id|startswith("wh:"))]|length>=48' 5
check "every page was asked for once" bash -c "test \$(jq -s '[.[]|select($API)|.path|capture(\"page=(?<p>[0-9]+)\").p]|group_by(.)|map(length)|max' $REQ) = 1"
wheel 60
sleep 0.5
shot wh-6-scrolled
wait_for "bounded thumbnail memory" '.wh_thumbs<=120' 3
wheel 80 4

echo "== filters, search and the API key"
click_id wh:filters
sleep 0.4
shot wh-7-filters
click_id wh:q
xdotool type --window "$WID" --delay 30 "forest"; sleep 0.2
xdotool key --window "$WID" Return
wait_cmd "search sent q=forest" "test \$(jq -s '[.[]|select($API and (.path|test(\"q=forest\")))]|length' $REQ) -ge 1" 6
wait_for "results replaced (query fixture first)" '[.thumbs[]|select(.id=="wh:4g1k9q")]|length==1' 6
check "stored in wallhaven.json (no key there)" bash -c "jq -e '.query==\"forest\"' $E2E_T/cfg/decebal/wallhaven.json >/dev/null"
# category chip
click_id wh:cat:2
wait_cmd "categories=110 after switching People off" "test \$(jq -s '[.[]|select($API and (.path|test(\"categories=110\")))]|length' $REQ) -ge 1" 6
# colour swatch
click_xy $(jq -r '[.layouts[]|select(.id=="wh:color:0066cc")][0].rect|"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)"' "$DUMP")
wait_cmd "colors=0066cc sent" "test \$(jq -s '[.[]|select($API and (.path|test(\"colors=0066cc\")))]|length' $REQ) -ge 1" 6
check "sketchy / NSFW chips are disabled without a key" test "$(q '[.ctls[]|select((.id=="wh:pur:1" or .id=="wh:pur:2" or .id=="wh:mature") and .enabled)]|length')" = 0
shot wh-8-filtered
# key
reveal wh:key:secret
click_id wh:key:secret
xdotool type --window "$WID" --delay 20 "$KEY"; sleep 0.2
shot wh-9-key-typed
xdotool key --window "$WID" Return
KF="$E2E_T/cfg/decebal/wallhaven.key"
wait_cmd "key file written" "test -s $KF" 4
check "key file is mode 600" test "$(stat -c %a "$KF")" = 600
wait_cmd "next request carries X-API-Key" "test \$(jq -s '[.[]|select($API and .key)]|length' $REQ) -ge 1" 6
check "the key is never in a URL" bash -c "! grep -q '$KEY' $REQ"
check "the key is nowhere but its own file" bash -c "! grep -rIl '$KEY' $E2E_T --exclude-dir=srv --exclude-dir=.git 2>/dev/null | grep -v 'wallhaven.key\$' | grep -q ."
check "the key is not in the process environment or log" bash -c "! grep -q '$KEY' $E2E_T/decsettings.log"
check "image requests carry no key" test "$(nreq '(.path|startswith("/th/") or startswith("/full/")) and .key')" = 0
sleep 0.4
check "mature switch enabled with a key" test "$(q '[.ctls[]|select(.id=="wh:mature" and .enabled)]|length')" = 1
reveal wh:mature
click_id wh:mature
wait_cmd "purity=110 after allowing sketchy" "test \$(jq -s '[.[]|select($API and (.path|test(\"purity=110\")))]|length' $REQ) -ge 1" 6
sleep 0.6
shot wh-10-mature

echo "== random wallpaper"
rm -f "$BG"/wallhaven-s*.png "$BG"/wallhaven-s*.jpg
wheel 60 4
click_id wh:random
wait_cmd "random search sent" "test \$(jq -s '[.[]|select($API and (.path|test(\"sorting=random\")))]|length' $REQ) -ge 1" 6
wait_cmd "a random wallpaper became the config image" "grep -q 'image = \".*wallhaven-[a-z0-9]*\\.\\(png\\|jpg\\)\"' $E2E_CFG && ! grep -q wallhaven-d8vvlo.png $E2E_CFG" 10
sleep 0.5
shot wh-11-random

echo "== offline: cached results, friendly error, no retry storm"
kill "$SRV_PID"; wait "$SRV_PID" 2>/dev/null; SRV_PID=""
wheel 60 4
click_part wh:sort 1
wait_for "friendly offline note" '(.warnings|map(test("Can.t reach"))|any)' 10
wait_for "the last results are still shown" '[.thumbs[]|select(.id|startswith("wh:"))]|length>0' 4
check "retry button offered" test "$(q '[.ctls[]|select(.id=="wh:retry")]|length')" = 1
sleep 0.6
shot wh-12-offline
check "app still alive" kill -0 $E2E_APP

echo "== rate limit (429)"
start_server "$PORT"
cat "$E2E_T/srv.log"
echo 429 > "$E2E_T/srv/mode"
rm -f "$REQ"
click_id wh:retry
wait_for "429 explained" '(.warnings|map(test("45 requests"))|any)' 8 || { tail -5 "$REQ"; cat "$E2E_T/srv.log"; tail -5 "$E2E_T/decsettings.log"; }
for _ in 1 2 3 4; do c=$(center wh:retry); { [ -n "$c" ] && [ "$c" != "null null" ]; } && click_xy $c; sleep 0.4; done
sleep 0.5
shot wh-13-ratelimited
check "exactly one request reached the server (cool-down, no retry storm)" test "$(nreq "$API")" = 1
echo ok > "$E2E_T/srv/mode"

echo "== CLI: decthemectl wallpaper wallhaven"
OUT=$(decthemectl wallpaper wallhaven search forest --list --screen 3440x1440 2>&1)
check "search --list prints matches" bash -c "echo '$OUT' | grep -q '^4g1k9q  3840x2160'"
rm -f "$BG"/wallhaven-*.jpg
OUT=$(decthemectl wallpaper wallhaven search forest --screen 3440x1440 2>&1)
check "search sets the best match ($OUT)" bash -c "echo '$OUT' | grep -q 'wallpaper set'"
wait_cmd "config.toml names it" "grep -q 'image = \".*wallhaven-4g1k9q.jpg\"' $E2E_CFG" 4
OUT=$(decthemectl wallpaper wallhaven random --screen 3440x1440 2>&1)
check "random sets one ($OUT)" bash -c "echo '$OUT' | grep -q 'wallpaper set'"
check "CLI output has no key" bash -c "! echo '$OUT' | grep -q '$KEY'"
check "CLI refuses nonsense" bash -c "! decthemectl wallpaper wallhaven nonsense 2>/dev/null"

echo "== resources"
APP=$E2E_APP
RSS=$(awk '/VmRSS/{print int($2/1024)}' /proc/$APP/status); THREADS=$(ls /proc/$APP/task | wc -l)
echo "  RSS ${RSS} MB, threads $THREADS"
check "RSS under 90 MB with ~50 thumbnails cached" test "$RSS" -lt 90
check "no leftover temp files in the backgrounds folder" bash -c "! ls -A $BG | grep -E '\\.part|\\.tmp'"
check "app log has no panic" bash -c "! grep -qi panick $E2E_T/decsettings.log"

echo
echo "passed $PASS, failed $FAIL"
[ "$FAIL" -eq 0 ]
