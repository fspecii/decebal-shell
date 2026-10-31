# Native replica evidence (start menu, shortcut panel, real Start click)

Tested source: branch `feat/claude-openkylin-replica` at `48ef39dbdf97230f7fa8e7b02d113694bc083668` (the later commit only adds this evidence) **plus root's `fbddaf8` applied in the private scratch tree only** (the branch itself is based on `f6e7ccb`; qualify on the merged tree).
Private build: `cargo build/test --offline -j2`, nice 10, slot 1, cleared environment, private target dir; the lib test binary is the same build.

| executable | sha256 |
|---|---|
| `decthemectl` | `9e165870af252733692eb2abe83909fab1cf6c96a1d1d9881419f98e3cd792d8` |
| `decdock` | `fc8b50d2446ff386960f63cd04ba58a9b4cf76f802999f6f4184d9decf83411e` |
| `decbar` | `78f681ba567cdb6b0ab0555ea160802eadff199ee14dd834f401c43153db90aa` |
| `declaunch` | `eaf46d1357c14dac37a3b2a5d230a170f9adaaaa00bca4e5a719ef20ac9cf8c6` |
| `decwidgets` | `6f4c9d148d2fde6ae6964388cace57cbb9d542c0ba1f43631578055aca149798` |
| `decebal-libtest` | `b4a5918c4175b80e9afca3142c9d9548fbbeeda3f2f60375da261c73117286cb` |

Fixture: private owned Xvfb (display number >= 700, ownership verified in the filesystem and abstract socket namespaces before input), picom xrender backdrop, private HOME/XDG, dead session bus, stubs for `peony gio xdg-open decsession decsettings` that only record. 1920x1080, scale 1 / 1.25 / 2, light and dark, packages `openkylin`, `-light-seeking`, `-heyin`, `-classic`. Icons: the private prefix of the pinned `openkylin-theme` packs.

| file | what it proves |
|---|---|
| `native-launcher.json` | start menu card size at 3 scales x 2 modes x 4 packages (bounding box = card + shadow) |
| `native-launcher-edges.json` | the card never covers the panel strip on top / bottom / left / right (baseline diff), 100 / 200 %, 2 packages |
| `native-launcher-input.json` | xdotool: typing searches; clicks on user, computer, settings, power each ran the recorded program (`decsettings users`, `peony computer:///`, `decsettings`, `decsession`) and closed the menu; Tab/Down/Return walk reaches Settings; Escape closes (100 % light, 200 % dark) |
| `native-launcher-expand.json` | expand covers the whole free area, restore brings the start menu back, the search survives |
| `native-start-click.json` | a real click on the taskbar Start cell opens the card at the Start end, clear of the strip, 4 edges x 100 / 200 % x light/dark x 2 packages (needs `fbddaf8`) |
| `native-controlcenter.json`, `cc-actions.log`, `stub-service-calls.log`, `test-output.txt` | shortcut panel captures; every control reaches its backend through the dry-run hook (no setter reached a stub) |
| `replica-compare-*.png`, `replica-compare.json` | official `ukui-globaltheme` preview (identified by commit and sha256) beside the native captures of the same theme and mode. The preview is a theme mock-up, not a capture of a running UKUI desktop |
| `failures/` | every failed attempt and its cause |

Original-reference limits are listed in `../../replica-gaps.md`.
