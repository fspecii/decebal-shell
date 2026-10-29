#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# Actual picom composition on a private Xvfb; no live session resources.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$HERE/target}/${PROFILE:-debug}
if [ "${NO_BUILD:-0}" != 1 ]; then
 args=(); [ "${PROFILE:-debug}" != release ] || args+=(--release)
 (cd "$HERE"; CARGO_BUILD_JOBS=2 cargo build --offline "${args[@]}" --bin decwidgets --bin decbar --bin decthemectl --example fakewin)
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
picom --backend "${PICOM_BACKEND:-xrender}" --blur-method kernel --blur-kern 3x3box --config "${PICOM_CONFIG:-$HERE/contrib/picom/decebal-glass.conf}" --no-vsync >"$T/picom.log" 2>&1 & PICOM=$!; PIDS+=($PICOM)
sleep 1; check 'private compositor remains alive' kill -0 "$PICOM"
decthemectl theme install "$HERE/themes/nextkde" --force >"$T/install.log" 2>&1
decthemectl apply nord --no-rdwm >/dev/null
decthemectl palette nord >/dev/null
decthemectl apply nextkde --reset --mode dark --no-rdwm >/dev/null
check 'NextKDE reset removes previous Nord palette and restores own colors' bash -c 'decthemectl get --json | jq -e '\''.theme=="nextkde" and .selection.palette==null and .colors.bar=="#294261"'\'''
jq '.widgets={enabled:true,items:[{use:"builtin:next-clock",anchor:"top-left",x:40,y:120}]}' "$DECEBAL_CONFIG/decebal.json" > "$T/shell.json"
mv "$T/shell.json" "$DECEBAL_CONFIG/decebal.json"
# Native capture needs a real wallpaper pixmap, as provided by rdwm/feh in a session.
magick -size 1280x800 xc:'#335577' "$T/initial-wallpaper.png"
feh --no-fehbg --bg-fill "$T/initial-wallpaper.png"
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
bar_maps() { BARWIN=$(bar_id); test -n "$BARWIN"; }
check 'bar DOCK window maps' bar_maps
hint() { xprop -id "$1" _DECEBAL_BLUR | rg -q "= $2$"; }
native_hint() { xprop -id "$1" _DECEBAL_GLASS_NATIVE | rg -q "= $2$"; }
check 'NextKDE bar captures native glass backdrop' native_hint "$BARWIN" 1
check 'NextKDE analog widget captures native glass backdrop' native_hint "$WIDGET" 1
check 'native bar avoids double Picom blur' hint "$BARWIN" 0
check 'native widget avoids double Picom blur' hint "$WIDGET" 0
cat > "$T/depth.py" <<'PY'
import ctypes as C,sys
x=C.CDLL('libX11.so.6');P=C.c_void_p;U=C.c_ulong
x.XOpenDisplay.restype=P;d=x.XOpenDisplay(None)
x.XGetGeometry.argtypes=[P,U,C.POINTER(U),C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint)]
r=U();a=C.c_int();b=C.c_int();w=C.c_uint();h=C.c_uint();border=C.c_uint();depth=C.c_uint()
x.XGetGeometry(d,int(sys.argv[1],0),C.byref(r),C.byref(a),C.byref(b),C.byref(w),C.byref(h),C.byref(border),C.byref(depth));assert depth.value==int(sys.argv[2] if len(sys.argv)>2 else 32),depth.value
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
# A checkerboard exposes the material's actual backdrop transformation. Compare
# the live card against straight alpha compositing of its ordinary foreground.
magick -size 1280x800 pattern:checkerboard -threshold 50% "$T/native-checker-bg.png"
feh --no-fehbg --bg-fill "$T/native-checker-bg.png"; sleep 1
import -window root "$T/native-checker.png"
decwidgets render next-clock --png "$T/foreground.png" --margin 0 >/dev/null
python3 - "$T" <<'PYNATIVE'
import subprocess,sys
p=sys.argv[1]
def raw(file,mode):return subprocess.check_output(['magick',file,'-depth','8',mode+':-'])
bg=raw(p+'/native-checker-bg.png','rgb');actual=raw(p+'/native-checker.png','rgb');fg=raw(p+'/foreground.png','rgba')
changed=0; samples=[]
for y in range(60,130):
 for x in range(2,12):
  i=((120+y)*1280+40+x)*3;f=(y*192+x)*4;a=fg[f+3]/255
  samples.append(actual[i])
  straight=[fg[f+c]*a+bg[i+c]*(1-a) for c in range(3)]
  if max(abs(actual[i+c]-straight[c]) for c in range(3))>15:changed+=1
