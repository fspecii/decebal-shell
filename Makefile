PREFIX ?= /usr/local
DESTDIR ?=
BINS = decdock decbar dechaven declaunch decspot decswitch decexpose dechud decdash decmon decsession decutil decsettings decthemectl decnotify decwm decclip dechot decwidgets
UNITS = decebal-dock.service decebal-bar.service decebal-notify.service decebal-clip.service decebal-hot.service decebal-wallpaper-colors.service
AUTOSTART = decebal-dock.desktop decebal-bar.desktop decebal-notify.desktop decebal-clip.desktop decebal-hot.desktop decebal-wallpaper-colors.desktop

all:
	cargo build --release --locked

install: all
	for b in $(BINS); do install -Dm755 target/release/$$b $(DESTDIR)$(PREFIX)/bin/$$b; done
	# DEPRECATED compatibility names: macbar -> decbar, ... (one release, so existing keybinds and autostart keep working)
	for b in $(BINS); do install -Dm755 contrib/compat/mac-compat $(DESTDIR)$(PREFIX)/bin/mac$${b#dec}; done
	for u in $(UNITS); do install -Dm644 contrib/$$u $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; sed -i 's|/usr/bin/|$(PREFIX)/bin/|' $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do install -Dm644 contrib/$$d $(DESTDIR)/etc/xdg/autostart/$$d; done
	install -Dm644 contrib/dechaven.desktop $(DESTDIR)$(PREFIX)/share/applications/dechaven.desktop
	install -Dm644 contrib/decsettings.desktop $(DESTDIR)$(PREFIX)/share/applications/decsettings.desktop
	install -Dm644 contrib/decmon.desktop $(DESTDIR)$(PREFIX)/share/applications/decmon.desktop
	install -Dm644 contrib/dbus/org.freedesktop.Notifications.service $(DESTDIR)$(PREFIX)/share/decebal/dbus/org.freedesktop.Notifications.service
	install -Dm644 README.md $(DESTDIR)$(PREFIX)/share/doc/decebal/README.md
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/decebal/LICENSE

uninstall:
	for b in $(BINS); do rm -f $(DESTDIR)$(PREFIX)/bin/$$b $(DESTDIR)$(PREFIX)/bin/mac$${b#dec}; done
	rm -f $(DESTDIR)$(PREFIX)/bin/macshell-session
	for u in $(UNITS); do rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do rm -f $(DESTDIR)/etc/xdg/autostart/$$d; done
	rm -f $(DESTDIR)$(PREFIX)/share/applications/dechaven.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/decsettings.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/decmon.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/decebal/dbus $(DESTDIR)$(PREFIX)/share/doc/decebal
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/decebal

# Extra login session "Hyprland (decebal)": the Rust shell without Quickshell (Omarchy + SDDM + hyprlock/hypridle/mako).
install-session:
	install -Dm755 contrib/session/decebal-session $(DESTDIR)$(PREFIX)/bin/decebal-session
	install -Dm755 contrib/compat/mac-compat $(DESTDIR)$(PREFIX)/bin/macshell-session
	install -Dm755 contrib/session/omarchy-launch-shell $(DESTDIR)$(PREFIX)/lib/decebal/shim/omarchy-launch-shell
	install -Dm755 contrib/session/omarchy-shell $(DESTDIR)$(PREFIX)/lib/decebal/shim/omarchy-shell
	for f in hypridle.conf hyprlock.conf mako.conf; do install -Dm644 contrib/session/$$f $(DESTDIR)$(PREFIX)/share/decebal/$$f; done
	for f in hyprland-layers.lua hyprland-dechaven.lua hyprland-declaunch.lua; do install -Dm644 contrib/$$f $(DESTDIR)$(PREFIX)/share/decebal/$$f; done
	install -Dm644 contrib/session/hyprland-decebal.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/hyprland-decebal.desktop

# End-to-end tests against rdwm on a private Xvfb (need Xvfb, xdotool, jq and a built rdwm; see the scripts).
e2e-theme: all
	tests/e2e/theme.sh

# Font presets: decthemectl font, bar / dashboard / Theme pane screenshots (Xvfb, xdotool, import, jq, dbus-daemon).
e2e-fonts:
	PROFILE=release tests/e2e/fonts.sh
# "Apply to all apps" in a private HOME, Xvfb and D-Bus session (needs Xvfb, xdotool, import, xrdb, dbus-run-session, PyGObject; nautilus and Omarchy are used when present).
e2e-system: all
	tests/e2e/system-theme.sh

e2e-decsettings: all
	tests/e2e/decsettings.sh

# decdash on a private Xvfb + private session bus with fake MPRIS players (needs Xvfb, xdotool, import, dbus-daemon, tesseract).
e2e-dash:
	tests/e2e/dash.sh

# decnotify on a private session bus + Xvfb (needs Xvfb, dbus-daemon, busctl, xdotool, import).
e2e-notify:
	cargo build --release --locked --bin decnotify
	DECNOTIFY_BIN=$(CURDIR)/target/release/decnotify tests/e2e/notify.sh

# decclip (clipboard history) on a private Xvfb (needs Xvfb, xclip, xdotool, xprop, import, jq).
e2e-clip:
	tests/e2e/clip.sh

# dechot (hot corners) on a private Xvfb with shimmed commands (needs Xvfb, xdotool, xrandr, xprop).
e2e-hot:
	tests/e2e/hot.sh

# The Caelestia bar (left edge) on a private Xvfb + rdwm (needs Xvfb, xdotool, xprop, import, jq, convert).
e2e-procmon:
	tests/e2e/procmon.sh

e2e-awake:
	tests/e2e/awake.sh

e2e-vbar:
	tests/e2e/vbar.sh

# Every Caelestia layout (left, right, top, bottom, split) on a private Xvfb :96 + rdwm (needs Xvfb, xdotool, xprop, import, jq).
e2e-layouts:
	tests/e2e/layouts.sh

# decsession + decutil on a private Xvfb with shimmed systemctl/loginctl/ffmpeg/xset/picom/powerprofilesctl (Xvfb, xdotool, import).
e2e-session: all
	tests/e2e/session.sh

test:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

.PHONY: all install install-session uninstall test e2e-decsettings e2e-theme e2e-system e2e-fonts e2e-notify e2e-clip e2e-hot e2e-session e2e-dash e2e-vbar e2e-procmon e2e-awake e2e-layouts
