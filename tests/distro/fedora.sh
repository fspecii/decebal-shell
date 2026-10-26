set -u
echo "== $(. /etc/os-release; echo $PRETTY_NAME), glibc $(ldd --version | head -1 | awk '{print $NF}')"
dnf -y -q install cargo gcc libxkbcommon-devel fontconfig-devel sway grim foot xorg-x11-server-Xvfb xdotool ImageMagick dejavu-sans-fonts procps-ng git make libX11-devel libXft-devel libXinerama-devel picom xterm >/tmp/dnf.log 2>&1 || { tail -5 /tmp/dnf.log; echo "dnf failed"; exit 1; }
cp -r /src /build && cd /build && rm -rf target
echo "== build"; cargo build --release --locked 2>&1 | tail -2
echo "== tests"; cargo test --release --locked 2>&1 | grep -E 'test result' | head -1
echo "== sizes"; ls -la target/release/macdock target/release/macbar target/release/machaven | awk '{printf "%s %.1f MB\n",$9,$5/1048576}'
export XDG_RUNTIME_DIR=/tmp/xdg; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR
printf 'output * bg #2b3440 solid_color\ndefault_border none\n' > /tmp/sway.conf
LIBSEAT_BACKEND=noop WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
sleep 4; export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep -E '^wayland-[0-9]+$' | head -1); echo "sway socket: $WAYLAND_DISPLAY"; [ -z "$WAYLAND_DISPLAY" ] && { echo "--- sway.log"; tail -8 /tmp/sway.log; }
export MACSHELL_CONFIG=/cfg
./target/release/macbar >/tmp/bar.log 2>&1 & B=$!
./target/release/macdock >/tmp/dock.log 2>&1 & D=$!
foot -e sleep 20 >/dev/null 2>&1 &
MACHAVEN_DOWNLOAD_DIR=/tmp/dl ./target/release/machaven --recent >/tmp/mh.log 2>&1 & M=$!
sleep 8
for p in $B $D $M; do echo "$(cat /proc/$p/comm) alive, RSS $(awk '/VmRSS/{printf "%.1f",$2/1024}' /proc/$p/status) MB"; done
grim /out/fedora-wayland.png && echo "screenshot ok"
head -2 /tmp/bar.log /tmp/dock.log /tmp/mh.log

echo "== X11: build dwm from source, run bar+dock+browser under it"
pkill -f './target/release' || true; pkill sway || true; sleep 1
git clone -q https://git.suckless.org/dwm /tmp/dwm && make -C /tmp/dwm -s >/dev/null 2>&1 && echo "dwm built"
unset WAYLAND_DISPLAY
Xvfb :7 -screen 0 1600x900x24 >/dev/null 2>&1 & sleep 1.5
export DISPLAY=:7 MACSHELL_BACKEND=x11
/tmp/dwm/dwm >/dev/null 2>&1 & picom --backend xrender --config /dev/null >/dev/null 2>&1 & sleep 1
./target/release/macbar >/tmp/barx.log 2>&1 & B=$!
./target/release/macdock >/tmp/dockx.log 2>&1 & D=$!
xterm -e sleep 15 >/dev/null 2>&1 &
sleep 2; MACHAVEN_DOWNLOAD_DIR=/tmp/dl ./target/release/machaven --recent >/tmp/mhx.log 2>&1 & M=$!
sleep 8
for p in $B $D $M; do echo "x11 $(cat /proc/$p/comm) alive, RSS $(awk '/VmRSS/{printf "%.1f",$2/1024}' /proc/$p/status) MB"; done
import -window root /out/fedora-x11.png && echo "x11 screenshot ok"
