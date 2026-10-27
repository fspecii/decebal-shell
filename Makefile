PREFIX ?= /usr/local
DESTDIR ?=
BINS = macdock macbar machaven maclaunch macspot macsettings
UNITS = macshell-dock.service macshell-bar.service
AUTOSTART = macshell-dock.desktop macshell-bar.desktop

all:
	cargo build --release --locked

install: all
	for b in $(BINS); do install -Dm755 target/release/$$b $(DESTDIR)$(PREFIX)/bin/$$b; done
	for u in $(UNITS); do install -Dm644 contrib/$$u $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; sed -i 's|/usr/bin/|$(PREFIX)/bin/|' $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do install -Dm644 contrib/$$d $(DESTDIR)/etc/xdg/autostart/$$d; done
	install -Dm644 contrib/machaven.desktop $(DESTDIR)$(PREFIX)/share/applications/machaven.desktop
	install -Dm644 contrib/macsettings.desktop $(DESTDIR)$(PREFIX)/share/applications/macsettings.desktop
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/macshell/LICENSE

uninstall:
	for b in $(BINS); do rm -f $(DESTDIR)$(PREFIX)/bin/$$b; done
	for u in $(UNITS); do rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do rm -f $(DESTDIR)/etc/xdg/autostart/$$d; done
	rm -f $(DESTDIR)$(PREFIX)/share/applications/machaven.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/macsettings.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/macshell

# Extra login session "Hyprland (macshell)": the Rust shell without Quickshell (Omarchy + SDDM + hyprlock/hypridle/mako).
install-session:
	install -Dm755 contrib/session/macshell-session $(DESTDIR)$(PREFIX)/bin/macshell-session
	install -Dm755 contrib/session/omarchy-launch-shell $(DESTDIR)$(PREFIX)/lib/macshell/shim/omarchy-launch-shell
	install -Dm755 contrib/session/omarchy-shell $(DESTDIR)$(PREFIX)/lib/macshell/shim/omarchy-shell
	for f in hypridle.conf hyprlock.conf mako.conf; do install -Dm644 contrib/session/$$f $(DESTDIR)$(PREFIX)/share/macshell/$$f; done
	for f in hyprland-layers.lua hyprland-machaven.lua hyprland-maclaunch.lua; do install -Dm644 contrib/$$f $(DESTDIR)$(PREFIX)/share/macshell/$$f; done
	install -Dm644 contrib/session/hyprland-macshell.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/hyprland-macshell.desktop

# End-to-end test of macsettings against rdwm on a private Xvfb (needs Xvfb, xdotool, jq and a built rdwm; see the script).
e2e-macsettings: all
	tests/e2e/macsettings.sh

test:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

.PHONY: all install install-session uninstall test e2e-macsettings
