#!/usr/bin/env bash
# "Apply to all apps" end to end, in a PRIVATE environment:
#   tests/e2e/system-theme.sh        (SHOTS=dir keeps the PNGs; E2E_DISPLAY=:92 by default)
# Own HOME / XDG_* / XDG_RUNTIME_DIR (a fake ~/.config with gtk, qt6ct, alacritty, foot, kitty and Xresources), a private Xvfb, a private
# D-Bus session and the keyfile gsettings backend. OMARCHY_PATH is only read. Omarchy's reload helpers that act on running processes
# (btop, foot, the browser policy that needs sudo) are replaced by logging shims, so nothing outside this directory is touched.
# Only PIDs started here are killed.
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
T=${CARGO_TARGET_DIR:-$HERE/target}
BIN=${MACSHELL_BIN:-$T/${PROFILE:-$([ -x "$T/release/macthemectl" ] && echo release || echo debug)}}
[ -x "$BIN/macthemectl" ] || { (cd "$HERE" && cargo build -q --offline --bins) || { echo "build failed"; exit 2; }; }
N=${E2E_DISPLAY:-:92}
[ "$N" = ":1" ] && { echo "refusing the live display"; exit 2; }
# never share a display with somebody else's server
if [ -e "/tmp/.X11-unix/X${N#:}" ] || [ -e "/tmp/.X${N#:}-lock" ]; then echo "display $N is in use; set E2E_DISPLAY to a free one"; exit 77; fi
for t in Xvfb xrdb import dbus-run-session gsettings python3; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
OMARCHY=${OMARCHY_PATH:-/usr/share/omarchy}
[ -d "$OMARCHY/default/themed" ] || OMARCHY=

PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/macshell-system-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; sleep 0.3; for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done; wait 2>/dev/null
  [ "$SHOTS" = "$TMP/shots" ] && rm -rf "$TMP" || { find "$TMP" -mindepth 1 -maxdepth 1 -exec rm -rf {} +; rmdir "$TMP" 2>/dev/null; }; }
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "got '$2', want '$3'"; fi; }
has() { if grep -q -- "$3" "$2" 2>/dev/null; then pass "$1"; else fail "$1" "no '$3' in $2"; fi; }
hasnt() { if grep -q -- "$3" "$2" 2>/dev/null; then fail "$1" "unexpected '$3' in $2"; else pass "$1"; fi; }

# ---- the private world
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE DBUS_SESSION_BUS_ADDRESS SWAYSOCK DISPLAY RDWM_SOCK KITTY_LISTEN_ON QT_QPA_PLATFORMTHEME
export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/home/.config XDG_STATE_HOME=$TMP/home/.local/state XDG_DATA_HOME=$TMP/data XDG_CACHE_HOME=$TMP/cache XDG_RUNTIME_DIR=$TMP/run
export MACSHELL_CONFIG=$XDG_CONFIG_HOME/omarchy MACSHELL_THEMES=$XDG_CONFIG_HOME/macshell/themes MACSHELL_SYSTEM_SYNC=1 MACSHELL_COLOR_SCHEME=dark
export GSETTINGS_BACKEND=keyfile RDWM_CONFIG=$XDG_CONFIG_HOME/rdwm/config.toml RDWMCTL=/nonexistent
[ -n "$OMARCHY" ] && export OMARCHY_PATH=$OMARCHY
mkdir -p "$XDG_CONFIG_HOME"/{gtk-3.0,gtk-4.0,qt6ct,alacritty,foot,kitty,omarchy,rdwm} "$XDG_STATE_HOME/omarchy/current" "$XDG_RUNTIME_DIR" "$TMP/shims" "$XDG_DATA_HOME" "$HOME/Documents"
chmod 700 "$XDG_RUNTIME_DIR"
for s in omarchy-restart-btop omarchy-theme-set-foot; do printf '#!/bin/sh\necho "%s $*" >> "$HOME/shim.log"\n' $s > "$TMP/shims/$s"; chmod +x "$TMP/shims/$s"; done
printf '#!/bin/sh\necho "browser: $(cat "$HOME/.local/state/omarchy/current/theme/chromium.theme")" >> "$HOME/shim.log"\n' > "$TMP/shims/omarchy-theme-set-browser"
chmod +x "$TMP/shims/omarchy-theme-set-browser"
export PATH=$TMP/shims:$BIN:$PATH
CFG=$XDG_CONFIG_HOME
printf '/* my own css */\nbutton { border-radius: 2px; }\n' > $CFG/gtk-3.0/gtk.css
printf '[Settings]\ngtk-font-name=Cantarell 11\n' > $CFG/gtk-3.0/settings.ini
printf '[Appearance]\nstyle=Breeze\n' > $CFG/qt6ct/qt6ct.conf
printf '[general]\nimport = ["~/.local/state/omarchy/current/theme/alacritty.toml"]\n' > $CFG/alacritty/alacritty.toml
echo '! mine' > $HOME/.Xresources
echo tokyo-night > $XDG_STATE_HOME/omarchy/current/theme.name
snapshot() { (cd "$HOME" && find .config/gtk-3.0 .config/gtk-4.0 .config/qt6ct .config/alacritty .config/omarchy/themes .config/macshell .Xresources -type f ! -name bookmarks -print0 2>/dev/null | sort -z | xargs -0 sha256sum); }
BEFORE=$(snapshot)
ctl() { macthemectl "$@"; }
sysval() { sed -n "s/^@define-color $1 \(#[0-9a-f]*\);.*/\1/p" "$CFG/gtk-4.0/gtk.css" | head -1; }

