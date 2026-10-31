# openKylin / UKUI: materials, toolkit contract, decoration states, icons (measured, with what is routed)

Machine-readable twin: `assets/desktop-ports/openkylin/toolkit-profiles.json` (sources, commits, file hashes). Palettes alone do not satisfy the port; this page separates what this lane did from what other owners must do.

## 1. Composited material (this lane; done for the start menu and the shortcut panel)

Source: `ukui-menu/qml/mainXcb.qml` draws the card with `DtThemeBackground` (the **window** colour, transparency from the global theme: `transparencyBlur` 65 %, HeYin 75 %, with blur; `transparencyNoBlur` 90 % without) over a KWin blur region, plus a 1 px `textActive` border at alpha 0.15. The earlier port drew the **menu** colour (`kmenu`) at >= 78 %.

Now: `overlay` = `window-active`, `panel_alpha` = `transparencyBlur`; the start menu and the shortcut panel draw the window colour at `panel_alpha` when the theme asks for blur (`blur > 0`, the shell then sets the compositor glass hint) and at 0.90 otherwise; an opaque surface (no compositor) is opaque. Text, icons and controls are always drawn opaque on top.

Proof: `tests/e2e/openkylin-native.py --matrix material` composites the real card over a **synthetic** high-contrast backdrop (4 px black/white columns; not a UKUI screenshot, not original-application proof) on the private fixture (picom, gaussian blur) with and without blur: the empty card area must equal `alpha x window + (1 - alpha) x backdrop mean`, be smooth with blur and show the stripes at `(1 - alpha)` without it, and the app-row text must reach the ink colour. Limits: the compositor decides whether the hint blurs (picom here; KWin in UKUI); `menu-transparency 72` of `org.ukui.style` (Qt menus) is not the same number as our `popup_alpha` 0.82 (v11 `kmenu`), not reconciled.

## 2. Toolkits: GTK is not Qt (routed; not edited here)

* **Qt (Peony, UKUI applications)** are styled by the Qt platform theme `QT_QPA_PLATFORMTHEME=ukui` (`qt5-ukui-platformtheme`, style key `ukui`) reading the GSettings schema `org.ukui.style`: `style-name` (ukui-light / ukui-dark / ukui-default), `icon-theme-name`, `widget-theme-name` (default / classical / fashion; DawnLight uses `dawnlight`), `theme-color`, `system-font(-size)`, `window-radius` 12, `enabled-global-blur`, `menu-transparency` 72, `peony-side-bar-transparency` 72. A GTK theme does **not** style these applications, and nothing in this lane claims GTK covers Qt. Decebal can at most write those keys when the schema is installed; whether Peony then renders the original look needs the platform theme binary and a private-profile capture of Peony itself: **not done**.
* **GTK** uses `ukui-white` / `ukui-black` (`ukui-themes`, SCSS sources under `themes/*/gtk-3.0`, compiled at package build). Accents: v11/DawnLight #3676F5, Light-Seeking and Classic #3790FA, **HeYin #6A287E (differs from the shell's HeYin brand token #8926EB)**.

Per-profile table (`toolkit-profiles.json`): effect (radius, transparency, animation), GTK names and accent, Qt style/widget theme/accent role, icon and cursor theme, for the four upstream global themes. Needs for the GTK specialist (via root): the names and accents above per package and mode, and that the `ukui-*` GTK themes need a build step the asset installer does not run. Needs for the file-manager lane (via root): Peony is Qt; the GitHub clone here is from 2023 and the Gitee repository was not read.

## 3. Window decorations: active and inactive (measured; routed to the decorations owner)

`kwin` 01bf914, `ukui-decoration.cpp`: frame light active `#ffffff` / inactive `#f5f5f5`, dark active `#121212` / inactive `#1c1c1c`; title font `#262626` / `#cfcfcf`; 1 px border at alpha 0.15 (black / white); shadow active 38 px, shift 22, alpha 0.60, **inactive 25 px, shift 15, alpha 0.40**; the button icons take an `-inactive` variant. These constants are hard-coded in the plugin and are not the global-theme window tokens (`#f5f5f5` / `#2e2e2e`) used for `titlebar` here. In the source tables `window-inactive` equals `window-active` for Light-Seeking and HeYin, so their package palettes have no inactive colour change (asserted in `tests/openkylin_port.py`). No pixel proof of active/inactive frames on a mapped window exists in this lane.

## 4. Icons and cursors

Matching packs per package are in `asset-packs.toml` (default / classical / fashion icons; Dark Sense, DMZ-White cursors). The automatic installation on theme apply belongs to the asset specialist. Gaps: **`ukui-icon-theme-dawnlight`**, the icon theme of the current default global theme DawnLight (accent and radius match v11), exists in no reachable repository (not in Gitee `openkylin-theme` 0f348e4 nor `ukui-themes` ddd06fe); the v11 packages keep suggesting the default pack.

## 5. Not done

Peony / file-manager rendering, Qt widget styling (`ukui` platform theme), GTK theme installation, active/inactive frames on a real window, the cursor on screen, the 72 % menu transparency reconciliation, HeYin's default-dark mode (not expressible in a package), the DawnLight icon pack.
