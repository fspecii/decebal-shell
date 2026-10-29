#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# Proves visible desktop pixels and click/lifecycle behavior with real rdwm.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
PROFILE=${PROFILE:-debug};export MS_BIN_DIR=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
if [ "${NO_BUILD:-0}" != 1 ]; then
 args=(); [ "$PROFILE" != release ] || args+=(--release)
 (cd "$HERE"; CARGO_BUILD_JOBS=2 cargo build --offline "${args[@]}" --bin decwidgets --bin decsettings --bin decthemectl --example fakewin)
fi
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:239};case "$N" in :0|:1) exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] || { echo 'display occupied'; exit 2; }
cleanup() { local result=$?; set +e; if [ -f "${XDG_RUNTIME_DIR:-/nonexistent}/decwidgets.lock" ]; then local p; p=$(cat "$XDG_RUNTIME_DIR/decwidgets.lock"); [ -z "$p" ] || kill "$p" 2>/dev/null || true; fi; if [ "$result" != 0 ] || [ "${KEEP:-0}" = 1 ]; then for p in "${E2E_PIDS[@]}"; do kill "$p" 2>/dev/null; done; echo "artifacts: $E2E_T"; else e2e_down; fi; }
trap cleanup EXIT
e2e_up "$N"
export XDG_DATA_HOME="$E2E_T/data" XDG_STATE_HOME="$E2E_T/state" XDG_DATA_DIRS="$E2E_T/nodata" DECEBAL_SYSTEM_THEMES="$E2E_T/nosystem"
mkdir -p "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_DATA_DIRS" "$XDG_CONFIG_HOME/decebal/widgets"
export DECSETTINGS_DEBUG_LAYOUT="$E2E_T/layout.json"
CFG="$DECEBAL_CONFIG/decebal.json";printf '{"widgets":{"enabled":false,"items":[]}}\n' > "$CFG"
PASS=0
check() { local label=$1; shift; for _ in $(seq 1 100); do if "$@" >/dev/null 2>&1; then echo "  ok   $label"; PASS=$((PASS+1)); return; fi;sleep .1;done;echo "FAIL $label"; import -window root -crop 1x1+310+170 +repage -depth 8 txt:- 2>/dev/null | tail -1; cat "$E2E_T/decwidgets.log" 2>/dev/null || true;exit 1; }
count() { local ids;ids=$(xdotool search --class '^decwidgets$' 2>/dev/null || true);[ "$(printf '%s\n' "$ids"|sed '/^$/d'|wc -l)" = "$1" ]; }
click_id() { local xy;xy=$(jq -r --arg id "$1" '[.ctls[]|select(.id==$id)][0].rect|"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)"' "$DECSETTINGS_DEBUG_LAYOUT");xdotool mousemove --window "$WID" $xy click 1; }
magick -size 1600x1000 xc:'#13b54a' "$E2E_T/green.png"
magick -size 1600x1000 xc:'#235ac9' "$E2E_T/blue.png"
rdwmctl wallpaper "$E2E_T/green.png" --mode fill >/dev/null
check 'rdwm publishes real wallpaper pixmap' bash -c 'xprop -root _XROOTPMAP_ID | rg -q "pixmap id"'
e2e_start_app widgets
WID=$(xdotool search --class DecSettings | head -1)
check 'fresh Widgets pane exposes Add' jq -e 'any(.ctls[];.id=="wg:add:builtin.next-clock")' "$DECSETTINGS_DEBUG_LAYOUT"
click_id wg:add:builtin.next-clock
check 'Add turns on hidden master and saves first instance' jq -e '.widgets.enabled==true and (.widgets.items|length)==1' "$CFG"
check 'Add starts real host without prelaunch' count 1
check 'host reports ready after successful render' bash -c 'decwidgets status --json | jq -e .running'
kill "$E2E_APP";wait "$E2E_APP" 2>/dev/null || true
# A diagnostic user card has exact pixels and an allowlisted workspace click.
cat > "$XDG_CONFIG_HOME/decebal/widgets/visible-test.toml" <<'TOML'
format=1
id="visible-test"
name="Visible test"
size=[180,100]
[root]
type="text"
text="Clickable"
size=20
color="#ffffff"
background="#ee2233"
on_click="workspace 2"
TOML
decwidgets add visible-test --anchor top-left --x 80 --y 160 >"$E2E_T/add.log" 2>&1
check 'CLI Add creates second widget' count 2
pixel_is() { local got;got=$(import -window root -crop 1x1+85+165 +repage -depth 8 txt:- 2>/dev/null|tail -1); printf '%s' "$got" | rg -qi '#EE2233'; }
check 'actual red widget pixels appear above rdwm wallpaper' pixel_is
xdotool mousemove 100 190 click 1
check 'visible widget receives real click and changes workspace' bash -c 'xprop -root _NET_CURRENT_DESKTOP | rg -q "= 1$"'
check 'widget remains visible across workspace switch' pixel_is
# A third-party desktop window must remain below cards; window counts alone miss this.
cat > "$E2E_T/desktop.py" <<'PYDESKTOP'
import ctypes as C,time
x=C.CDLL('libX11.so.6');P=C.c_void_p;U=C.c_ulong
x.XOpenDisplay.restype=P;d=x.XOpenDisplay(None)
x.XDefaultRootWindow.argtypes=[P];x.XDefaultRootWindow.restype=U
x.XCreateSimpleWindow.argtypes=[P,U,C.c_int,C.c_int,C.c_uint,C.c_uint,C.c_uint,U,U];x.XCreateSimpleWindow.restype=U
x.XInternAtom.argtypes=[P,C.c_char_p,C.c_int];x.XInternAtom.restype=U
x.XChangeProperty.argtypes=[P,U,U,U,C.c_int,C.c_int,C.POINTER(C.c_ubyte),C.c_int]
x.XMapRaised.argtypes=[P,U];x.XFlush.argtypes=[P]
w=x.XCreateSimpleWindow(d,x.XDefaultRootWindow(d),0,0,1600,1000,0,0,0x13b54a)
atom=lambda s:x.XInternAtom(d,s,0)
v=(U*1)(atom(b'_NET_WM_WINDOW_TYPE_DESKTOP'))
x.XChangeProperty(d,w,atom(b'_NET_WM_WINDOW_TYPE'),4,32,0,C.cast(v,C.POINTER(C.c_ubyte)),1)
x.XMapRaised(d,w);x.XFlush(d)
while True:time.sleep(1)
PYDESKTOP
python3 "$E2E_T/desktop.py" >"$E2E_T/desktop.log" 2>&1 & DESKTOP=$!;E2E_PIDS+=($DESKTOP)
sleep .3
check 'card pixels remain above a late DESKTOP wallpaper window' pixel_is
kill "$DESKTOP";wait "$DESKTOP" 2>/dev/null || true

