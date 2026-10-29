#!/usr/bin/env bash
# The Appearance pane end to end on a private Xvfb + rdwm: every old pane name still opens the right tab, and search finds
# each moved setting and opens its tab and section. Usage: tests/e2e/appearance.sh [display]   (SHOTS=dir keeps screenshots)
set -u
OLDHOME=$HOME; export OLDHOME
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
source tests/e2e/env.sh
trap e2e_down EXIT
e2e_up "${1:-:97}"
DUMP="$E2E_T/dump.json"; export DECSETTINGS_DEBUG_LAYOUT="$DUMP"
PASS=0; FAIL=0
ok() { PASS=$((PASS+1)); echo "  ok   $*"; }
bad() { FAIL=$((FAIL+1)); echo "  FAIL $*"; }
q() { jq -r "$1" "$DUMP" 2>/dev/null; }
view() { q '.pane + "/" + .tab + "/" + .section'; }
shot() { [ -n "${SHOTS:-}" ] && { mkdir -p "$SHOTS"; e2e_shot "$SHOTS/$1.png"; }; }

echo "== old pane names open something sensible"
# name  expected(pane/tab/section; the section only matters in Customise)
for spec in "theme:appearance/theme" "appearance:appearance/theme" "icons:appearance/customise/icons" "desktop:appearance/customise/dock" \
            "palettes:appearance/palettes" "wallpaper:appearance/wallpaper" "appearance/fonts:appearance/customise/fonts" "appearance/window:appearance/customise/window" \
            "appearance/colours:appearance/palettes" "system-theme:appearance/apps" "config:config" "general:general" "tiling:tiling"; do
  name=${spec%%:*}; want=${spec##*:}
  rm -f "$DUMP"
  decsettings --config "$E2E_CFG" "$name" >"$E2E_T/ds.log" 2>&1 & pid=$!
  for _ in $(seq 1 60); do [ -s "$DUMP" ] && break; sleep 0.1; done
  sleep 0.5
  got=$(view)
  case $want in
    appearance/theme|appearance/palettes|appearance/wallpaper|appearance/apps) got=${got%/*}; [ "$(q .pane)/$(q .tab)" = "$want" ] && got=$want ;;
    config|general|tiling) got=$(q .pane) ;;
  esac
  [ "$got" = "$want" ] && ok "decsettings $name -> $want" || bad "decsettings $name -> $got (want $want)"
  [ "$name" = icons ] && shot appearance-customise-icons
  kill $pid 2>/dev/null; wait $pid 2>/dev/null; sleep 0.3
done

echo "== search finds every moved setting and opens its tab"
rm -f "$DUMP"
decsettings --config "$E2E_CFG" general >"$E2E_T/ds.log" 2>&1 & APP=$!
for _ in $(seq 1 60); do [ -s "$DUMP" ] && break; sleep 0.1; done; sleep 0.5
WID=$(xdotool search --sync --class DecSettings 2>/dev/null | head -1)
xdotool mousemove --window "$WID" 400 300
# query | label shown in the results | tab/section it opens
for spec in "accent colour|Accent colour|palettes" "translucency|Translucency|customise/colours" "text size|Text size|customise/fonts" \
            "monospace|Monospace font|customise/fonts" "green button|Green button|customise/window" "window border|Window border|customise/window" \
            "magnification|Magnification|customise/dock" "pinned|Pinned apps|customise/dock" "hot corner|Hot corners|customise/dock" \
            "icon theme|Icon theme|customise/icons" "tint|Tint symbolic icons|customise/icons" "wallhaven|Wallhaven|wallpaper" \
            "scaling|Scaling|wallpaper" "match wallpaper|Match wallpaper colours|palettes" "light dark|Mode|palettes" \
            "desktop design|Themes|theme" "gtk|GTK apps|apps" "apply to all|Apply theme to all apps|apps" "chrome|Browser colour|apps"; do
  query=${spec%%|*}; rest=${spec#*|}; label=${rest%%|*}; where=${rest##*|}
  xdotool key ctrl+f; sleep 0.2
  xdotool key --repeat 30 BackSpace; sleep 0.1; xdotool type --delay 20 "$query"; sleep 0.7
  if jq -e --arg l "$label" '.hit_labels|index($l)' "$DUMP" >/dev/null 2>&1; then
    ok "search '$query' lists '$label'"
  else
    bad "search '$query' does not list '$label' ($(q '.hit_labels|tostring'))"
  fi
  # Enter opens the first hit; check the best hit is in the expected place when it is the one we asked for
  first=$(q '.hit_labels|map(select(. != null))|first // ""')
  if [ "$first" = "$label" ]; then
    xdotool key Return; sleep 0.6
    case $where in theme|palettes|wallpaper|apps) [ "$(q .tab)" = "$where" ] && ok "  Enter opens tab $where" || bad "  Enter opened $(q .tab) (want $where)";;
      *) [ "$(q .tab)/$(q .section)" = "$where" ] && ok "  Enter opens $where" || bad "  Enter opened $(q .tab)/$(q .section) (want $where)";; esac
  fi
done
shot appearance-after-search
kill $APP 2>/dev/null; wait $APP 2>/dev/null
echo "appearance: $PASS ok, $FAIL failed"
[ $FAIL -eq 0 ]
