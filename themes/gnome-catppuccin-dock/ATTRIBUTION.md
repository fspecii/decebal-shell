# Attribution

The package manifest and these notes are original Decebal content under the MIT licence of this repository (`LICENSE.txt`).

## Catppuccin palette (MIT)

The Mocha and Latte colour values are the Catppuccin palette 1.8.0, `catppuccin/palette` commit
`07d02aa110ef9eb7e7427afca5c73ba9cf7f8ebd` (`palette.json`). Its licence, reproduced as required:

```
MIT License

Copyright (c) 2021 Catppuccin

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Reference configuration (read only, nothing copied)

Geometry and defaults were measured from the following projects; they are GPL-3.0 / GPL-2.0 extensions and no code, stylesheet or asset
from them is included in this package or in the MIT engine:

* **Dash to Dock** (`ubuntu/dash-to-dock`, GPL-2.0-or-later) `2dfae3474d6984b566ef0c3dab1524a675bbba48`: `schemas/*.gschema.xml`
  defaults and `_stylesheet.scss` dash metrics.
* **Blur my Shell** (`aunetx/blur-my-shell`, GPL-3.0) `f13bdcf36be5e1d8b62203c77face3303f0860af`: default brightness 0.6, sigma 30.
* **Just Perfection** (`jrahmatzadeh/just-perfection`, GPL-3.0) `a46f18957422210411f5675e462c73d47e79266e`: the panel Activities-button and
  panel-size options.
* **libadwaita** 1.10.0 `c9ab118612eee92324143783c4641c98eaded3f0` and **GNOME Shell** 51.1 `33da06854e229c256afd5872fedd83c28cbfa176`
  (LGPL-2.1-or-later): the window radius, headerbar height, panel height and dash metrics.

"GNOME" and "Catppuccin" are used only to name the designs being reproduced; this package is not affiliated with or endorsed by either.

## Bundled font

`fonts/MaterialSymbolsRounded.ttf` is the exact upstream Material Symbols Rounded variable font (Google, Apache-2.0, full text in
`MaterialSymbolsRounded-LICENSE.txt`), byte-identical to the copy in `themes/dux-lavender`. It is bundled only because the native bar
(`bar.style = "dux"`, the only bar that honours item lists today) paints its item glyphs from it; it is registered inside the shell
process only. These are Material glyphs, not Adwaita symbolic icons: that mismatch is a documented framework blocker in
`docs/gnome-theme-ports.md`.
