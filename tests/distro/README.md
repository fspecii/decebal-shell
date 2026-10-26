# Clean-distro check

`fedora.sh` builds macshell from source in a stock Fedora container, runs the unit tests, then runs
`macbar`, `macdock` and `machaven` under headless sway (Wayland) and under dwm built from source (X11),
and saves a screenshot of each.

```sh
docker run --rm --cap-add SYS_NICE \
  -v "$PWD":/src:ro -v "$PWD"/out:/out -v "$PWD"/tests/distro/fedora.sh:/test.sh:ro \
  fedora:latest bash /test.sh
```

Optional: mount an icon theme (`/root/.local/share/icons`), Inter (`/root/.local/share/fonts`) and a
config dir (`/cfg`, read via `MACSHELL_CONFIG`) to reproduce the original look.