echo "== off by default: nothing outside macshell changes"
ctl apply nord --mode dark --no-rdwm >/dev/null
[ ! -e $CFG/gtk-4.0/gtk.css ] && pass "no gtk.css before 'system on'" || fail "gtk.css written while off"
eq "snapshot unchanged" "$(snapshot)" "$BEFORE"
[[ "$(ctl system status)" == "Apply to all apps: off"* ]] && pass "status says off" || fail "status"
ctl apply-system --dry-run >/dev/null && [ ! -e $CFG/gtk-4.0/gtk.css ] && pass "--dry-run writes nothing" || fail "dry-run wrote"

echo "== system on"
ctl system on >"$TMP/on.log" 2>&1 || { fail "system on" "$(cat "$TMP/on.log")"; }
has "gtk-4.0 gtk.css has the managed block" $CFG/gtk-4.0/gtk.css "macshell system theme"
has "user css kept" $CFG/gtk-3.0/gtk.css "my own css"
has "libadwaita accent from the theme" $CFG/gtk-4.0/gtk.css "@define-color accent_bg_color #88c0d0;"
has "gtk3 widget rules" $CFG/gtk-3.0/gtk.css "button.suggested-action"
has "settings.ini dark" $CFG/gtk-4.0/settings.ini "gtk-application-prefer-dark-theme=1"
has "settings.ini keeps the user's font" $CFG/gtk-3.0/settings.ini "gtk-font-name=Cantarell 11"
has "qt6ct scheme file" $CFG/qt6ct/colors/macshell.conf "^active_colors=#ff"
has "qt6ct.conf selects it, keeps style" $CFG/qt6ct/qt6ct.conf "style=Breeze"
has "qt6ct.conf custom palette" $CFG/qt6ct/qt6ct.conf "custom_palette=true"
has "Xresources included" $HOME/.Xresources '#include'
eq "gsettings color-scheme" "$(gsettings get org.gnome.desktop.interface color-scheme)" "'prefer-dark'"
if [ -n "$OMARCHY" ]; then
  T=$XDG_STATE_HOME/omarchy/current/theme
  eq "omarchy current theme" "$(cat $XDG_STATE_HOME/omarchy/current/theme.name)" "macshell"
  for f in colors.toml alacritty.toml foot.ini kitty.conf ghostty.conf btop.theme chromium.theme helix.toml neovim.lua obsidian.css vscode-theme.json; do
    [ -s "$T/$f" ] && pass "omarchy rendered $f" || fail "omarchy did not render $f"
  done
  has "alacritty background is the theme's" $T/alacritty.toml 'background = "#2e3440"'
  has "foot background" $T/foot.ini 'background=2e3440'
  has "kitty accent" $T/kitty.conf 'active_border_color #88c0d0'
  has "browser colour handed to Omarchy's helper" $HOME/shim.log 'browser: 46,52,64'
  has "btop reload helper ran" $HOME/shim.log 'omarchy-restart-btop'
  has "foot OSC helper ran" $HOME/shim.log 'omarchy-theme-set-foot'
  [ -e $XDG_CONFIG_HOME/omarchy/themes/macshell/colors.toml ] && pass "theme dir written" || fail "theme dir"
  hasnt "no signals by default" $HOME/shim.log 'restart-terminal'
