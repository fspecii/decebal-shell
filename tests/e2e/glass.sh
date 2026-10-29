#!/usr/bin/env bash
# Actual picom composition on a private Xvfb; no live session resources.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$HERE/target}/${PROFILE:-debug}
if [ "${NO_BUILD:-0}" != 1 ]; then
 args=(); [ "${PROFILE:-debug}" != release ] || args+=(--release)
 (cd "$HERE"; CARGO_BUILD_JOBS=2 cargo build --offline "${args[@]}" --bin decwidgets --bin decbar --bin decthemectl)
fi
for tool in Xvfb picom xdotool xprop feh import magick python3 jq rg; do command -v "$tool" >/dev/null || { echo "$tool required"; exit 77; }; done
N=${E2E_DISPLAY:-:231}
case "$N" in :0|:1) echo 'refusing live display'; exit 2;; esac
[ ! -e "/tmp/.X11-unix/X${N#:}" ] || { echo 'display occupied'; exit 2; }
T=$(mktemp -d /tmp/decebal-glass.XXXXXX); PIDS=()
cleanup() { local result=$?; for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true; if [ "$result" != 0 ] || [ "${KEEP:-0}" = 1 ]; then echo "test artifacts: $T"; else rm -rf "$T"; fi; }
trap cleanup EXIT
export HOME="$T/home" XDG_CONFIG_HOME="$T/cfg" XDG_RUNTIME_DIR="$T/run" XDG_DATA_HOME="$T/data" XDG_STATE_HOME="$T/state" XDG_DATA_DIRS="$T/nodata"
export DECEBAL_CONFIG="$T/cfg/omarchy" DECEBAL_BACKEND=x11 DISPLAY="$N" DECEBAL_SYSTEM_THEMES="$T/nosystem" DECEBAL_COLOR_SCHEME=dark
unset WAYLAND_DISPLAY RDWM_SOCK XAUTHORITY
mkdir -p "$HOME" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_DATA_DIRS"
chmod 700 "$XDG_RUNTIME_DIR"; export PATH="$BIN:$PATH"
PASS=0
check() { local label=$1; shift; for _ in $(seq 1 100); do if "$@" >/dev/null 2>&1; then echo "  ok   $label"; PASS=$((PASS+1)); return; fi; sleep .1; done; echo "FAIL $label (logs $T)"; cat "$T/picom.log"; exit 1; }
Xvfb "$N" -screen 0 1280x800x24 +extension GLX +extension RENDER -nolisten tcp -ac >"$T/xvfb.log" 2>&1 & PIDS+=($!)
check 'private Xvfb starts' test -e "/tmp/.X11-unix/X${N#:}"
picom --backend glx --config "${PICOM_CONFIG:-$HERE/contrib/picom/decebal-glass.conf}" --no-vsync >"$T/picom.log" 2>&1 & PICOM=$!; PIDS+=($PICOM)
sleep 1; check 'picom GLX remains alive' kill -0 "$PICOM"
decthemectl theme install "$HERE/themes/nextkde" --force >"$T/install.log" 2>&1
decthemectl apply nord --no-rdwm >/dev/null
decthemectl palette nord >/dev/null
decthemectl apply nextkde --reset --mode dark --no-rdwm >/dev/null
check 'NextKDE reset removes previous Nord palette and restores own colors' bash -c 'decthemectl get --json | jq -e '\''.theme=="nextkde" and .selection.palette==null and .colors.bar=="#294261"'\'''
jq '.widgets={enabled:true,items:[{use:"builtin:next-clock",anchor:"top-left",x:40,y:120}]}' "$DECEBAL_CONFIG/decebal.json" > "$T/shell.json"
mv "$T/shell.json" "$DECEBAL_CONFIG/decebal.json"
decbar >"$T/bar.log" 2>&1 & BAR=$!; PIDS+=($BAR)
decwidgets daemon >"$T/widgets.log" 2>&1 & PIDS+=($!)
check 'live analog clock maps' bash -c 'xdotool search --class "^decwidgets$"'
WIDGET=$(xdotool search --class '^decwidgets$' | head -1)
# Find the shell bar by its DOCK type, excluding the desktop widget.
bar_id() { python3 - <<'PY'
import ctypes as C,subprocess
x=C.CDLL('libX11.so.6');P=C.c_void_p;U=C.c_ulong
x.XOpenDisplay.restype=P;d=x.XOpenDisplay(None);x.XDefaultRootWindow.argtypes=[P];x.XDefaultRootWindow.restype=U
x.XQueryTree.argtypes=[P,U,C.POINTER(U),C.POINTER(U),C.POINTER(C.POINTER(U)),C.POINTER(C.c_uint)]
r=U();p=U();children=C.POINTER(U)();n=C.c_uint();x.XQueryTree(d,x.XDefaultRootWindow(d),C.byref(r),C.byref(p),C.byref(children),C.byref(n))
for w in children[:n.value]:
 props=subprocess.check_output(['xprop','-id',hex(w),'_NET_WM_WINDOW_TYPE'],text=True)
 if '_NET_WM_WINDOW_TYPE_DOCK' in props: print(hex(w));break
PY
}
BARWIN=$(bar_id); check 'bar DOCK window maps' test -n "$BARWIN"
hint() { xprop -id "$1" _DECEBAL_BLUR | rg -q "= $2$"; }
check 'NextKDE bar requests scoped blur' hint "$BARWIN" 1
check 'NextKDE analog widget requests scoped blur' hint "$WIDGET" 1
cat > "$T/depth.py" <<'PY'
import ctypes as C,sys
x=C.CDLL('libX11.so.6');P=C.c_void_p;U=C.c_ulong
x.XOpenDisplay.restype=P;d=x.XOpenDisplay(None)
x.XGetGeometry.argtypes=[P,U,C.POINTER(U),C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint)]
r=U();a=C.c_int();b=C.c_int();w=C.c_uint();h=C.c_uint();border=C.c_uint();depth=C.c_uint()
x.XGetGeometry(d,int(sys.argv[1],0),C.byref(r),C.byref(a),C.byref(b),C.byref(w),C.byref(h),C.byref(border),C.byref(depth));assert depth.value==32,depth.value
PY
check 'live analog clock uses 32-bit ARGB visual' python3 "$T/depth.py" "$WIDGET"
cat > "$T/shape.py" <<'PYSHAPE'
import ctypes as C,sys
x=C.CDLL('libX11.so.6'); ext=C.CDLL('libXext.so.6');P=C.c_void_p;U=C.c_ulong
x.XOpenDisplay.restype=P;d=x.XOpenDisplay(None)
class Rect(C.Structure): _fields_=[('x',C.c_short),('y',C.c_short),('w',C.c_ushort),('h',C.c_ushort)]
ext.XShapeGetRectangles.argtypes=[P,U,C.c_int,C.POINTER(C.c_int),C.POINTER(C.c_int)];ext.XShapeGetRectangles.restype=C.POINTER(Rect)
n=C.c_int();order=C.c_int();rects=ext.XShapeGetRectangles(d,int(sys.argv[1],0),0,C.byref(n),C.byref(order))
# The bar's large popup backing canvas must not blur the desktop clock below it.
assert n.value>0
assert not any(r.x<=60<r.x+r.w and r.y<=145<r.y+r.h for r in rects[:n.value]),'transparent bar canvas overlaps widget'
PYSHAPE
check 'bar shape excludes transparent canvas above analog widget' python3 "$T/shape.py" "$BARWIN"
for color in bb2020 2020bb; do
 magick -size 1280x800 xc:"#$color" "$T/bg-$color.png" 2>/dev/null
 feh --no-fehbg --bg-fill "$T/bg-$color.png"; sleep 1
 import -window root "$T/$color.png"
