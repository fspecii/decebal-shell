#!/usr/bin/env bash
# The bar's weather widget end to end, with fake data only: a canned wttr.in / Open-Meteo answer from tests/fakebin/curl (no network),
# a private HOME / XDG cache and a private Xvfb. Checks that the weather popup renders (macOS bar, caelestia pill bars (side and top))
# and that the fetch path fills it. Screenshots in $SHOTS.
#   tests/e2e/weather.sh        (E2E_DISPLAY=:96 by default; MS_BIN_DIR=dir with the built binaries, default target/release)
set -u
HERE=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${MS_BIN_DIR:-${CARGO_TARGET_DIR:-$HERE/target}/release}
[ -x "$BIN/decbar" ] || { echo "decbar not built in $BIN"; exit 77; }
for t in Xvfb import convert; do command -v $t >/dev/null || { echo "$t needed"; exit 77; }; done
N=${E2E_DISPLAY:-:96}
case "$N" in :0 | :1 | :2 | 0 | 1 | 2) echo "refusing a live display"; exit 2 ;; esac
PASS=0; FAIL=0; PIDS=()
TMP=$(mktemp -d /tmp/decebal-weather-e2e.XXXXXX)
SHOTS=${SHOTS:-$TMP/shots}; mkdir -p "$SHOTS"
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    wait 2>/dev/null
    if [ "$SHOTS" = "$TMP/shots" ]; then rm -rf "$TMP"; else find "$TMP" -mindepth 1 -maxdepth 1 ! -path "$SHOTS" -exec rm -rf {} +; rmdir "$TMP" 2>/dev/null; fi
}
trap cleanup EXIT
pass() { PASS=$((PASS + 1)); echo "  ok   $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $1${2:+  ($2)}"; }
ok() { if "${@:2}"; then pass "$1"; else fail "$1"; fi; }

export HOME=$TMP/home XDG_CONFIG_HOME=$TMP/cfg XDG_RUNTIME_DIR=$TMP/run XDG_DATA_HOME=$TMP/data XDG_DATA_DIRS=$TMP/nodata XDG_CACHE_HOME=$TMP/cache XDG_STATE_HOME=$TMP/state
export DECEBAL_CONFIG=$TMP/cfg/omarchy DECEBAL_BACKEND=x11 DISPLAY=$N FAKE_STATE=$TMP/fs DECBAR_POWER_SUPPLY=$TMP/ps
export RDWMCTL=/bin/true DECSETTINGS_CONFIG=$TMP/cfg/rdwm/config.toml
export PATH=$HERE/tests/fakebin:$BIN:$PATH
unset WAYLAND_DISPLAY RDWM_SOCK HYPRLAND_INSTANCE_SIGNATURE
mkdir -p "$HOME" "$XDG_CONFIG_HOME/rdwm" "$DECEBAL_CONFIG" "$XDG_RUNTIME_DIR" "$TMP/nodata" "$XDG_CACHE_HOME/decebal" "$XDG_STATE_HOME" "$FAKE_STATE" "$TMP/ps"
printf "[general]\n" > "$DECSETTINGS_CONFIG"; chmod 700 "$XDG_RUNTIME_DIR"; : > "$FAKE_STATE/log"
Xvfb $N -screen 0 1600x900x24 -nolisten tcp -ac >"$TMP/xvfb.log" 2>&1 & PIDS+=($!)
for _ in $(seq 100); do [ -e /tmp/.X11-unix/X${N#:} ] && break; sleep 0.05; done
[ -e /tmp/.X11-unix/X${N#:} ] || { echo "Xvfb failed"; exit 2; }

cache=$XDG_CACHE_HOME/decebal/weather.json
now=$(date +%s)
# a cached reading with everything the popup shows
printf '{"temp_c":14,"code":296,"night":false,"fetched":%s,"feels_c":12,"humidity":71,"wind_kmh":19,"wind_dir":"SW","desc":"Light rain","location":"Cacheton, Testland","days":[{"date":"2026-10-29","high_c":15,"low_c":8,"code":296},{"date":"2026-10-30","high_c":13,"low_c":6,"code":116},{"date":"2026-10-31","high_c":10,"low_c":3,"code":338}]}' "$now" > "$cache"

# png NAME ARGS...: render with decbar --png and require a non-empty image
png() { local n=$1; shift; decbar --png "$SHOTS/$n.png" --width 1600 $POS "$@" >"$TMP/out.log" 2>&1 && [ -s "$SHOTS/$n.png" ]; }
# Pixels that differ from the popup-less render: the popup really drew something.
diff_px() { compare -metric AE "$SHOTS/$1.png" "$SHOTS/$2.png" null: 2>&1 | grep -o '^[0-9]*'; }

# macOS menu bar (no --position), then the Caelestia pill bar on the left edge and along the top
for STYLE in macos caelestia-left caelestia-top; do
    case $STYLE in macos) POS=""; decthemectl style macos >/dev/null 2>&1;; caelestia-left) POS="--position left"; decthemectl style caelestia >/dev/null 2>&1;; *) POS="--position top";; esac
    ok "$STYLE: bar renders" png "wx-$STYLE-closed"
    ok "$STYLE: weather popup renders" png "wx-$STYLE-open" --open weather
    d=$(diff_px "wx-$STYLE-closed" "wx-$STYLE-open"); [ "${d:-0}" -gt 3000 ] && pass "$STYLE: the popup paints a panel ($d px differ)" || fail "$STYLE: popup missing" "${d:-none} px differ"
done
POS=""; decthemectl style macos >/dev/null 2>&1

# The fetch path: no cache, the fake curl answers; a real (non --png) bar shows the fetched location in the cache file.
rm -f "$cache"
decbar --png "$SHOTS/wx-fetch.png" --width 1600 --open weather >"$TMP/out.log" 2>&1
for _ in $(seq 60); do [ -s "$cache" ] && break; sleep 0.1; done
ok "fetch: the fake wttr.in answer was cached" grep -q '"location":"Fakeville, Testland"' "$cache"
ok "fetch: forecast days were cached" grep -q '"high_c":8' "$cache"
ok "fetch: only the fake curl was asked" bash -c "! grep -v '^curl' '$FAKE_STATE/log' | grep -q . ; grep -q 'wttr.in' '$FAKE_STATE/log'"

echo "$PASS passed, $FAIL failed (screenshots: $SHOTS)"
[ "$FAIL" = 0 ]
