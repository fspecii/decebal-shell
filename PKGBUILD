# Maintainer: ambsd <fspecii@gmail.com>
pkgname=decebal
pkgver=0.2.0
pkgrel=1
pkgdesc='Decebal: customisable, rice-friendly desktop environment for rdwm (dock, bar, launcher, settings, themes, widgets) for Wayland and X11'
arch=('x86_64' 'aarch64')
url='https://github.com/fspecii/decebal'
license=('MIT')
depends=('fontconfig' 'libxcb' 'pam')
makedepends=('cargo')
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
  # not --all-features: the test-only `test-auth` feature compiles a password bypass into declock
  cargo build --frozen --release --features wayland,x11
}

check() {
  cd "$pkgname-$pkgver"
  cargo test --frozen --all-features
}

package() {
  cd "$pkgname-$pkgver"
  for bin in decwall decdock decbar dechaven declaunch decspot decswitch decexpose dechud declock decdash decmon decsession decutil decsettings decthemectl decnotify decwm decclip dechot decwidgets decnotes dectimer; do
    install -Dm755 "target/release/$bin" "$pkgdir/usr/bin/$bin"
  done
  # DEPRECATED compatibility names (macbar -> decbar, ...), kept for one release so existing keybinds keep working
  for b in decdock decbar dechaven declaunch decspot decswitch decexpose dechud decdash decsession decutil decsettings decthemectl decnotify decwm decclip dechot decwidgets; do
    install -Dm755 contrib/compat/mac-compat "$pkgdir/usr/bin/mac${b#dec}"
  done
  install -Dm644 contrib/decebal-dock.service "$pkgdir/usr/lib/systemd/user/decebal-dock.service"
  install -Dm644 contrib/decebal-bar.service "$pkgdir/usr/lib/systemd/user/decebal-bar.service"
  install -Dm644 contrib/decebal-notify.service "$pkgdir/usr/lib/systemd/user/decebal-notify.service"
  install -Dm644 contrib/decebal-dock.desktop "$pkgdir/etc/xdg/autostart/decebal-dock.desktop"
  install -Dm644 contrib/decebal-clip.service "$pkgdir/usr/lib/systemd/user/decebal-clip.service"
  install -Dm644 contrib/decebal-clip.desktop "$pkgdir/etc/xdg/autostart/decebal-clip.desktop"
  install -Dm644 contrib/decebal-timer.service "$pkgdir/usr/lib/systemd/user/decebal-timer.service"
  install -Dm644 contrib/decebal-timer.desktop "$pkgdir/etc/xdg/autostart/decebal-timer.desktop"
  install -Dm644 contrib/decebal-live-wallpaper.service "$pkgdir/usr/lib/systemd/user/decebal-live-wallpaper.service"
  install -Dm644 contrib/decebal-live-wallpaper.desktop "$pkgdir/etc/xdg/autostart/decebal-live-wallpaper.desktop"
  install -Dm644 contrib/decebal-widgets.service "$pkgdir/usr/lib/systemd/user/decebal-widgets.service"
  install -Dm644 contrib/decebal-widgets.desktop "$pkgdir/etc/xdg/autostart/decebal-widgets.desktop"
  install -Dm644 contrib/decebal-hot.service "$pkgdir/usr/lib/systemd/user/decebal-hot.service"
  install -Dm644 contrib/decebal-hot.desktop "$pkgdir/etc/xdg/autostart/decebal-hot.desktop"
  install -Dm644 contrib/decebal-wallpaper-colors.service "$pkgdir/usr/lib/systemd/user/decebal-wallpaper-colors.service"
  install -Dm644 contrib/decebal-wallpaper-colors.desktop "$pkgdir/etc/xdg/autostart/decebal-wallpaper-colors.desktop"
  install -Dm644 contrib/decebal-bar.desktop "$pkgdir/etc/xdg/autostart/decebal-bar.desktop"
  install -Dm644 contrib/decebal-notify.desktop "$pkgdir/etc/xdg/autostart/decebal-notify.desktop"
  install -Dm644 contrib/dbus/org.freedesktop.Notifications.service "$pkgdir/usr/share/decebal/dbus/org.freedesktop.Notifications.service"
  for f in $(cd themes && find . -type f); do
    install -Dm644 "themes/$f" "$pkgdir/usr/share/decebal/themes/${f#./}"
  done
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
  install -Dm644 contrib/dechaven.desktop "$pkgdir/usr/share/applications/dechaven.desktop"
  install -Dm644 contrib/decsettings.desktop "$pkgdir/usr/share/applications/decsettings.desktop"
  install -Dm644 contrib/decnotes.desktop "$pkgdir/usr/share/applications/decnotes.desktop"
  install -Dm644 contrib/dectimer.desktop "$pkgdir/usr/share/applications/dectimer.desktop"
  install -Dm644 contrib/decmon.desktop "$pkgdir/usr/share/applications/decmon.desktop"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
