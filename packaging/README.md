# Packaging

| Target | How | Status |
|---|---|---|
| Debian / Ubuntu | `deb/build.sh [outdir]` after `cargo build --release --locked` | verified on Ubuntu 24.04 |
| Arch / Omarchy | `../PKGBUILD` | maintained |

Installed-package smoke test (Ubuntu 24.04 container): the rdwm and decebal `.deb` files build and install with
`dpkg -i`; rdwm, decbar, decdock and decnotify start and stay up on a private X server, and `rdwmctl` and `decthemectl`
answer. The full e2e suite runs against a source build, not against an installed package.
