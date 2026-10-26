# Maintainer: ambsd <fspecii@gmail.com>
pkgname=macshell
pkgver=0.1.0
pkgrel=1
pkgdesc='Lightweight macOS-style dock, menu bar and wallpaper browser for Wayland and X11'
arch=('x86_64' 'aarch64')
url='https://github.com/fspecii/macshell'
license=('MIT')
depends=('fontconfig' 'libxcb')
makedepends=('cargo')
optdepends=('picom: translucency and blur on X11'
            'networkmanager: Wi-Fi tile (nmcli)'
            'bluez-utils: Bluetooth tile (bluetoothctl)'
            'wireplumber: volume (wpctl)'
            'brightnessctl: brightness slider'
            'playerctl: Now Playing')
source=("$pkgname-$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
  cd "$pkgname-$pkgver"
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=target
  cargo build --frozen --release --all-features
}

check() {
  cd "$pkgname-$pkgver"
  cargo test --frozen --all-features
}

package() {
  cd "$pkgname-$pkgver"
  for bin in macdock; do
    install -Dm755 "target/release/$bin" "$pkgdir/usr/bin/$bin"
  done
  install -Dm644 contrib/macshell-dock.service "$pkgdir/usr/lib/systemd/user/macshell-dock.service"
  install -Dm644 contrib/macshell-dock.desktop "$pkgdir/etc/xdg/autostart/macshell-dock.desktop"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
