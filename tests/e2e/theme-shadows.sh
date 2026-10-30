#!/usr/bin/env bash
# Actual theme metric -> live WM schema -> carrier lifecycle, isolated from the user's session.
set -eu
for cmd in Xvfb picom python3; do command -v "$cmd" >/dev/null || exit 77; done
python3 -I -c 'from Xlib import display' || exit 77
HERE=$(cd "$(dirname "$0")/../.." && pwd)
OLDHOME=$HOME; export OLDHOME
export MS_BIN_DIR=${MS_BIN_DIR:-$HERE/target/release}
source "$HERE/tests/e2e/env.sh"
N=${E2E_DISPLAY:-:319}
[[ "$N" =~ ^:([0-9]+)$ ]] && [ "$((10#${BASH_REMATCH[1]}))" -ge 2 ] || exit 2
[ ! -e "/tmp/.X11-unix/X${N#:}" ] && [ ! -e "/tmp/.X${N#:}-lock" ] || exit 77
ROOT_XDG=$(mktemp -d /tmp/decebal-shadow-xdg.XXXXXX)
export XDG_CACHE_HOME="$ROOT_XDG/cache" XDG_STATE_HOME="$ROOT_XDG/state" XDG_DATA_HOME="$ROOT_XDG/data" XDG_DATA_DIRS="$ROOT_XDG/nodata"
export E2E_WALLS=/nonexistent DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent
cleanup() { set +e; e2e_down; rm -rf "$ROOT_XDG"; return 0; }
trap cleanup EXIT
e2e_up "$N"
mkdir -p "$E2E_T/themes/shadow-on" "$E2E_T/themes/shadow-off"
export DECEBAL_THEMES="$E2E_T/themes" DECEBAL_SYSTEM_THEMES="$E2E_T/no-system" SHADOW_THEME_CFG="$E2E_CFG"
python3 - <<'PY'
import os,pathlib
for name,strength in [('shadow-on',.65),('shadow-off',0)]:
 p=pathlib.Path(os.environ['DECEBAL_THEMES'])/name/'theme.toml'
 p.write_text(f'''format=1
id="{name}"
name="Shadow test fixture"
[colors.dark]
background="#20242a"
text="#ffffff"
accent="#55aaff"
[metrics]
shadow={strength}
[dock]
enabled=false
[bar]
enabled=false
''')
PY
cat > "$E2E_T/picom.conf" <<'CONF'
backend="xrender";
vsync=false;
shadow=false;
fading=false;
no-frame-pacing=true;
CONF
picom --config "$E2E_T/picom.conf" > "$E2E_T/picom.log" 2>&1 & E2E_PIDS+=($!)
python3 -I - > "$E2E_T/client.log" 2>&1 <<'CLIENT' &
from Xlib import X,display
d=display.Display();root=d.screen().root
w=root.create_window(20,20,400,260,0,d.screen().root_depth,X.InputOutput,X.CopyFromParent,background_pixel=0xeeeeee,event_mask=X.ExposureMask)
w.set_wm_name('Private theme shadow fixture');w.set_wm_class('shadow-bridge','ShadowBridge');w.map();d.flush()
while True:
 if d.next_event().type==X.Expose:w.clear_area();d.flush()
CLIENT
E2E_PIDS+=($!)
python3 - <<'PY'
import os,pathlib,subprocess,time,tomllib
from Xlib import display,error
cfg=pathlib.Path(os.environ['SHADOW_THEME_CFG']);d=display.Display();root=d.screen().root
atom=d.intern_atom('_RDWM_SHADOW_OWNER')
def carriers():
 result=[]
 for w in root.query_tree().children:
  try:
   if w.get_full_property(atom,0):result.append(w)
  except error.BadWindow:pass # Destruction can finish during enumeration.
 return result
def wait(fn,reason):
 for _ in range(120):
  if fn():return
  time.sleep(.05)
 raise AssertionError(reason)
wait(lambda: (lambda p:p is not None and bool(p.value))(root.get_full_property(d.intern_atom('_NET_CLIENT_LIST'),0)),'client missing')
assert not carriers(),'default-off WM allocated a shadow'
for theme,enabled in [('shadow-on',True),('shadow-off',False)]:
 subprocess.run(['decthemectl','theme','apply',theme],check=True,capture_output=True)
 actual=tomllib.loads(cfg.read_text())['decoration']
 assert actual['shadow']==enabled,actual
 assert actual['shadow_opacity']==(23 if enabled else 0),actual
 assert actual['shadow_inactive_opacity']==(12 if enabled else 0),actual
 wait(lambda:bool(carriers())==enabled,'theme strength did not change actual carrier allocation')
 assert subprocess.run(['rdwmctl','ping'],capture_output=True).returncode==0
print('PASS native theme metric -> live schema -> bounded opacity -> carrier enable/free')
PY
