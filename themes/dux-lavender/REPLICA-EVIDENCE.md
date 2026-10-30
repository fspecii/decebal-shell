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

## Section3 actual source DClockFace — native comparison pending

Eight actual component grabs at content102×184/408×400, dark/light100/200,
09/08 and Friday,30 October. Original variable font, ROND100/wght750; source-derived
opsz92/200. Transparent component canvas; no painted mock or reference wallpaper.
Raw metrics and limitations are in the port ledger. font.files stays unchanged.

|File|SHA256|
|---|---|
|`replica-upstream-clock-102x184-dark-100.png`|`65e47c093dfc9a7374ca16c188b44e4151363c521410635930557252ac2e1eab`|
|`replica-upstream-clock-408x400-dark-100.png`|`bb73076abbebff1bc9f971df399c4c744769e64f6fa875bfa8df3f55bf0cb342`|
|`replica-upstream-clock-102x184-dark-200.png`|`b09d89d2b8b6afe9cd8691af7d20fc30ae192e070019ec13ff9644e6ac14fa9e`|
|`replica-upstream-clock-408x400-dark-200.png`|`bb73076abbebff1bc9f971df399c4c744769e64f6fa875bfa8df3f55bf0cb342`|
|`replica-upstream-clock-102x184-light-100.png`|`711127fc62398acc41b1236b609008e0626c3072521d858fcad0950361d3e528`|
|`replica-upstream-clock-408x400-light-100.png`|`6e459e8901c6b9a6bb9507695d2898b82dcf7aa1075656847ef2f25cec6f8d90`|
|`replica-upstream-clock-102x184-light-200.png`|`36fae9e0068d1d5b6813b4901fce5edcd9944e982bf6661ce8cdc12c6b6cd933`|
|`replica-upstream-clock-408x400-light-200.png`|`6e459e8901c6b9a6bb9507695d2898b82dcf7aa1075656847ef2f25cec6f8d90`|

## Historical Section 3 component evidence — a8068ec (superseded)

This historical component probe preceded the production-grid 3f87285 port. Same-size actual native
clock crops,102×184/408×400,dark/light100/200,09/08/date fixed samples; no live
telemetry claim. Eight source/native digit boxes within2px; source and native
rasterizers differ. Native date elision intentionally preserves bounds at minimum
12×text-scale. Fifth font.files entry now registers the exact original variable;
static UI faces are unchanged. Full evidence/metrics/comparison:
`/tmp/decebal-dux-clock-final`, including source-comparison.json.
Native-owner gates:16normal+4missing renders, identical separate reduced-motion
process outputs,8actual dashboard windows including short-output scrolling. Native
X11 library1635pass/0fail/16ignored and strict all-target Clippy pass per handoff.
Native Wayland host/seconds/analog/animation are not accepted by these gates.

|File|SHA256|
|---|---|
|`replica-native-clock-102x184-dark-100.png`|`b55150e18d10344f63468b026b6cce4477fe560a4d297374dfa978f815fd76f9`|
|`replica-native-clock-102x184-dark-200.png`|`6d977d7ad1d54e0f96cff9a9861e07e96a191dc65b7db12c1309a7ba8680c87a`|
|`replica-native-clock-102x184-light-100.png`|`e9a745627fe9e2caf7d73a122b7f26f8810b8e7b41e1141695fe1bab9505e7bd`|
|`replica-native-clock-102x184-light-200.png`|`8830b59a798933b4ae47a9d43bc11408870f51f387b18c638b455b16a83c5e2a`|
|`replica-native-clock-408x400-dark-100.png`|`4d0f841c9d290d97f956536063544cc8934b57415bab5ef4aa6fc5500fe4918a`|
|`replica-native-clock-408x400-dark-200.png`|`4d0f841c9d290d97f956536063544cc8934b57415bab5ef4aa6fc5500fe4918a`|
|`replica-native-clock-408x400-light-100.png`|`31626764276eabd7b04c000be219a6d33b294f3b2d43c867793890dcb46b4398`|
|`replica-native-clock-408x400-light-200.png`|`31626764276eabd7b04c000be219a6d33b294f3b2d43c867793890dcb46b4398`|


## Production clock activation (Decebal 0.2.1 / Dux Lavender 1.3)

The original registered variable font now requires Decebal 0.2.1. Earlier
four-font Dux 1.2 packages keep their existing clock path. The production
renderer is integrated from 3f87285, preserving the six-column 128/8/96 grid;
its 96×168 content cell is the current reference. Earlier 102×184 and 408×400
clock captures remain historical component evidence, not proof of the current
whole Dashboard. See ../../docs/native-dux-clock.md for current geometry and source
comparison limits. Hot text-size changes invalidate supporting-text coverage
even below 100%, when the panel grid geometry remains clamped.
