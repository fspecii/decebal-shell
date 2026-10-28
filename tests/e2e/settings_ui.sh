# Shared by the end-to-end scripts that drive decsettings with xdotool (source it after WID, DECSETTINGS_DEBUG_LAYOUT and `fail` exist).
#
# The panes keep growing sections, so nothing is clicked at a fixed position: every control is looked up in the layout dump
# (window coordinates; a control scrolled out of view has a rect above or below the window) and the pane is scrolled with
# the mouse wheel until the control is inside the visible content before it is clicked.

# rect_of ID: "x y w h" of the layout tile / control / its first part with that id (empty when the pane has no such control)
rect_of() { jq -r --arg id "$1" '([.layouts[]|select(.id==$id)|.rect][0] // [.ctls[]|select(.id==$id)|(.parts[0] // .rect)][0]) // empty | "\(.[0]|floor) \(.[1]|floor) \(.[2]|floor) \(.[3]|floor)"' "$DECSETTINGS_DEBUG_LAYOUT" 2>/dev/null; }

win_geometry() { xdotool getwindowgeometry "$WID" 2>/dev/null | sed -n 's/.*Geometry: \([0-9]*\)x\([0-9]*\).*/\1 \2/p'; }

# wheel BUTTON: one wheel notch (5 = down, 4 = up) over the middle of the content, then wait for the repaint
wheel() {
  local w h; read -r w h <<<"$(win_geometry)"
  xdotool mousemove --window "$WID" $((w * 2 / 3)) $((h / 2)); sleep 0.05; xdotool click --window "$WID" "$1"; sleep 0.25
}

# reveal ID: scroll until the centre of ID is inside the content (below the pane title, above the window's bottom edge) and
# echo that centre as "x y"; fails after 60 notches or when the pane has no such control
reveal() {
  local r x y w h cy i ww wh
  read -r ww wh <<<"$(win_geometry)"
  for i in $(seq 1 60); do
    r=$(rect_of "$1"); [ -z "$r" ] && { sleep 0.2; continue; }
    read -r x y w h <<<"$r"; cy=$((y + h / 2))
    if [ "$cy" -gt $((wh - 30)) ]; then wheel 5; elif [ "$cy" -lt 90 ]; then wheel 4; else echo "$((x + w / 2)) $cy"; return 0; fi
  done
  return 1
}

click() { xdotool mousemove --window "$WID" $1 $2; sleep 0.1; xdotool click --window "$WID" 1; sleep 0.5; }

# click_ctl ID: reveal the control and click its centre (the position is read again once the scrolling has settled)
click_ctl() {
  local c
  c=$(reveal "$1") || { fail "control $1 not found or not reachable in the pane" "$(rect_of "$1")"; return 1; }
  sleep 0.3
  c=$(reveal "$1") || return 1
  click $c
}

# click_seg ID INDEX: click the INDEX-th segment (0-based) of a segmented control
click_seg() {
  reveal "$1" >/dev/null || { fail "segmented control $1 not found"; return 1; }
  local r; r=$(jq -r --arg id "$1" --argjson i "$2" '[.ctls[]|select(.id==$id)][0].parts[$i]|"\(.[0]+.[2]/2|floor) \(.[1]+.[3]/2|floor)"' "$DECSETTINGS_DEBUG_LAYOUT")
  click $r
}

# Appearance pane: open_tab theme|customise|wallpaper|apps, open_section colours|fonts|window|dock|icons
open_tab() {
  local i; case $1 in theme) i=0;; customise) i=1;; wallpaper) i=2;; apps) i=3;; *) fail "unknown tab $1"; return 1;; esac
  click_seg ap:tab $i; sleep 0.4
}
open_section() {
  local i; case $1 in colours) i=0;; fonts) i=1;; window) i=2;; dock) i=3;; icons) i=4;; *) fail "unknown section $1"; return 1;; esac
  open_tab customise; click_seg ap:sec $i; sleep 0.4
}
