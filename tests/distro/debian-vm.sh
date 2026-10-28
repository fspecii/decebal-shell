set -u
echo "== $(. /etc/os-release; echo $PRETTY_NAME) kernel $(uname -r) glibc $(ldd --version | head -1 | awk '{print $NF}')"
sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq cargo gcc libxkbcommon-dev libfontconfig-dev pkg-config sway grim foot xvfb dwm picom xterm imagemagick fonts-dejavu-core procps curl >/tmp/apt.log 2>&1 || { tail -5 /tmp/apt.log; echo "apt failed"; exit 1; }
echo "rustc $(rustc --version)"
cd ~/decebal && echo "== build" && cargo build --release 2>&1 | tail -2
echo "== tests" && cargo test 2>&1 | grep -E 'test result: ' | head -1
ls -la target/release/decdock target/release/decbar target/release/dechaven | awk '{printf "%s %.1f MB\n",$9,$5/1048576}'
export XDG_RUNTIME_DIR=/tmp/xdg; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR
printf 'output * bg #2b3440 solid_color\ndefault_border none\nfor_window [app_id="dechaven"] floating enable, resize set 1000 640, move position center\n' > /tmp/sway.conf
LIBSEAT_BACKEND=noop WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
sleep 5; export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep -E '^wayland-[0-9]+$' | head -1); echo "sway socket: ${WAYLAND_DISPLAY:-none}"
[ -z "${WAYLAND_DISPLAY:-}" ] && tail -5 /tmp/sway.log
./target/release/decbar >/tmp/bar.log 2>&1 & B=$!
./target/release/decdock >/tmp/dock.log 2>&1 & D=$!
foot -e sleep 25 >/dev/null 2>&1 &
sleep 2; DECHAVEN_DOWNLOAD_DIR=/tmp/dl ./target/release/dechaven --recent >/tmp/mh.log 2>&1 & M=$!
sleep 9
for p in $B $D $M; do echo "wayland $(cat /proc/$p/comm) alive, RSS $(awk '/VmRSS/{printf "%.1f",$2/1024}' /proc/$p/status) MB"; done
grim /tmp/vm-wayland.png && echo "wayland screenshot ok"
cat /tmp/bar.log /tmp/dock.log /tmp/mh.log | head -3
kill $B $D $M 2>/dev/null; pkill sway; sleep 1
echo "== X11 with Debian's dwm package"
unset WAYLAND_DISPLAY; Xvfb :7 -screen 0 1600x900x24 >/dev/null 2>&1 & sleep 1.5
export DISPLAY=:7 DECEBAL_BACKEND=x11
dwm >/dev/null 2>&1 & picom --backend xrender --config /dev/null >/dev/null 2>&1 & sleep 1
./target/release/decbar >/tmp/barx.log 2>&1 & B=$!
./target/release/decdock >/tmp/dockx.log 2>&1 & D=$!
xterm -e sleep 15 >/dev/null 2>&1 & sleep 2
DECHAVEN_DOWNLOAD_DIR=/tmp/dl ./target/release/dechaven --recent >/tmp/mhx.log 2>&1 & M=$!
sleep 8
for p in $B $D $M; do echo "x11 $(cat /proc/$p/comm) alive, RSS $(awk '/VmRSS/{printf "%.1f",$2/1024}' /proc/$p/status) MB"; done
import -window root /tmp/vm-x11.png && echo "x11 screenshot ok"
