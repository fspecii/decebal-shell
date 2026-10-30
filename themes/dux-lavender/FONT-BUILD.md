# Dux upstream typography assets

Source: read-only dux-shell-reference87312075dd414a6f60691464bcd6c6a9bb242a7e,
pinned dux-qml-common43e05926fcdacc53675623169bcef686faa95301.
Google Sans Flex is SIL OFL1.1; complete OFL.txt retained. Material Symbols Rounded
is Apache2.0; complete MaterialSymbolsRounded-LICENSE.txt retained. Font embedded
copyright/name metadata is preserved. These licences are separate from packageMIT.
No font installed globally; no fc-cache/config edits.

GoogleSansFlex-400.ttf is a STATIC regular instance of upstream GoogleSansFlex.ttf,
all axes pinned to source defaults: opsz18,wdth100,wght400,GRAD0,ROND0,slnt0. This
removes fvar/gvar variability from the ordinary UI fontdue path;
it does not substitute a different typeface. GoogleSansFlex-500/700.ttf are copied
unchanged from upstream's own static instances. The original variable regular is now bundled separately as
`fonts/GoogleSansFlex-Variable.ttf` for section3; it is the fifth font.files entry after the validated native
`a8068ec` handoff. Static UI faces remain unchanged. All3static faces passed actual native fontdue rasterization for time,
date,media andsystem sample strings, with nonempty coverage per nonspace glyph.

Generation: official PyPI fonttools4.68.0 pure-Python wheel downloaded and SHA256
verified, then extracted ONLY under/tmp/decebal-dux-replica-specialist/font-build.
No pip/site-package/system installation. WheelSHA256:
`d76f8fdcb6936f5e1045a0c9dd721a4bf11112e60d1e243bfa1d39f06a8b3104`.
SourceAPI:https://pypi.org/pypi/fonttools/4.68.0/json

```python
font=TTFont(source,recalcTimestamp=False)
axes={a.axisTag:a.defaultValue for a in font['fvar'].axes}
axes['wght']=400
instance=instantiateVariableFont(font,axes,inplace=False)
instance.recalcTimestamp=False
instance.save('GoogleSansFlex-400.ttf')
```

MaterialSymbolsRounded.ttf is the exact upstream variable font, copied without
alteration. Native implementation owner supplies process-private PUA glyph rendering;
upstream DIcon.qml uses FILL0,GRAD-25dark/0light,opsz24,wght400 unfilled (500filled).
Do not claim ordinary regular-font rasterization covers those variable icons.

The package font.files registration uses the validated process-private FontDb contract. Merely bundling these files is not runtime parity.

## Exact hashes

Variable regular source SHA256: `c6d53424121196b81de816b8daccf200e285dd506df43766db3d7e8cdf06ee30`.

| File | Bytes | SHA256 |
| --- | ---: | --- |
|GoogleSansFlex-400.ttf|129988|`7f3c9201743b4f2f261de099edc0ea54a68d9376ae5f5e12db21bdf0c66063ab`|
|GoogleSansFlex-500.ttf|130268|`a967ac53d4464d3009c636b6e7fd907b1ff62294eadecf47438cdf7faed7ed16`|
|GoogleSansFlex-700.ttf|130476|`e2de2a038b23a40e45fa5b7b52a3c52236d1045f065627af7eef81c940d2254d`|
|MaterialSymbolsRounded-LICENSE.txt|11358|`cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`|
|MaterialSymbolsRounded.ttf|15141224|`7f3a16d9aa1b6f367eadc1614ad27308ae2c0e91e617e2069f122afd9f0c3c58`|
|OFL.txt|4393|`425676ce436ceb37cdcd172e0bd4946f24c9eb3ed245b22971e60f5993d95723`|

## Section3 variable-clock registration — native a8068ec

Exact upstream original `DCommon/assets/fonts/google-sans-flex/GoogleSansFlex.ttf`
from pinned common43e05926 copied unchanged to fonts/GoogleSansFlex-Variable.ttf.
4153300 bytes; SHA256 `c6d53424121196b81de816b8daccf200e285dd506df43766db3d7e8cdf06ee30`.
Covered by existing complete package-root OFL.txt; no font conversion or new licence.
Native owner proved existing ttf-parser0.21 optional gvar-alloc handles the source
font's215 variation tuples for glyph A, exceeding its default32 stack bound.
Renderer feature/cache and axis QA remain native ownership. Following the validated
a8068ec handoff, font.files appends this original asset as fifth entry; the first
four and their order stay unchanged. Parent controls integration/live activation.
No font install or cache/config edit.