print(f'  native patterned edge transformed pixels: {changed}/700')
assert changed>100,'native marker present but patterned backdrop was not transformed'
mean=sum(samples)/len(samples);std=(sum((v-mean)**2 for v in samples)/len(samples))**.5
print(f'  captured patterned edge contrast: {std:.1f}')
assert std>25,'native edge contains a stale flat backdrop rather than checkerboard'
PYNATIVE
PASS=$((PASS+1)); echo '  ok   actual native card transforms checkerboard backdrop'
for color in bb2020 2020bb; do
 magick -size 1280x800 xc:"#$color" "$T/bg-$color.png" 2>/dev/null
 feh --no-fehbg --bg-fill "$T/bg-$color.png"; sleep 1
 import -window root "$T/$color.png"
 import -window "$BARWIN" "$T/bar-client-$color.png"
done
python3 - "$T" <<'PY'
import subprocess,sys
p=sys.argv[1]
def pixel(file,x,y):
 raw=subprocess.check_output(['magick',file,'-crop',f'1x1+{x}+{y}','-depth','8','rgb:-']);return tuple(raw[:3])
for name,x,y in [('bar',1100,8),('widget',60,145)]:
 a=pixel(p+'/bb2020.png',x,y);b=pixel(p+'/2020bb.png',x,y)
 print(f'  composition {name}: {a} -> {b}')
 if name=="bar": print("  direct bar client:",pixel(p+"/bar-client-bb2020.png",x,y),"->",pixel(p+"/bar-client-2020bb.png",x,y))
 assert sum(abs(c-d) for c,d in zip(a,b))>100,(name,a,b)
 assert a!=(187,32,32) and b!=(32,32,187),(name,'surface absent instead of translucent')
PY
PASS=$((PASS+2)); echo '  ok   actual bar and analog widget pixels reveal changing background'
# A normal redirected window below the bar must replace the wallpaper in the
# optical backdrop. Sampling the root wallpaper alone is not sufficient.
"$BIN/examples/fakewin" GlassBackdrop GlassBackdrop 18cb59 >"$T/backdrop-window.log" 2>&1 & BACKDROP=$!;PIDS+=($BACKDROP)
check 'normal backdrop window maps' bash -c 'xdotool search --class GlassBackdrop'
BWIN=$(xdotool search --class GlassBackdrop | head -1)
xdotool windowsize "$BWIN" 1280 90 windowmove "$BWIN" 0 0 windowlower "$BWIN"
sleep 1
import -window root "$T/window-backdrop.png"
python3 - "$T/window-backdrop.png" <<'PYWINDOW'
import subprocess,sys
p=subprocess.check_output(['magick',sys.argv[1],'-crop','1x1+1100+8','-depth','8','rgb:-'])[:3]
print(f'  underlying window glass sample: {tuple(p)}')
assert p[1]>p[0]+60 and p[1]>p[2]+60, 'glass ignored the actual window behind it'
PYWINDOW
PASS=$((PASS+1));echo '  ok   native glass captures the normal window behind it'
kill "$BACKDROP";wait "$BACKDROP" 2>/dev/null||true
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
check 'ordinary theme clears native glass marker on bar' native_hint "$BARWIN" 0
widget_native_clear() { local id; id=$(xdotool search --class '^decwidgets$' | head -1); native_hint "$id" 0; }
check 'ordinary theme clears native glass marker on widget' widget_native_clear
check 'picom survives material change' kill -0 "$PICOM"
kill "$PICOM"; wait "$PICOM" 2>/dev/null || true
widget_depth() { local id; id=$(xdotool search --class '^decwidgets$' | head -1); python3 "$T/depth.py" "$id" "$1"; }
check 'widget host safely rebuilds without compositor' widget_depth 24
picom --backend "${PICOM_BACKEND:-xrender}" --blur-method kernel --blur-kern 3x3box --config "$HERE/contrib/picom/decebal-glass.conf" --no-vsync >"$T/picom-restart.log" 2>&1 & PICOM=$!;PIDS+=($PICOM)
check 'widget host restores ARGB after compositor starts or restarts' widget_depth 32
echo "passed $PASS, failed 0"
