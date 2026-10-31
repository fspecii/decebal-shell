# Registration notes for root (no patch is shipped)

An earlier revision carried `registration.patch`; it had stale counts and context, and its test stub listed only OpenKylin/Adwaita styles for **every** mode-aware theme, so it was
removed. Root registers the family manually, with its own multi-family schema proof. What this port needs, as facts rather than a diff:

* **Theme ids** to register: `openkylin`, `openkylin-islands`, `openkylin-light-seeking`, `openkylin-heyin`, `openkylin-classic` (folders `themes/<id>/`). Each requests the measured
  `openkylin` / `openkylin-dark` window-style pair with fallbacks `adwaita-light` / `adwaita`; any test that applies every built-in theme with a stub target `rdwmctl` must list the
  styles of **every** family that uses the mode keys (not one family's), otherwise the apply is correctly refused ("could not be checked").
* **Wallpaper catalogue** (`data/wallhaven-curated.json`): `tests/curated_theme.rs` needs every shipped theme to have a record and every image to be used and distinct. The fragment
  `assets/desktop-ports/openkylin/wallpapers.json` (5 themes, 10 images) is shaped like the catalogue; `tools/merge_catalogue.py IN OUT` inserts it (refuses a clash). The lane test accepts
  both states: before integration (disjoint, the merge succeeds, invariants hold) and after (the catalogue's records for these ids are **deep-equal** to the fragment's).
* **Asset catalogue:** list `assets/desktop-ports/openkylin/asset-packs.toml` where the asset-pack module lists families (one `include_str!`).
* **Counts** change with every other family landing, so none is stated here; derive them from the tree at integration time.
* Historical note, not a status claim: in an earlier private scratch merge (root `d8d03e1` + the Lingmo lane + this branch) two unit tests (`golden::every_builtin_resolves_to_the_same_theme_as_before`,
  `cli::tests::the_whole_workflow_from_new_to_apply_and_undo`) failed with and without these packages. That scratch branch is not root; nothing here claims a green suite.
