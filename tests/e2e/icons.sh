#!/usr/bin/env bash
# Icon themes end to end on a private Xvfb + rdwm: the real macdock and macsettings, themes switched with
# `macthemectl icons` and through the Icons pane. Icon themes are symlinked from ICON_SRC (default: the installed ones)
# into a private XDG_DATA_HOME; nothing is installed anywhere.
#   tests/e2e/icons.sh                  (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:90 by default)
# Everything is private: own HOME/XDG dirs, only PIDs started here are killed, never the live display.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${CARGO_TARGET_DIR:-$HERE/target}/release
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert compare; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
(cd "$HERE" && cargo build -q --offline --release --bins --examples) || { echo "build failed"; exit 2; }
N=${E2E_DISPLAY:-:90}
case "$N" in :1|:2|1|2|:0|0) echo "refusing a live display"; exit 2;; esac
REAL_HOME=$HOME

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/macshell-icons-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  if [ "$SHOTS" = "$TMP/shots" ]; then rm -rf "$TMP"; else find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; rmdir "$TMP" 2>/dev/null; fi; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
ne() { if [ "$2" != "$3" ]; then pass "$1"; else fail "$1" "both '$2'"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata
export MACSHELL_CONFIG=$TMP/cfg/omarchy MACSHELL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl MACSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export MACSHELL_COLOR_SCHEME=dark GSETTINGS_BACKEND=memory
unset WAYLAND_DISPLAY RDWM_SOCK DBUS_SESSION_BUS_ADDRESS
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$MACSHELL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$XDG_DATA_HOME/icons" "$TMP/nodata"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$MACSETTINGS_CONFIG

# themes: whatever is installed here, linked (not copied) into the private data home
link() { for d in "$@"; do [ -d "$d" ] && ln -sfn "$d" "$XDG_DATA_HOME/icons/$(basename "$d")"; done; }
ICON_SRC=${ICON_SRC:-"$REAL_HOME/.local/share/icons /usr/share/icons"}
for root in $ICON_SRC; do
  for t in hicolor Adwaita WhiteSur WhiteSur-dark WhiteSur-cursors Fluent-dark breeze Yaru Papirus Papirus-Dark Tela; do link "$root/$t"; done
done
have() { [ -e "$XDG_DATA_HOME/icons/$1/index.theme" ]; }
for need in hicolor WhiteSur-dark breeze; do have $need || { echo "icon theme $need needed (ICON_SRC)"; exit 77; }; done
THEMES=(); for t in WhiteSur-dark breeze Yaru Adwaita Papirus-Dark Tela Fluent-dark; do have $t && THEMES+=($t); done

printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[bar]\nshow = false\n' > "$CFG"
printf '{"version":1,"pinned":["launchpad","firefox","utilities-terminal","system-file-manager","preferences-system","image-x-generic","org.kde.dolphin","trash"],"order":["launchpad","firefox","utilities-terminal","system-file-manager","preferences-system","image-x-generic","org.kde.dolphin","trash"]}\n' > "$MACSHELL_CONFIG/dock-pinned-macos.json"
printf '{"workspaces": false}\n' > "$MACSHELL_CONFIG/macshell.json"

Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 800 300
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
macbar >"$TMP/macbar.log" 2>&1 & PIDS+=($!)
macdock >"$TMP/macdock.log" 2>&1 & PIDS+=($!)
sleep 2

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
# the dock strip at the bottom centre (the dock is centred; its panel is ~71 px high)
dockcrop() { import -window root -crop 760x110+420+790 +repage "$1" 2>/dev/null; }
sum() { dockcrop "$TMP/d.png"; magick "$TMP/d.png" -depth 8 rgba:- | md5sum | cut -c1-12; }
settle() { sleep 1.4; }
ic() { macthemectl icons "$@"; }
stat_json() { macthemectl icons status --json | jq -r "$1"; }

echo "== start: automatic"
eq "default is auto" "$(stat_json .configured)" auto
eq "chain has hicolor then the Adwaita fallback" "$(stat_json '.chain|join(",")|test("hicolor,Adwaita")')" true
shot 00-start-auto

echo "== list / set / refusals"
ic list | grep -q "WhiteSur-dark" && pass "list shows WhiteSur-dark" || fail "list"
ic list | grep "WhiteSur-cursors" | grep -q "not selectable" && pass "cursor theme flagged" || fail "cursor theme not flagged"
ic set WhiteSur-cursors >/dev/null 2>&1 && fail "cursor theme accepted" || pass "cursor theme refused"
ic set NoSuchTheme >/dev/null 2>&1 && fail "unknown theme accepted" || pass "unknown theme refused"
ic scale 7 >/dev/null 2>&1 && fail "scale 7 accepted" || pass "scale 7 refused"
eq "refusals changed nothing" "$(jq -c .icons "$MACSHELL_CONFIG/macshell.json")" null

echo "== the dock follows the theme (live, no restart)"
declare -A SUM
for t in "${THEMES[@]}"; do
  ic set "$t" >/dev/null; settle
  eq "status says $t" "$(stat_json .active)" "$t"
  SUM[$t]=$(sum)
  dockcrop "$SHOTS/dock-$t.png"
  shot "10-desktop-$t"
done
seen=""; dup=0
for t in "${THEMES[@]}"; do case " $seen " in *" ${SUM[$t]} "*) dup=1; echo "     same dock pixels: $t = ${SUM[$t]}";; esac; seen="$seen ${SUM[$t]}"; done
[ $dup = 0 ] && pass "every theme draws a different dock (${#THEMES[@]} themes)" || fail "two themes drew identical docks"
eq "macdock still alive" "$(kill -0 "${PIDS[3]}" 2>/dev/null && echo alive)" alive

