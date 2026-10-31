# Handoff: island cluster increment (against root `38dafed`)

Base: this branch merged root `38dafed` (`bc9e56b`, root's versions of `tests/openkylin_port.py` and `tests/e2e/openkylin-native.py` kept) and then added the increment below as separate commits
(`5f6f2fd` … `HEAD`); the reviewed history up to `6d9b079` is preserved. Nothing was pushed, installed, restarted or run against a live session.

## Shared-layout hunks (generic, no theme id) — for root and the universal-panel owner to review

| file | change |
|---|---|
| `src/config.rs` | `BarLayout`: `cluster`, `group_gap`, `group_padding` (clamped); the group lists also accept applet registry keys (`AppletId::parse`) |
| `src/menubar/segmented.rs` | `place(..., cluster)`: one run, centred, content-sized, face per group; stock placement untouched when `None`; `Layout::cluster` / `faces()`; face outline in cluster mode; call sites |
| `src/menubar/segmented/cluster_tests.rs` | new: numbers against the pinned ukui-panel source, content sizing, centring, empty groups, widths 1920 → 160, stock layout unchanged, key clamping, manifest checks |
| `src/theme/engine/schema.rs` | fields `bar.cluster`, `bar.group_gap`, `bar.group_padding`; `bar.left/center/right` description |
| `src/theme/engine/manifest.rs` | Dux list validation accepts registry keys (unknown ids still refused) |
| `src/theme/engine/apply.rs` | the three keys are applied and reset with the other bar layout keys (two lines; the existing test fixtures are unchanged) |
| `src/panel/mod.rs` | the three keys count as 0.3.0-framework keys in `needs_framework` |

Not touched: `docs/universal-panels.md`, vertical bar (`vbar.rs`), root `bar_height` / exported menus / panel metrics paths (preserved by the merge).

## Port-only files of the increment

`themes/openkylin-islands/theme.toml` (generated), `assets/desktop-ports/openkylin/tools/{build_packages,native_summary}.py`, `tests/openkylin_port.py` (island assertions), `tests/e2e/openkylin-native.py`
(cluster measurement; `tests/e2e/openkylin_fixture.py` remains its dependency), `docs/desktop-ports/openkylin/{README,integration-contract,island-cluster,island-cluster-handoff}.md`, and the regenerated
`docs/desktop-ports/openkylin/native/` evidence (JSON, README, PNGs).

## What was run (scoped; no whole-suite claim)

Private scratch clone of `da3ba72` (engine + package), private target, `-j2`, `--offline`, `flock --close` on the build slot, cleared environment:

* `cargo test --lib -- cluster_tests config:: theme::engine::manifest theme::engine::apply menubar::segmented panel::` → **104 passed, 0 failed, 1 ignored** (the filter, not the suite);
* `cargo test --test openkylin_desktop_port` → 1 passed;
* `python3 tests/openkylin_port.py` → 23 tests OK (2 skipped without binaries);
* native: 132 cases on private owned displays ≥ 700 with the scratch binaries (`decthemectl 87e7dc20…`, `decdock fae4a15d…`, `decbar ad837030…`): colour Δ ≤ 1/255; 24 cluster measurements (islands, bottom and top, 100/125/200 %,
  1920×1080 and 1280×560, light and dark): three faces, gaps = 8 × scale (±2), face height 48 × scale, 12 × scale from the edge, centred on the bar within 2 px; side edges captured (turned band, unchanged).

Earlier whole-suite baseline failures of the old scratch branch (`registration-notes.md`) are historical and are not re-run or claimed here.

## Still not claimed

Full UKUI launcher or Control Center replica; drag handles, island min/max widths and scrolling; the Dux renderer's blank icon-only cells without a package Material Symbols font (a 15 MB Apache-2.0 file this package does
not ship). The icon packs' `from moka` provenance stays **unresolved** (unchanged).


Root integration follow-up: shared semantic icon fallback is now supplied when package Material fonts are absent; canonical applet aliases are deduplicated across groups, and decorative padding yields to the overflow target on narrow outputs. The captures and 104-test claim above describe the earlier scratch build, not those later fixes. Current root acceptance is recorded separately.