done
python3 - "$T" <<'PY'
import subprocess,sys
p=sys.argv[1]
def pixel(file,x,y):
 raw=subprocess.check_output(['magick',file,'-crop',f'1x1+{x}+{y}','-depth','8','rgb:-']);return tuple(raw[:3])
for name,x,y in [('bar',1100,8),('widget',60,145)]:
 a=pixel(p+'/bb2020.png',x,y);b=pixel(p+'/2020bb.png',x,y)
 print(f'  composition {name}: {a} -> {b}')
 assert sum(abs(c-d) for c,d in zip(a,b))>100,(name,a,b)
 assert a!=(187,32,32) and b!=(32,32,187),(name,'surface absent instead of translucent')
PY
PASS=$((PASS+2)); echo '  ok   actual bar and analog widget pixels reveal changing background'
decthemectl apply nord --reset --no-rdwm >/dev/null
check 'opaque theme clears bar blur hint' hint "$BARWIN" 0
# Theme changes rebuild widget windows, so find its current id for each assertion.
widget_clear() { local id; id=$(xdotool search --class '^decwidgets$' | head -1); hint "$id" 0; }
check 'opaque theme clears shared widget blur hint' widget_clear
# A blur method enables Picom's default blur unless an explicit all-window rule
# disables it. Test untagged/zero-hint transparent popup canvas over sharp detail.
magick -size 1280x800 pattern:checkerboard -threshold 50% "$T/checker.png"
feh --no-fehbg --bg-fill "$T/checker.png"; sleep 1
import -window root "$T/nord-checker.png"
python3 - "$T/nord-checker.png" <<'PYSHARP'
import subprocess,sys
raw=subprocess.check_output(['magick',sys.argv[1],'-crop','80x80+600+140','+repage','-depth','8','gray:-'])
mean=sum(raw)/len(raw);std=(sum((x-mean)**2 for x in raw)/len(raw))**.5
print(f'  non-glass background contrast: {std:.1f}')
assert std>80, f'ordinary desktop content was blurred through unused bar canvas: {std}'
PYSHARP
PASS=$((PASS+1)); echo '  ok   non-glass canvas leaves ordinary desktop detail sharp'
check 'picom survives material change' kill -0 "$PICOM"
echo "passed $PASS, failed 0"
