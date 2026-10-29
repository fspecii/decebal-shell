#!/usr/bin/env bash
# This desktop test must not retint apps through an inherited session bus.
export DECEBAL_SYSTEM_AUTOMATIC=0
# Theme engine end to end, on a private Xvfb with a private rdwm, HOME and runtime dir:
#   validate + pack + install a community-style theme package (examples/themes/sunset-grid), apply it (rdwm config, dock, wallpaper,
#   widgets), see its desktop widgets appear and react to a click, preview-and-revert, undo, uninstall, the safety refusals, and the
#   Theme pane's package gallery. Screenshots land in $SHOTS.
#   tests/e2e/theme-engine.sh          (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:88 by default; PROFILE=release for a release build)
# Only PIDs started here are killed; the live session is never touched (DISPLAY :1 is refused).
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-debug}
BIN=${CARGO_TARGET_DIR:-$HERE/target}/$PROFILE
RDWM_DIR=${RDWM_BIN_DIR:-$HOME/Projects/rdwm/target/release}
[ -x "$RDWM_DIR/rdwm" ] || { echo "rdwm not found (set RDWM_BIN_DIR)"; exit 77; }
for t in Xvfb xdotool import jq convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
if [ "$PROFILE" = release ]; then (cd "$HERE" && cargo build -q --offline --release --bins --examples) || { echo "build failed"; exit 2; }
else (cd "$HERE" && cargo build -q --offline --bins --examples) || { echo "build failed"; exit 2; }; fi
N=${E2E_DISPLAY:-:88}
[ "$N" = ":1" ] && { echo "refusing the live display"; exit 2; }

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-engine-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
has() { if printf '%s' "$2" | grep -q -- "$3"; then pass "$1"; else fail "$1" "no '$3' in: $(printf '%s' "$2" | head -c 300)"; fi; }
wait_for() { local t=$1; shift; local end=$(($(date +%s%N) / 1000000 + t * 1000)); while [ $(($(date +%s%N) / 1000000)) -lt $end ]; do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; return 1; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_STATE_HOME=$TMP/state
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N RDWMCTL=$RDWM_DIR/rdwmctl DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export DECEBAL_COLOR_SCHEME=dark DECEBAL_SYSTEM_THEMES=$TMP/nosystem DECEBAL_LOG=debug DECEBAL_LOG_DIR=$TMP/logs
unset WAYLAND_DISPLAY RDWM_SOCK
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME/applications" "$TMP/nodata" "$TMP/work"
chmod 700 "$XDG_RUNTIME_DIR"
export PATH="$RDWM_DIR:$BIN:$PATH"
CFG=$DECSETTINGS_CONFIG
THEMES=$XDG_CONFIG_HOME/decebal/themes
printf '# test config\n[general]\ngap_inner = 6\n\n[decoration]\nenabled = true\n' > "$CFG"
printf '{"version":1,"pinned":["launchpad","trash"],"order":["launchpad","trash"]}\n' > "$DECEBAL_CONFIG/dock-pinned-macos.json"

Xvfb $N -screen 0 1600x900x24 +extension RANDR +extension SHAPE -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 &
PIDS+=($!)
wait_for 5 test -e /tmp/.X11-unix/X${N#:} || { echo "Xvfb failed"; exit 2; }
xdotool mousemove 800 450
rdwm --backend x11 --config "$CFG" >"$TMP/rdwm.log" 2>&1 & PIDS+=($!)
wait_for 5 bash -c "xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window" || { echo "rdwm did not start"; cat "$TMP/rdwm.log"; exit 2; }
rdwmctl wallpaper --color "#46607a" >/dev/null 2>&1
decbar >"$TMP/decbar.log" 2>&1 & PIDS+=($!)
decdock >"$TMP/decdock.log" 2>&1 & PIDS+=($!)
sleep 2

shot() { import -window root "$SHOTS/$1.png" 2>/dev/null; }
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
eqpx() { local end=$((SECONDS + 6)) got; while [ $SECONDS -lt $end ]; do got=$(px "$2" "$3"); [ "$got" = "$4" ] && { pass "$1"; return; }; sleep 0.3; done; fail "$1" "got '$got', want '$4'"; }
rdcfg() { rdwmctl config | jq -r "$1"; }
sel_name() { local n; n=$(jq -r '.theme.name // empty' "$DECEBAL_CONFIG/decebal.json" 2>/dev/null); echo "${n:-none}"; }
widget_windows() { xdotool search --class decwidgets 2>/dev/null | wc -l; }

echo "== validate, pack, install the community theme"
PKG=$HERE/examples/themes/sunset-grid
out=$(decthemectl theme validate "$PKG" 2>&1); has "example package validates" "$out" "^ok: Sunset Grid"
out=$(decthemectl theme pack "$PKG" -o "$TMP/work/sunset-grid.dtheme" 2>&1); has "pack records a content hash" "$out" "sha256:"
out=$(decthemectl theme validate "$TMP/work/sunset-grid.dtheme" 2>&1); has "archive validates" "$out" "^ok:.*archive"
out=$(decthemectl theme install "$TMP/work/sunset-grid.dtheme" 2>&1); has "install" "$out" "installed sunset-grid"
decthemectl theme list | grep -q "sunset-grid.*1.0.0.*user" && pass "list shows the installed theme" || fail "list"
out=$(decthemectl theme install "$TMP/work/sunset-grid.dtheme" 2>&1); has "installing twice is refused" "$out" "already installed"
decthemectl theme list --json | jq -e '.[]|select(.id=="sunset-grid" and .integrity=="ok")' >/dev/null && pass "integrity ok in list --json" || fail "integrity"

echo "== safety: bad packages are refused and change nothing"
mkdir -p "$TMP/work/evil/widgets" "$TMP/work/evil2"
cp -r "$PKG/theme.toml" "$TMP/work/evil/"; sed -i 's/id = "sunset-grid"/id = "evil"/; s#wallpapers/sunset.jpg#wallpapers/missing.jpg#' "$TMP/work/evil/theme.toml"
printf '#!/bin/sh\ntouch /tmp/PWNED\n' > "$TMP/work/evil/run.sh"
out=$(decthemectl theme install "$TMP/work/evil" 2>&1); has "a script in the package is refused" "$out" "run.sh"
printf 'format = 1\n[root]\ntype = "button"\non_click = "exec touch /tmp/PWNED"\n' > "$TMP/work/evil/widgets/x.toml"; rm "$TMP/work/evil/run.sh"
out=$(decthemectl theme install "$TMP/work/evil" 2>&1); has "a widget with exec is refused" "$out" "cannot run commands"
ln -s /etc/passwd "$TMP/work/evil2/x.png"; cp "$PKG/theme.toml" "$TMP/work/evil2/"
out=$(decthemectl theme install "$TMP/work/evil2" 2>&1); has "a symlink is refused" "$out" "symlink"
tar czf "$TMP/work/trav.dtheme" -C "$TMP/work" --transform 's#^evil/#../../x/#' evil/theme.toml 2>/dev/null
out=$(decthemectl theme install "$TMP/work/trav.dtheme" 2>&1); has "a path-traversal archive is refused" "$out" "escapes\|not\|missing"
[ -e /tmp/PWNED ] && fail "something ran" || pass "nothing was executed"
decthemectl theme list | grep -q "evil" && fail "bad theme got installed" || pass "bad themes are not installed"
cp "$TMP/work/sunset-grid.dtheme" "$TMP/work/tampered.dtheme"
python3 -I - "$TMP/work" <<'PY'
import sys, tarfile, io, gzip
d = sys.argv[1]
src = tarfile.open(d + "/sunset-grid.dtheme")
out = io.BytesIO()
with tarfile.open(fileobj=out, mode="w") as t:
    for m in src.getmembers():
        f = src.extractfile(m).read()
        if m.name.endswith("sunset-clock.toml"):
            f = f.replace(b"56", b"57")
        m.size = len(f); t.addfile(m, io.BytesIO(f))
open(d + "/tampered.dtheme", "wb").write(gzip.compress(out.getvalue()))
PY
decthemectl theme uninstall sunset-grid >/dev/null
out=$(decthemectl theme install "$TMP/work/tampered.dtheme" 2>&1); has "a modified package fails its content hash" "$out" "content hash mismatch"
decthemectl theme install "$TMP/work/sunset-grid.dtheme" >/dev/null

echo "== apply: rdwm, dock, wallpaper, widgets"
before_cfg=$(cat "$CFG")
out=$(decthemectl theme apply sunset-grid 2>&1); has "apply" "$out" "applied sunset-grid"
eq "selection is the package" "$(sel_name)" sunset-grid
eq "rdwm gap_inner from the manifest" "$(rdcfg .config.general.gap_inner)" 10
eq "rdwm border_px from the manifest" "$(rdcfg .config.general.border_px)" 2
eq "rdwm wallpaper image is the bundled file" "$(rdcfg .config.wallpaper.image)" "$THEMES/sunset-grid/wallpapers/sunset.jpg"
eq "bar style reaches the resolved theme" "$(decthemectl get --json | jq -r .bar_style)" floating
style=$(rdcfg .config.decoration.style)
if [ "$style" = adwaita ]; then pass "titlebar style adwaita"; else has "an rdwm without that style says so instead of failing" "$out" "decoration.style"; fi
grep -q "# test config" "$CFG" && pass "comments in rdwm's config survived" || fail "comment lost"
eq "hot corner from the manifest" "$(jq -r .hot_corners.top_right "$DECEBAL_CONFIG/decebal.json")" dashboard
eq "three widget instances recorded" "$(jq '.widgets.items|length' "$DECEBAL_CONFIG/decebal.json")" 3
eq "first widget points at the package" "$(jq -r '.widgets.items[0].source' "$DECEBAL_CONFIG/decebal.json")" sunset-grid
shot 10-applied-no-widgets-yet

echo "== desktop widgets"
decwidgets >"$TMP/decwidgets.log" 2>&1 & WP=$!; PIDS+=($WP)
wait_for 8 bash -c "[ \$(xdotool search --class decwidgets 2>/dev/null | wc -l) -ge 3 ]" && pass "three widget windows" || fail "widget windows" "$(widget_windows) $(cat "$TMP/decwidgets.log")"
sleep 1.2
shot 11-desktop-widgets
# the clock card sits 48 px from the right edge, 64 px from the top: its background is the theme's popup colour
eqpx "clock card painted with the theme's popup colour" 1500 80 "#2e2050"
[ "$(px 400 600)" != "#46607a" ] && pass "the bundled wallpaper replaced the flat colour" || fail "wallpaper not applied"
echo "-- click on the clock card opens the dashboard"
xdotool mousemove 1450 120; sleep 0.2; xdotool click 1; sleep 1.5
kids=$(pgrep -P "$WP" -x decdash | head -1)
[ -n "$kids" ] && pass "clicking the clock card started decdash" || fail "no decdash after the click"
shot 12-after-click
[ -n "$kids" ] && kill "$kids" 2>/dev/null
xdotool key Escape; sleep 0.3

echo "== live edit of a widget file"
sed -i 's/size = 56/size = 40/' "$THEMES/sunset-grid/widgets/sunset-clock.toml"
sleep 2
shot 13-widget-edited
kill -0 $WP 2>/dev/null && pass "decwidgets survived the edit" || fail "decwidgets died"
echo "-- light mode follows"
decthemectl apply sunset-grid --mode light >/dev/null 2>&1; sleep 1.5
eqpx "light popup colour in the clock card" 1500 80 "#fffaf5"
shot 14-light
decthemectl apply sunset-grid --mode dark >/dev/null 2>&1; sleep 1

echo "== undo and preview"
decthemectl theme undo >/dev/null 2>&1
sleep 1.5
eq "undo: widgets gone" "$(widget_windows)" 0
eq "undo: selection back to the start" "$(sel_name)" none
eq "undo: rdwm gap_inner back" "$(rdcfg .config.general.gap_inner)" 6
[ "$(cat "$CFG")" = "$before_cfg" ] && pass "undo: config.toml restored byte for byte" || fail "undo: config differs"
decthemectl theme apply nord --preview 3 >"$TMP/preview.log" 2>&1 & PV=$!; PIDS+=($PV)
sleep 1.2; eq "preview applies right away" "$(sel_name)" nord
wait $PV 2>/dev/null; sleep 0.5
eq "preview reverts by itself" "$(sel_name)" none
[ "$(cat "$CFG")" = "$before_cfg" ] && pass "preview: config.toml restored" || fail "preview: config differs"

echo "== snapshot, duplicate, export"
decthemectl theme apply nord >/dev/null 2>&1
decthemectl theme snapshot my-nord --name "My Nord" | grep -q created && pass "snapshot created" || fail "snapshot"
decthemectl theme validate "$THEMES/my-nord" | grep -q "^ok" && pass "snapshot validates" || fail "snapshot invalid"
decthemectl theme duplicate my-nord my-nord-2 >/dev/null && pass "duplicate" || fail "duplicate"
decthemectl theme export my-nord "$TMP/work/my-nord.dtheme" >/dev/null && decthemectl theme validate "$TMP/work/my-nord.dtheme" | grep -q "^ok" && pass "export validates" || fail "export"
decthemectl theme uninstall sunset-grid >/dev/null && pass "uninstall" || fail "uninstall"
decthemectl theme uninstall dracula 2>&1 | grep -q "built into" && pass "built-ins cannot be uninstalled" || fail "uninstalled a built-in"
decthemectl theme install "$TMP/work/sunset-grid.dtheme" >/dev/null

echo "== the Theme pane gallery"
export DECSETTINGS_DEBUG_LAYOUT=$TMP/dump.json
decthemectl theme apply nord >/dev/null 2>&1
decsettings --config "$CFG" theme >"$TMP/ms.log" 2>&1 & PIDS+=($!)
wait_for 8 test -s "$DECSETTINGS_DEBUG_LAYOUT" || fail "settings dump never written"
sleep 1
WID=$(xdotool search --sync --class DecSettings 2>/dev/null | head -1)
[ -n "$WID" ] && pass "settings window up" || fail "no settings window"
q() { jq -r "$1" "$DECSETTINGS_DEBUG_LAYOUT" 2>/dev/null; }
scroll_to() { for i in $(seq 1 $1); do xdotool mousemove --window "$WID" 500 500; xdotool click --window "$WID" 5; done; sleep 0.5; }
btn() { jq -r "[.layouts[]|select(.id==\"$1\")][0].rect|\"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)\"" "$DECSETTINGS_DEBUG_LAYOUT"; }
click() { xdotool mousemove --window "$WID" $1 $2; sleep 0.1; xdotool click --window "$WID" 1; sleep 0.8; }
# the pane scrolls with an animation: read the button's place again until it stops moving, then click its centre
click_btn() { local a b i; a=$(btn "$1"); for i in 1 2 3 4 5 6 7 8 9 10; do sleep 0.4; b=$(btn "$1"); [ "$a" = "$b" ] && break; a=$b; done; click $b; }
card_y() { jq -r "[.layouts[]|select(.id==\"$1\")][0].rect[1]" "$DECSETTINGS_DEBUG_LAYOUT" 2>/dev/null; }
# scroll (either way) until the card's buttons are inside the window
reveal() {
  local i y n
  for i in $(seq 1 40); do
    sleep 0.5
    y=$(card_y "l:tcard:$1"); y=${y%.*}
    if [ -n "$y" ] && [ "$y" != null ] && [ "$y" -gt 120 ] 2>/dev/null && [ "$y" -lt 700 ]; then return; fi
    n=5; [ -n "$y" ] && [ "$y" != null ] && [ "$y" -le 120 ] 2>/dev/null && n=4
    xdotool mousemove --window "$WID" 500 500; xdotool click --window "$WID" $n
  done
}
reveal apply:sunset-grid
eq "a card for the installed theme has an Apply button" "$(q '[.layouts[]|select(.id=="l:tcard:apply:sunset-grid")]|length')" 1
eq "built-in cards offer no Delete" "$(q '[.layouts[]|select(.id=="l:tcard:delete:nord")]|length')" 0
eq "the user theme card offers Delete" "$(q '[.layouts[]|select(.id=="l:tcard:delete:sunset-grid")]|length')" 1
shot 20-pane-cards
reveal apply:sunset-grid; click_btn l:tcard:apply:sunset-grid; sleep 1
eq "Apply on the card applies the whole package" "$(sel_name)" sunset-grid
eq "...including the rdwm gap" "$(rdcfg .config.general.gap_inner)" 10
shot 21-pane-after-apply
reveal preview:dracula; click_btn l:tcard:preview:dracula; sleep 1
eq "Preview applies temporarily" "$(sel_name)" dracula
sleep 9.5
eq "...and reverts" "$(sel_name)" sunset-grid
echo "-- delete needs two clicks"
reveal delete:my-nord
click_btn l:tcard:delete:my-nord
[ -d "$THEMES/my-nord" ] && pass "first click only arms Delete" || fail "deleted on the first click"
click_btn l:tcard:delete:my-nord
[ -d "$THEMES/my-nord" ] && fail "second click did not delete" || pass "second click deletes"
shot 22-pane-after-delete

echo "== the bundled asset themes (themes/<id>: wallpaper, widgets, screenshot)"
# They are installed system-wide by `make install`; here the repository's themes/ folder plays /usr/share/decebal/themes.
export DECEBAL_SYSTEM_THEMES=$HERE/themes
OLDWP=$WP; kill "$WP" 2>/dev/null; wait "$WP" 2>/dev/null; decwidgets >"$TMP/decwidgets2.log" 2>&1 & WP=$!; PIDS+=($WP)   # the daemon reads the variable at start
declare -A WSTYLE=([nothing-dot]=classic [nothing-light]=classic [brutalist]=classic [terminal-minimal]=minimal [terminal-amber]=minimal)
for id in nothing-dot nothing-light brutalist terminal-minimal terminal-amber; do
  out=$(decthemectl theme validate "$HERE/themes/$id" 2>&1); has "$id validates as a package" "$out" "^ok:"
  decthemectl theme list | grep -q "^$id\b.*system\|$id.*system" && pass "$id is listed from the system folder" || fail "$id list" "$(decthemectl theme list | grep "$id")"
  out=$(decthemectl theme apply "$id" 2>&1); has "$id applies" "$out" "applied $id"
  eq "$id: selection" "$(sel_name)" "$id"
  case "$out" in *"is missing from the package"*|*"cannot bundle"*) fail "$id: wallpaper not found" ;; *) pass "$id: wallpaper found" ;; esac
  case "$(rdcfg .config.wallpaper.image)" in */$id/wallpapers/*.png) pass "$id: wallpaper path points into the package" ;; *) fail "$id: wallpaper path" "$(rdcfg .config.wallpaper.image)" ;; esac
  eq "$id: titlebar style" "$(rdcfg .config.decoration.style)" "${WSTYLE[$id]}"
  eq "$id: widgets placed" "$(jq '[.widgets.items[]|select(.from_theme==true)]|length' "$DECEBAL_CONFIG/decebal.json")" "$([ "$id" = nothing-dot ] || [ "$id" = nothing-light ] && echo 3 || echo 1)"
  want=1; [ "$id" = nothing-dot ] || [ "$id" = nothing-light ] && want=3
  end=$((SECONDS + 15)); while [ $SECONDS -lt $end ] && [ "$(widget_windows)" != "$want" ]; do sleep 0.5; done
  eq "$id: its widget windows appear" "$(widget_windows)" "$want"
  [ "$(widget_windows)" = "$want" ] || tail -3 "$TMP/decwidgets2.log"
  shot "theme-$id"
done
decthemectl theme apply nord >/dev/null 2>&1; unset DECEBAL_SYSTEM_THEMES; export DECEBAL_SYSTEM_THEMES=$TMP/nosystem

echo "== palettes are separate from themes"
decthemectl palette list | grep -q "catppuccin.*dark+light.*built-in" && pass "palette list" || fail "palette list"
for th in nord brutalist terminal-minimal; do
  decthemectl apply "$th" --mode dark >/dev/null 2>&1; decthemectl palette catppuccin >/dev/null 2>&1
  eq "$th under catppuccin: background" "$(decthemectl get --json | jq -r .colors.background)" "#1e1e2e"
  eq "$th under catppuccin: rdwm focused border is the palette accent" "$(rdcfg .config.colors.focused)" "#cba6f7"
done
decthemectl apply nord --mode dark >/dev/null 2>&1
eq "the palette survives a theme switch" "$(jq -r .theme.palette "$DECEBAL_CONFIG/decebal.json")" catppuccin
decthemectl palette nope >/dev/null 2>&1 && fail "an unknown palette was accepted" || pass "an unknown palette is refused"
decthemectl palette none >/dev/null 2>&1
eq "palette none gives the theme's own colours back" "$(decthemectl get --json | jq -r .colors.background)" "#2e3440"

echo "== debug logs (DECEBAL_LOG=debug, one file per program)"
out=$(decthemectl log status 2>&1); has "log status names the folder" "$out" "$TMP/logs"
for prog in decthemectl decsettings decbar decdock decwidgets; do
  [ -s "$TMP/logs/$prog.log" ] && pass "$prog wrote its log" || fail "$prog has no log" "$(ls "$TMP/logs" 2>&1 | tr '\n' ' ')"
done
grep -q "theme apply .nothing-dot" "$TMP/logs/decthemectl.log" && pass "theme applies are logged" || fail "no apply record"
grep -q "INFO  .* decsettings .*start: pane" "$TMP/logs/decsettings.log" && pass "decsettings logs its start" || fail "decsettings log content" "$(tail -3 "$TMP/logs/decsettings.log")"
out=$(decthemectl log tail 5 2>&1); [ "$(printf '%s\n' "$out" | wc -l)" = 5 ] && pass "log tail merges the programs" || fail "log tail" "$out"
decthemectl log level info >/dev/null && jq -e '.debug.level=="info"' "$DECEBAL_CONFIG/decebal.json" >/dev/null && pass "log level is persisted" || fail "log level"
decthemectl log clear >/dev/null; [ -z "$(ls "$TMP/logs" 2>/dev/null)" ] && pass "log clear" || fail "log clear left files"

echo "== no crashes"
for p in "${PIDS[@]}"; do kill -0 "$p" 2>/dev/null || { [ "$p" = "$PV" ] || [ "$p" = "${OLDWP:-}" ] || fail "process $p died"; }; done
grep -il "panicked" "$TMP"/*.log 2>/dev/null | head -3 | while read -r f; do fail "panic in $f"; done
echo; echo "passed $PASS, failed $FAIL (screenshots in $SHOTS)"
[ "$FAIL" = 0 ]
