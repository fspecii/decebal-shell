# Window-style proof against real window managers (2026-10-31)

What was run, exactly, and what it showed. Nothing live was touched: every run used a fresh private HOME / `XDG_*` / `DECEBAL_CONFIG`, a private
`XDG_RUNTIME_DIR` socket (rdwm ignores `RDWM_SOCK` and would otherwise use the shared `/tmp/rdwm-<uid>/`), and a private Xvfb on a reserved high display
(730 to 733), with the ownership checks of `tools/private-xvfb.sh`. The user's running rdwm socket was never opened. Afterwards no process, socket or lock of
the fixtures remained.

| what | exact |
|---|---|
| decthemectl under test | built from repository `ca418f246ecd5acd7252e5e572ec8efd294ef71f` (source digest `a535fd1600b86658ca2b104a20a8394533aae57b5243298fd1c196354fa5a51b`, `src` + `themes` + `Cargo.*`), sha256 `416c574504ac2b9086e10f12be3c2149c5fac8f84c453b7754281236811a19e8` |
| NEW rdwm (borrowed, immutable, read-only) | `/var/tmp/rdwm-layout-decorations-release-artifacts/release/rdwm` sha256 `d7a49344a9f7db98ccf3207c43b144a6a794d568d7a41f79a1503346c38b762d`, `rdwmctl` `061313cb81972def843e3d35dc1c53b0799f0a0c452c4d32ddd52b15c051876f`; built from rdwm `2559173fbac91442ffb1d203d09f74935d3d498a` (`source.json` of that directory) |
| OLD rdwm (installed) | `/usr/bin/rdwm` sha256 `5d62d8126e3a2898b518d08d06af43ba8e835a95182539bc09bf838ae07351c6` (2026-10-28), `rdwmctl` `18909a4ddb93ad9012c65dbf4ba82e9b90e3be35e6fe65e90aa866b21eb34bce`; its `schema` lists **15** styles |
| harness | `tools/decoration-proof.sh` (real `decthemectl theme apply lingmo`, then the config file and the running WM's own `rdwmctl config` / `schema`), `tools/preview-style-proof.sh` |

## Apply, per running window manager and mode

| running rdwm | mode | styles the running WM lists | config written | the running WM reports in force | `decoration_compat` record |
|---|---|---|---|---|---|
| new | dark | 24 | `style = "lingmo-dark"` | `lingmo-dark` | none |
| new | light | 24 | `style = "lingmo"` | `lingmo` | none |
| old | dark | 15 | `style = "adwaita"` | `adwaita` | `{desired: lingmo-dark, applied: adwaita, proof: running}` + message |
| old | light | 15 | `style = "adwaita-light"` | `adwaita-light` | `{desired: lingmo, applied: adwaita-light, proof: running}` + message |

The old WM's apply printed: "Window style `lingmo-dark` needs a newer rdwm: the running rdwm does not list it, so `adwaita` is used for now. Upgrade rdwm and log in again
(or restart rdwm), then apply the theme again." The apply reloaded the config as usual and did not restart or replace the window manager. Both applies exited 0.
The old rdwm accepted the fallback style and the running WM itself confirmed it; the wanted style was never written.

## Native preview, per mode

`tools/preview-style-proof.sh` runs `decthemectl theme render lingmo --mode M` with a recording stand-in for the decoration helper (`DECEBAL_DECO_RENDERER`) and no window manager:
the helper was given `style = "lingmo-dark"` in dark mode and `style = "lingmo"` in light mode (the preview worker sets `DECEBAL_DECO_CAPS=preview`, the only gate bypass).
The stand-in proves what reaches the native renderer, not what the real renderer draws: the real `decebal-deco-preview` helper was not available here (it needs an rdwm checkout build).

## Not proven here

* a pixel capture of the native `lingmo` frame on a real window (no window manager was driven to map a client);
* an unknown or transient answer from a real rdwm (covered with fake servers in `tests/lingmo_desktop_port.rs`, not with a real process);
* a login or restart of the user's own window manager (deliberately not done).
