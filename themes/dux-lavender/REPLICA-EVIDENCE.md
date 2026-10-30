# Replica section1 source captures

These are actual upstream DBarBody renders, not wallpaper assets or live UI.
No screenshots field points at them. Generated gallery previews remain native and automatic.
Source clone87312075dd414a6f60691464bcd6c6a9bb242a7e; common43e05926fcdacc53675623169bcef686faa95301.
Private Sway headless/pixman + Quickshell, private HOME/XDG/bus,1920x1080/DPR1.
Actual source WidgetModel.normalize and module imports; fixture wrapper supplies hostWindow/modelData only.
Source config stockPurple, comfortable density, canonical default item lists, fontScale1/2.
DMS backend intentionally absent: metrics/music/battery unavailable/hidden; weather unknown.
Private runtime clock/focus-title state are not live desktop telemetry and differ from native fixed samples.
At200% text the source band stays38/faces28; text grows24px.
Wayland capture succeeded; X11 attempt crashed in unsupported layer-shell use and its blank frame was excluded.

| File | SHA256 |
| --- | --- |
|replica-upstream-dark-1920x1080-100.png|`786c981bc2b5ed2c74d5bbab73d5938840a4befcdce8e0724c69692565fcea34`|
|replica-upstream-dark-1920x1080-200.png|`8191cf57d07004168e03dc47f569a06715af9cd64740904fde9f416af402232a`|
|replica-upstream-light-1920x1080-100.png|`b4a1ab686424070957a5aa5077069aa205614e2d9c28448b64db17eeb2d06cf8`|
|replica-upstream-light-1920x1080-200.png|`070dc9e649e97861bd23f7416ad3354ad2b8d7e17ac46006d25ff7eb2ee2c6a2`|

## Section1 native handoff — 8addeef

Matched actual native1920×1080/DPR1 renders are included below. Text scale100/200
is matched to upstream; band38/face28/icon14 remain fixed. No rescaling or wallpaper.
Native private Xvfb clock/focus state differs from upstream private Sway. Native
CPU/RAM uses real host sampling; upstream has no DMS backend and hides these items.
Missing music is hidden; weather is explicitly --°C. Neither side is live desktop QA.

| File | SHA256 |
| --- | --- |
|replica-native-dark-1920x1080-100.png|`ae0fbdeb5ebe71086b60968ab8910f8e391dc798c58141503922ef7918c88ec5`|
|replica-native-dark-1920x1080-200.png|`60f4a9ed26c78ab5d084e8c3d9fe5e1b92982c2764486cbaf8dba5a69fd33f35`|
|replica-native-light-1920x1080-100.png|`481bc8222c9932038ec6f0cdda1abe9a882f45d4117b7d3a212ea28e4669eaf1`|
|replica-native-light-1920x1080-200.png|`21fe5b688cb6dc3ea85aa8c86a7b967a99fa2a9b523c63863e8758017b4c112c`|

Native-owner acceptance:171 menubar regressions plus clock-anchor test;24 actual-font
dark/light renders across100/200%,38/26px bands and720/1280/1920 widths;6 private
Xvfb action runs; strict X11 all-target Clippy and fmt passed. Evidence manifests:
`/tmp/decebal-dux-bar-stock-native/native-checks.json` and
`/tmp/decebal-dux-bar-stock-raster/bar-layouts.json`. Those tests validate native
section1 implementation; they do not validate pending dashboard/control-centre bodies.

Exact geometry/tokens/typography and canonical item selection are implemented.
Remaining: joined segments/shape morphs, source overflow popover, media cover/marquee/
waveform/lyrics, composite control trigger options and alternate battery styling.
Native provider values are not pixel-identical fixtures. Wayland popup coordinates,
hardware mixed-DPI and screen-reader acceptance are not claimed.

## Section2 native host evidence — requires1b45fdf

Actual private native window captures, dark100/light200, real clock and explicitly
unavailable provider states. Historical README profile, not current source defaults.
No matched upstream section2 screenshot comparison yet; no full replica claim.
Original clock variable-axis fidelity is pending section3. Scroll-accessible content
extends below the short viewport by design. No screenshots field references these.

|File|SHA256|
|---|---|
|`replica-native-section2-dash-100-dark-overview.png`|`0b97aff35d41b2560e4e8f194320313ca4f4191b31e5b68aed3705d3520d6442`|
|`replica-native-section2-dash-200-light-overview.png`|`c8fb02977aeddb0555360bb8091abb53b352fd63d32fd6bf7c6435abb432eb69`|
|`replica-native-section2-control-100-dark.png`|`cde9d00334419da2b6210bcae722801051e65250c58845636521a2fcad0186f0`|
|`replica-native-section2-control-200-light.png`|`386094f08fc5cb3714d5b6bc1b34e93a2b0e4d086266698b6c66f28c5aedee58`|

## Section4 Applications native evidence — requires8eedcc0

Actual private Xvfb windows with labelled local desktop-entry fixtures and safe
typed launch sink. Dark100 at960×720 and light200 at960×420. Source-style list,
not grid; native200row84 is a deliberate fix for measured source overlap. No
All/Files/Plugins provider claim; no matched full-modal pixel-parity claim.

|File|SHA256|
|---|---|
|`replica-native-section4-launcher-720-100-dark-top.png`|`c2ff19f9ab54e4171cfd1bf703c7741b55c2b82209989e225e6b6a2613fd9444`|
|`replica-native-section4-launcher-420-200-light-top.png`|`0155d9ec30abe5fab8f89455664c84003ce7c2b01f0cac470c54034df715ebb1`|