echo "== size scale and tint"
ic set WhiteSur-dark >/dev/null; settle
base=$(sum); dockcrop "$SHOTS/dock-scale-100.png"
ic scale 0.8 >/dev/null; settle
small=$(sum); dockcrop "$SHOTS/dock-scale-080.png"
ne "scale 0.8 redraws the dock" "$base" "$small"
ic scale 1 >/dev/null; settle
eq "scale 1 is pixel-identical to the start" "$(sum)" "$base"
ic tint on >/dev/null; settle
eq "tint toggle stored" "$(jq -r .icons.monochrome_tint "$MACSHELL_CONFIG/macshell.json")" true
ic tint off >/dev/null

echo "== auto follows gtk settings.ini"
ic set auto >/dev/null
mkdir -p "$XDG_CONFIG_HOME/gtk-3.0"
printf '[Settings]\ngtk-icon-theme-name=breeze\n' > "$XDG_CONFIG_HOME/gtk-3.0/settings.ini"
settle
eq "auto picks the GTK theme" "$(stat_json .active)" breeze
printf '[Settings]\ngtk-icon-theme-name=NotInstalled\n' > "$XDG_CONFIG_HOME/gtk-3.0/settings.ini"
settle
eq "an uninstalled GTK theme means hicolor only" "$(stat_json .active)" null
rm -f "$XDG_CONFIG_HOME/gtk-3.0/settings.ini"

echo "== a theme appearing while running"
ic set Fresh >/dev/null 2>&1 || true
printf '{"icons": {"theme": "Fresh"}}\n' > "$MACSHELL_CONFIG/macshell.json"
settle
eq "unknown explicit theme is reported" "$(stat_json '.problem != null')" true
mkdir -p "$XDG_DATA_HOME/icons/Fresh/scalable/apps"
printf '[Icon Theme]\nName=Fresh\nDirectories=scalable/apps\n[scalable/apps]\nSize=64\nMinSize=8\nMaxSize=512\nType=Scalable\n' > "$XDG_DATA_HOME/icons/Fresh/index.theme"
printf '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><circle cx="32" cy="32" r="30" fill="#e91e63"/></svg>' > "$XDG_DATA_HOME/icons/Fresh/scalable/apps/firefox.svg"
settle
eq "installing it fixes the selection" "$(stat_json .active)" Fresh
shot 20-fresh-theme
ic reset >/dev/null

echo "== settings pane"
ic set breeze >/dev/null
export MACSETTINGS_DEBUG_LAYOUT=$TMP/dump.json
macsettings --config "$CFG" icons >"$TMP/ms.log" 2>&1 & PIDS+=($!)
wait_for 8 test -s "$MACSETTINGS_DEBUG_LAYOUT" || fail "settings dump never written"
sleep 1
WID=$(xdotool search --sync --class MacSettings 2>/dev/null | head -1)
[ -n "$WID" ] && pass "settings window up" || fail "no settings window"
q() { jq -r "$1" "$MACSETTINGS_DEBUG_LAYOUT" 2>/dev/null; }
eq "icons pane is shown" "$(q .pane)" icons
shot 30-settings-icons-breeze
tile() { jq -r "[.layouts[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$MACSETTINGS_DEBUG_LAYOUT"; }
ctl() { jq -r "[.ctls[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$MACSETTINGS_DEBUG_LAYOUT"; }
click() { xdotool mousemove --window "$WID" $1 $2; sleep 0.1; xdotool click --window "$WID" 1; sleep 0.6; }
scroll() { for i in $(seq 1 $1); do xdotool mousemove --window "$WID" 500 500; xdotool click --window "$WID" 5; done; sleep 0.5; }
eq "the in-use theme is highlighted at first" "$(q '[.layouts[]|select(.id=="l:icons:pick:breeze")]|length')" 1
scroll 6
shot 31-settings-list
click $(tile l:icons:pick:WhiteSur-dark); sleep 0.4
eq "clicking a row does not apply it yet" "$(stat_json .active)" breeze
shot 32-settings-whitesur-previewed
click $(ctl ic:use); settle
eq "Use this theme applies WhiteSur-dark" "$(stat_json .active)" WhiteSur-dark
eq "...and wrote macshell.json" "$(jq -r .icons.theme "$MACSHELL_CONFIG/macshell.json")" WhiteSur-dark
shot 33-settings-applied
scroll 12
shot 34-settings-bottom
click $(ctl ic:tint); settle
eq "tint toggle in the pane" "$(jq -r .icons.monochrome_tint "$MACSHELL_CONFIG/macshell.json")" true
click $(ctl ic:reset); settle
eq "Reset removes the icons object" "$(jq -c .icons "$MACSHELL_CONFIG/macshell.json")" null
# external change while the pane is open
ic set Adwaita >/dev/null; sleep 1.6
shot 35-settings-external-adwaita

echo "== no crashes, no panics"
for p in "${PIDS[@]}"; do kill -0 "$p" 2>/dev/null || fail "process $p died"; done
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
rss=$(awk '/VmRSS/ {print $2}' /proc/${PIDS[3]}/status 2>/dev/null)
echo "     macdock RSS: ${rss:-?} kB"
[ -n "$rss" ] && [ "$rss" -lt 90000 ] && pass "macdock RSS under 90 MB" || fail "macdock RSS" "${rss:-?} kB"
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