else
  echo "  skip Omarchy checks (no OMARCHY_PATH)"
fi

echo "== idempotent, and the theme change follows"
H1=$(snapshot)
ctl apply-system | grep -q "nothing to change" && pass "second apply changes nothing" || fail "second apply wrote"
eq "snapshot identical" "$(snapshot)" "$H1"
ctl apply "macOS" --mode light --no-rdwm >/dev/null
has "light: prefer-light in settings.ini" $CFG/gtk-4.0/settings.ini "gtk-application-prefer-dark-theme=0"
eq "light: gsettings follows" "$(gsettings get org.gnome.desktop.interface color-scheme)" "'prefer-light'"
has "macOS style: window buttons on the left" $CFG/gtk-4.0/settings.ini "gtk-decoration-layout=close,minimize,maximize:"
ctl apply windows11 --mode light --no-rdwm >/dev/null
has "Windows 11: window buttons on the right" $CFG/gtk-4.0/settings.ini "appmenu:minimize,maximize,close"
has "Windows 11 font reaches GTK" $CFG/gtk-4.0/settings.ini "gtk-font-name=Segoe UI"
ctl apply nord --mode dark --no-rdwm >/dev/null
ctl system target qt off >/dev/null
[ ! -e $CFG/qt6ct/colors/macshell.conf ] && pass "unchecking Qt restores its files" || fail "qt file still there"
has "qt6ct.conf back to the original" $CFG/qt6ct/qt6ct.conf "style=Breeze"
hasnt "no custom_palette left" $CFG/qt6ct/qt6ct.conf "custom_palette"
ctl system target qt default >/dev/null

echo "== debounced requests (no MACSHELL_SYSTEM_SYNC): several changes make one apply"
(
  unset MACSHELL_SYSTEM_SYNC
  ctl apply dracula --no-rdwm >/dev/null; ctl apply solarized --mode dark --no-rdwm >/dev/null; ctl apply nord --mode dark --radius 6 --no-rdwm >/dev/null
)
end=$((SECONDS + 10)); while [ $SECONDS -lt $end ] && ! grep -q '"nord"\|nord' <(ctl system status --json | grep applied_theme); do sleep 0.3; done
eq "the last change won" "$(ctl system status --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["applied_theme"])')" "nord"