# App covers the diagnostic card; it must be above widgets, rather than hidden by them.
"$MS_BIN_DIR/examples/fakewin" Cover "Cover" 2244dd >"$E2E_T/cover.log" 2>&1 & APP=$!;E2E_PIDS+=($APP)
check 'managed app maps' bash -c 'xdotool search --class Cover'
APPWIN=$(xdotool search --class Cover|head -1)
xdotool windowactivate --sync "$APPWIN" 2>/dev/null; rdwmctl action togglefullscreen >/dev/null
app_covers() { import -window root -crop 1x1+85+165 +repage -depth 8 txt:- 2>/dev/null|tail -1|rg -qi '#2244DD'; }
check 'managed fullscreen app visibly covers widget' app_covers
kill "$APP";wait "$APP" 2>/dev/null || true
check 'widget reappears after app closes' pixel_is
BEFORE=$(jq -cS .widgets "$CFG")
decthemectl palette nord >/dev/null
check 'widget survives palette recolor' pixel_is
check 'palette preserves exact saved widget choices' test "$BEFORE" = "$(jq -cS .widgets "$CFG")"
decthemectl apply macos --reset --no-rdwm >/dev/null
check 'widget remains visible after theme switch' pixel_is
check 'theme preserves exact saved widget choices' test "$BEFORE" = "$(jq -cS .widgets "$CFG")"
HOST=$(cat "$XDG_RUNTIME_DIR/decwidgets.lock")
decwidgets daemon >"$E2E_T/duplicate.log" 2>&1
check 'duplicate daemon cannot toggle existing host off' kill -0 "$HOST"
check 'duplicate daemon cannot duplicate cards' count 2
kill "$HOST";check 'host shutdown removes cards' count 0
decwidgets daemon >"$E2E_T/decwidgets.log" 2>&1 & HOST=$!;E2E_PIDS+=($HOST)
check 'daemon restart restores both saved widgets' count 2
check 'restarted cards produce visible pixels' pixel_is
# Wallpaper replacement must not cover cards.
rdwmctl wallpaper "$E2E_T/blue.png" --mode fill >/dev/null
check 'wallpaper replacement keeps widget visible' pixel_is
cat > "$XDG_CONFIG_HOME/decebal/widgets/tint-test.toml" <<'TOML'
format=1
id="tint-test"
name="Tint test"
size=[80,60]
[root]
type="spacer"
width=80
height=60
background="#ffffff@0.5"
TOML
decwidgets add tint-test --anchor top-left --x 300 --y 160 >/dev/null
check 'translucent diagnostic card maps' count 3
pixel_tint() { import -window root -crop 1x1+310+170 +repage -depth 8 txt:- 2>/dev/null|tail -1|rg -qi "$1"; }
# Alpha .5 over rdwm's blue wallpaper; tolerate one premultiplication rounding unit.
check 'card samples actual wallpaper instead of covered app content' pixel_tint '#9[012]A[B-Eb-e][Ee][2345]'
rdwmctl wallpaper "$E2E_T/green.png" --mode fill >/dev/null
check 'non-composited translucent card refreshes after wallpaper change' pixel_tint '#8[89Aa][Dd][9A-Ea-e][Aa][345]'

