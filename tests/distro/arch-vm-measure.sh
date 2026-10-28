#!/bin/bash
# Runs inside the Arch VM: starts Xorg + dwm + picom + mako + decbar + decdock + xterm and reports memory.
pss() { awk '/^Pss:/{s+=$2} END{printf "%.1f", s/1024}' /proc/$1/smaps_rollup 2>/dev/null; }
row() { p=$(pgrep -nx "$1") && printf "  %-10s %6s MB PSS\n" "$1" "$(pss $p)"; }
echo "== bare (no X)"; free -m | sed -n 2p
sudo sh -c 'Xorg :0 vt7 -ac -nolisten tcp -noreset >/tmp/xorg.log 2>&1 &'
sleep 4
export DISPLAY=:0 DECEBAL_BACKEND=x11
xsetroot -solid '#2b3440'
dwm >/tmp/dwm.log 2>&1 &
sleep 1; echo "== Xorg + dwm"; free -m | sed -n 2p
picom --backend xrender --config /dev/null >/tmp/picom.log 2>&1 &
mako >/tmp/mako.log 2>&1 &
~/decebal/target/release/decbar >/tmp/decbar.log 2>&1 &
~/decebal/target/release/decdock >/tmp/decdock.log 2>&1 &
xterm -e sleep 600 >/dev/null 2>&1 &
sleep 12
echo "== full desktop (Xorg, dwm, picom, mako, decbar, decdock, xterm)"; free -m | sed -n 2p
for n in Xorg dwm picom mako decbar decdock xterm; do row $n; done
import -window root /tmp/arch-x11.png && echo screenshot-ok