# ---- real GTK 4 / libadwaita and GTK 3 windows + Nautilus on a private Xvfb
Xvfb $N -screen 0 1600x900x24 -noreset -nolisten tcp >"$TMP/xvfb.log" 2>&1 & PIDS+=($!)
for i in $(seq 100); do [ -e /tmp/.X11-unix/X${N#:} ] && break; sleep 0.2; done; sleep 1
kill -0 "${PIDS[-1]}" 2>/dev/null || { echo "Xvfb $N did not start"; tail -3 "$TMP/xvfb.log"; exit 2; }
export DISPLAY=$N GDK_BACKEND=x11
px() { import -window root -crop 1x1+$1+$2 +repage -depth 8 txt:- 2>/dev/null | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1 | tr 'A-F' 'a-f'; }
echo "== X resources and GTK windows on $N"
ctl apply-system >/dev/null
xrdb -query | grep -q '^\*\.background:.*#2e3440' && pass "xrdb has the theme's background" || fail "xrdb background" "$(xrdb -query | head -3)"
xrdb -query | grep -q '^\*\.color1:' && pass "xrdb has the ANSI colours" || fail "xrdb colour1"
DEMO=$HERE/tests/e2e/fakes/gtk_demo.py
shoot() {
  local name=$1
  dbus-run-session -- bash -c "
    python3 '$DEMO' 4 'GTK4 + libadwaita' & A=\$!
    python3 '$DEMO' 3 'GTK3' & B=\$!
    sleep 3.5
    xdotool search --name 'GTK4' windowmove 0 0 2>/dev/null
    xdotool search --name 'GTK3' windowmove 860 0 2>/dev/null
    nautilus --new-window '$HOME' >/dev/null 2>&1 & C=\$!
    sleep 5
    xdotool search --name '^Home\$' windowmove 0 400 windowsize 1000 480 2>/dev/null
    sleep 1.5
    import -window root '$SHOTS/$name.png'
    PX_SIDEBAR=\$(import -window root -crop 1x1+100+300 +repage -depth 8 txt:- | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1)
    PX_GTK3=\$(import -window root -crop 1x1+1200+300 +repage -depth 8 txt:- | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1)
    PX_NAUTILUS=\$(import -window root -crop 1x1+700+800 +repage -depth 8 txt:- | tail -1 | grep -o '#[0-9A-F]\{6\}' | head -1)
    echo \"\$PX_SIDEBAR \$PX_GTK3 \$PX_NAUTILUS\" > '$TMP/px.txt'
    kill \$A \$B \$C 2>/dev/null; sleep 0.5
  " >/dev/null 2>&1
  tr 'A-F' 'a-f' < "$TMP/px.txt"
}
if command -v nautilus >/dev/null && command -v xdotool >/dev/null && python3 -I -c "import gi; gi.require_version('Adw','1')" 2>/dev/null; then
  read -r sb g3 nt <<<"$(shoot nord-dark)"
  eq "GTK4 sidebar is sidebar_bg_color (nord dark)" "$sb" "$(sysval sidebar_bg_color)"
  eq "GTK3 window is window_bg_color" "$g3" "$(sysval window_bg_color)"
  eq "Nautilus content is the window colour" "$nt" "$(sysval window_bg_color)"
  ctl apply "macOS" --mode light --no-rdwm >/dev/null
  read -r sb g3 nt <<<"$(shoot macos-light)"
  eq "light: GTK4 sidebar" "$sb" "$(sysval sidebar_bg_color)"
  eq "light: GTK3 window" "$g3" "$(sysval window_bg_color)"
  ctl apply caelestia --mode dark --no-rdwm >/dev/null
  read -r sb g3 nt <<<"$(shoot caelestia-dark)"
  eq "caelestia: Nautilus content" "$nt" "$(sysval window_bg_color)"
  ctl apply windows11 --mode light --no-rdwm >/dev/null
  shoot windows11-light >/dev/null
else
  echo "  skip GTK windows (need nautilus, xdotool, PyGObject with libadwaita)"
fi

echo "== off puts everything back"
ctl system off >"$TMP/off.log" 2>&1
eq "files identical to the start" "$(snapshot)" "$BEFORE"
[ ! -e $CFG/gtk-4.0/gtk.css ] && pass "created gtk.css removed" || fail "gtk-4.0/gtk.css left"
[ ! -d $CFG/qt6ct/colors ] && pass "created qt6ct/colors removed" || fail "qt6ct/colors left"
[ ! -d $XDG_CONFIG_HOME/omarchy/themes/macshell ] && pass "omarchy theme dir removed" || fail "theme dir left"
[ -z "$(xrdb -query)" ] && pass "xrdb database back to empty" || fail "xrdb still has keys" "$(xrdb -query | head -2)"
[ -n "$OMARCHY" ] && eq "previous Omarchy theme current again" "$(cat $XDG_STATE_HOME/omarchy/current/theme.name)" "tokyo-night"
[[ "$(ctl system status)" == "Apply to all apps: off"* ]] && pass "status says off again" || fail "status after off"
ctl system off >/dev/null 2>&1 && pass "off twice is harmless" || fail "second off failed"

echo "== the GUI switch (macsettings unit tests cover the pane); screenshots in $SHOTS"
echo "$PASS passed, $FAIL failed"
[ $FAIL -eq 0 ]