cp "$CFG" "$E2E_T/good.json"
printf '{broken json' > "$CFG"
if decwidgets add builtin:next-clock >"$E2E_T/invalid.log" 2>&1; then echo 'FAIL CLI accepted invalid configuration';exit 1;fi
check 'CLI Add leaves malformed configuration untouched' bash -c 'test "$(cat "$1")" = "{broken json"' _ "$CFG"
mv "$E2E_T/good.json" "$CFG"
check 'host recovers after configuration is repaired' count 3
env XDG_SESSION_TYPE=wayland WAYLAND_DISPLAY=wayland-private decwidgets autostart
check 'Wayland autostart skips Xwayland without disturbing X11 host' kill -0 "$HOST"
env -u DISPLAY decwidgets autostart
check 'headless autostart exits without disturbing host' kill -0 "$HOST"
kill "$HOST";check 'watcher can stop cleanly before login restart' count 0
cp "$CFG" "$E2E_T/restore.json"
jq '.widgets.items=[]|.widgets.enabled=false' "$CFG" > "$CFG.new";mv "$CFG.new" "$CFG"
decwidgets autostart >"$E2E_T/autostart.log" 2>&1 & HOST=$!;E2E_PIDS+=($HOST)
check 'X11 login autostart stays alive with hidden empty collection' bash -c 'decwidgets status --json | jq -e ".running and (.enabled|not) and (.items|length==0)"'
check 'empty login watcher creates no unwanted cards' count 0
mv "$E2E_T/restore.json" "$CFG"
check 'login watcher notices subsequently restored choices' count 3
check 'login watcher paints actual visible cards' pixel_is
echo "passed $PASS, failed 0"
