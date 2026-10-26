PREFIX ?= /usr/local
DESTDIR ?=
BINS = macdock

all:
	cargo build --release --locked

install: all
	for b in $(BINS); do install -Dm755 target/release/$$b $(DESTDIR)$(PREFIX)/bin/$$b; done
	install -Dm644 contrib/macshell-dock.service $(DESTDIR)$(PREFIX)/lib/systemd/user/macshell-dock.service
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/macshell/LICENSE

uninstall:
	for b in $(BINS); do rm -f $(DESTDIR)$(PREFIX)/bin/$$b; done
	rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/macshell-dock.service

test:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

.PHONY: all install uninstall test
