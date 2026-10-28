PREFIX ?= /usr/local
DESTDIR ?=
BINS = macdock macbar machaven maclaunch macspot macswitch macexpose machud macdash macsession macutil macsettings macthemectl macnotify macwm macclip machot
UNITS = macshell-dock.service macshell-bar.service macshell-notify.service macshell-clip.service macshell-hot.service
AUTOSTART = macshell-dock.desktop macshell-bar.desktop macshell-notify.desktop macshell-clip.desktop macshell-hot.desktop

all:
	cargo build --release --locked

install: all
	for b in $(BINS); do install -Dm755 target/release/$$b $(DESTDIR)$(PREFIX)/bin/$$b; done
	for u in $(UNITS); do install -Dm644 contrib/$$u $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; sed -i 's|/usr/bin/|$(PREFIX)/bin/|' $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do install -Dm644 contrib/$$d $(DESTDIR)/etc/xdg/autostart/$$d; done
	install -Dm644 contrib/machaven.desktop $(DESTDIR)$(PREFIX)/share/applications/machaven.desktop
	install -Dm644 contrib/macsettings.desktop $(DESTDIR)$(PREFIX)/share/applications/macsettings.desktop
	install -Dm644 contrib/dbus/org.freedesktop.Notifications.service $(DESTDIR)$(PREFIX)/share/macshell/dbus/org.freedesktop.Notifications.service
	install -Dm644 README.md $(DESTDIR)$(PREFIX)/share/doc/macshell/README.md
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/macshell/LICENSE

uninstall:
	for b in $(BINS); do rm -f $(DESTDIR)$(PREFIX)/bin/$$b; done
	for u in $(UNITS); do rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do rm -f $(DESTDIR)/etc/xdg/autostart/$$d; done
	rm -f $(DESTDIR)$(PREFIX)/share/applications/machaven.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/macsettings.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/macshell/dbus $(DESTDIR)$(PREFIX)/share/doc/macshell
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/macshell

# Extra login session "Hyprland (macshell)": the Rust shell without Quickshell (Omarchy + SDDM + hyprlock/hypridle/mako).
install-session:
	install -Dm755 contrib/session/macshell-session $(DESTDIR)$(PREFIX)/bin/macshell-session
	install -Dm755 contrib/session/omarchy-launch-shell $(DESTDIR)$(PREFIX)/lib/macshell/shim/omarchy-launch-shell
	install -Dm755 contrib/session/omarchy-shell $(DESTDIR)$(PREFIX)/lib/macshell/shim/omarchy-shell
	for f in hypridle.conf hyprlock.conf mako.conf; do install -Dm644 contrib/session/$$f $(DESTDIR)$(PREFIX)/share/macshell/$$f; done
	for f in hyprland-layers.lua hyprland-machaven.lua hyprland-maclaunch.lua; do install -Dm644 contrib/$$f $(DESTDIR)$(PREFIX)/share/macshell/$$f; done
	install -Dm644 contrib/session/hyprland-macshell.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/hyprland-macshell.desktop

# End-to-end tests against rdwm on a private Xvfb (need Xvfb, xdotool, jq and a built rdwm; see the scripts).
e2e-theme: all
	tests/e2e/theme.sh

e2e-macsettings: all
	tests/e2e/macsettings.sh

# macdash on a private Xvfb + private session bus with fake MPRIS players (needs Xvfb, xdotool, import, dbus-daemon, tesseract).
e2e-dash:
	tests/e2e/dash.sh

# macnotify on a private session bus + Xvfb (needs Xvfb, dbus-daemon, busctl, xdotool, import).
e2e-notify:
	cargo build --release --locked --bin macnotify
	MACNOTIFY_BIN=$(CURDIR)/target/release/macnotify tests/e2e/notify.sh

# macclip (clipboard history) on a private Xvfb (needs Xvfb, xclip, xdotool, xprop, import, jq).
e2e-clip:
	tests/e2e/clip.sh

# machot (hot corners) on a private Xvfb with shimmed commands (needs Xvfb, xdotool, xrandr, xprop).
e2e-hot:
	tests/e2e/hot.sh

# The Caelestia bar (left edge) on a private Xvfb + rdwm (needs Xvfb, xdotool, xprop, import, jq, convert).
e2e-vbar:
	tests/e2e/vbar.sh

# Every Caelestia layout (left, right, top, bottom, split) on a private Xvfb :96 + rdwm (needs Xvfb, xdotool, xprop, import, jq).
e2e-layouts:
	tests/e2e/layouts.sh

# macsession + macutil on a private Xvfb with shimmed systemctl/loginctl/ffmpeg/xset/picom/powerprofilesctl (Xvfb, xdotool, import).
e2e-session: all
	tests/e2e/session.sh

test:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

.PHONY: all install install-session uninstall test e2e-macsettings e2e-theme e2e-notify e2e-clip e2e-hot e2e-session e2e-dash e2e-vbar e2e-layouts
