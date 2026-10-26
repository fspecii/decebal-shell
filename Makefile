PREFIX ?= /usr/local
DESTDIR ?=
BINS = macdock macbar machaven maclaunch
UNITS = macshell-dock.service macshell-bar.service
AUTOSTART = macshell-dock.desktop macshell-bar.desktop

all:
	cargo build --release --locked

install: all
	for b in $(BINS); do install -Dm755 target/release/$$b $(DESTDIR)$(PREFIX)/bin/$$b; done
	for u in $(UNITS); do install -Dm644 contrib/$$u $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do install -Dm644 contrib/$$d $(DESTDIR)/etc/xdg/autostart/$$d; done
	install -Dm644 contrib/machaven.desktop $(DESTDIR)$(PREFIX)/share/applications/machaven.desktop
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/macshell/LICENSE

uninstall:
	for b in $(BINS); do rm -f $(DESTDIR)$(PREFIX)/bin/$$b; done
	for u in $(UNITS); do rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/$$u; done
	for d in $(AUTOSTART); do rm -f $(DESTDIR)/etc/xdg/autostart/$$d; done
	rm -f $(DESTDIR)$(PREFIX)/share/applications/machaven.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/macshell

test:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

.PHONY: all install uninstall test
