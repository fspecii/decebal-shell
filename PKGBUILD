# Maintainer: ambsd <fspecii@gmail.com>
pkgname=decebal
pkgver=0.3.0
pkgrel=1
pkgdesc='Decebal: customisable, rice-friendly desktop environment for rdwm (dock, bar, launcher, settings, themes, widgets) for Wayland and X11'
arch=('x86_64' 'aarch64')
url='https://github.com/fspecii/decebal'
license=('MIT')
depends=('fontconfig' 'libxcb' 'pam')
makedepends=('cargo' 'python')
provides=('macshell')
conflicts=('macshell')
replaces=('macshell')
optdepends=('mpv: live wallpapers on X11'
            'mpvpaper: live wallpapers on Wayland'
            'picom: translucency and blur on X11'
            'networkmanager: Wi-Fi tile (nmcli)'
            'bluez-utils: Bluetooth tile (bluetoothctl)'
            'wireplumber: volume (wpctl)'
            'brightnessctl: brightness slider'
            'playerctl: Now Playing'
            'fastfetch: Fastfetch system-information widget'
            'ffmpeg: live wallpaper still frames and screen recording on X11'
            'slop: region selection for decutil record --region'
            'wf-recorder: screen recording on Wayland'
            'power-profiles-daemon: performance profile for decutil gamemode'
            'xorg-xset: decutil idle-inhibit'
            'nvidia-utils: GPU card in the decdash dashboard (nvidia-smi)'
            'curl: weather and remote album art'
            'rdwm: the tiling window manager these panels are made for (see the macdesktop meta package)'
            'adwaita-icon-theme: fallback icons')
source=("$pkgname-$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
  cd "$pkgname-$pkgver"
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=target
  # not --all-features: the test-only `test-auth` feature compiles a password bypass into declock.
  # stage.py records the exact executables cargo reports for this build (and their sha256) for package().
  python3 packaging/stage.py build --manifest "$srcdir/decebal-build-manifest.json" -- --frozen --features wayland,x11
}

check() {
  cd "$pkgname-$pkgver"
  cargo test --frozen --all-features
}

package() {
  cd "$pkgname-$pkgver"
  # Same file list as `make install` (binaries re-checked against the build manifest, compatibility wrappers, user units,
  # autostart and desktop entries, D-Bus service file, every theme package with its fonts and licences, docs).
  # No inventory file: pacman owns the file list.
  python3 packaging/stage.py install --prefix /usr --destdir "$pkgdir" \
    --manifest "$srcdir/decebal-build-manifest.json" --no-inventory
}
