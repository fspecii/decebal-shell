# Contract for the shared launcher engine: style-owned targets (for root, to merge with Deepin and UKUI)

The Kickoff footer needs tabs, captioned buttons and a menu that no other launcher style has. Rather than add a `Target` variant per desktop, the shared
engine (`src/launcher/styled.rs`) gets one generic hook that any style can use; nothing else about the other styles changes. Everything here is additive
and small; the Kickoff code lives in `src/launcher/styled/kde.rs` and `src/launcher/styled/kde/`.

| Shared piece | Meaning |
| --- | --- |
| `Target::Style(u8, u16)` | A target whose kind and index only its own style understands (a tab, a button, a menu row, a list row). `Target::focusable` is true; hit testing, focus and keyboard walking treat it like any other target. |
| `enum Outcome { Nothing, Redraw, Relayout, Close }` | What a style's own action did. `Styled::activate` turns it into the usual `Response` (`Relayout` runs `relayout()` first). |
| `Styled::activate(Target::Style(k, i))` | Calls the style's `activate(&mut Model, k, i) -> Outcome` (only `LauncherStyle::Kde` is wired; every other style answers `Nothing`). |
| `Model.ui: HashMap<&'static str, usize>`, `Model::ui(key)` | Small integers a style keeps for itself (current tab, an open menu). Keys are the style's own namespace (`"kde.tab"`). |
| `Model.entries: HashMap<&'static str, Vec<Entry>>`, `struct Entry { name, detail, icon, open: Vec<String>, at: u64 }` | Lists a style may fill; Kickoff no longer uses it for Places (they come from a single-flight worker and a store in `kde/places.rs`, see `kickoff-places-hook.md`, which also describes the one host hook still needed to wake the loop). `open` is the argv that opens a row; `at` orders a history. |
| `kde::escape(&mut Model) -> bool`, `kde::press(&mut Model, Option<zone>) -> bool` | Hooks in `Styled::key` (Escape) and `Styled::button`: a style's open menu or pending confirmation takes Escape first, and a press outside it is consumed by closing it. |
| `Zone::horizontal()` | A row is now a fixed zone whose items share a vertical **centre** (was: the same top edge), so full-height tabs and shorter buttons form one row for the arrow keys. Same result for every existing zone. |
| `History::frequent(prefix, n)`, `History::last_used(key)` (`fuzzy.rs`), `Catalog::frequent(n)`, `Catalog::last_launch(i)` (`catalog.rs`) | The launch count and last-use time the history already stored, exposed read-only. |

A Deepin or UKUI style adds its own `LauncherStyle::X => x::activate(...)` arm in the same two places (`activate` dispatch, and `escape` / `press` if it has
transient menus), its own kind constants, and keeps its state in `Model.ui` / `Model.entries`. Nothing needs another `Target` variant.

One more shared line, already in this change and flagged for review: the natural size of the Kde style in `placement.rs` is now 655 x 516 (it was 620 x 560); the placement test that asserted exact centring now allows the half pixel an odd width rounds to.
Root's `docs/launcher-styles.md` "KDE Kickoff" fidelity gap (only the Applications tab, Leave opens decsession) is superseded by `kickoff-fidelity.md`.
