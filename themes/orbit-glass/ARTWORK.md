# Original wallpaper artwork

These SVG compositions are original Decebal artwork, released under MIT with the theme. They contain no app UI, widget labels, logos, third-party illustrations or embedded fonts. The packaged PNGs are rendered at 3840 × 2160 with `rsvg-convert`. SVG is kept in this document because `.svg` files are intentionally not accepted inside theme packages.

## Dark

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 1920 1080">
<defs>
 <linearGradient id="field" x2="1" y2="1"><stop stop-color="#111a33"/><stop offset="1" stop-color="#050814"/></linearGradient>
 <radialGradient id="haze"><stop stop-color="#566fc0" stop-opacity=".23"/><stop offset="1" stop-color="#566fc0" stop-opacity="0"/></radialGradient>
 <linearGradient id="ribbon" x1="0" y1="1" x2="1" y2="0"><stop stop-color="#9c8fd7" stop-opacity=".08"/><stop offset=".25" stop-color="#63a0b5" stop-opacity=".48"/><stop offset=".48" stop-color="#c5e9f4" stop-opacity=".84"/><stop offset=".58" stop-color="#d5ccff" stop-opacity=".25"/><stop offset=".86" stop-color="#6880b8" stop-opacity=".18"/><stop offset="1" stop-color="#9fcacb" stop-opacity=".58"/></linearGradient>
 <linearGradient id="rim" x1="0" y1="1" x2="1" y2="0"><stop stop-color="#7897b5" stop-opacity=".04"/><stop offset=".3" stop-color="#adccdc" stop-opacity=".35"/><stop offset=".5" stop-color="#f1ffff" stop-opacity=".85"/><stop offset=".8" stop-color="#ab97e5" stop-opacity=".28"/><stop offset="1" stop-color="#7ca0bf" stop-opacity=".08"/></linearGradient>
 <filter id="soft"><feGaussianBlur stdDeviation="55"/></filter>
 <filter id="glow"><feGaussianBlur stdDeviation="8"/></filter>
</defs>
<rect width="1920" height="1080" fill="url(#field)"/>
<ellipse cx="700" cy="520" rx="830" ry="620" fill="url(#haze)"/>
<g opacity=".11" stroke="#93a6d0" stroke-width=".5">
 <path d="M0 879H1920M0 949H1920M0 1019H1920M99 0V1080M1821 0V1080"/>
 <path d="M96 90h24m-12-12v24M1797 90h24m-12-12v24M96 1000h24m-12-12v24M1797 1000h24m-12-12v24"/>
</g>
<g transform="translate(685 570) rotate(-29)">
 <ellipse rx="659" ry="316" fill="none" stroke="#7794e6" stroke-width="70" opacity=".07" filter="url(#soft)"/>
 <ellipse rx="639" ry="293" fill="none" stroke="url(#ribbon)" stroke-width="48"/>
 <ellipse rx="664" ry="317" fill="none" stroke="url(#rim)" stroke-width="1.3"/>
 <ellipse rx="616" ry="270" fill="none" stroke="url(#rim)" stroke-width="1"/>
 <ellipse rx="641" ry="295" fill="none" stroke="#c7e6f9" stroke-width="2" opacity=".20" filter="url(#glow)"/>
 <ellipse rx="531" ry="224" fill="none" stroke="#bedbff" stroke-width=".7" opacity=".10"/>
</g>
<g transform="translate(706 554) rotate(46)">
 <ellipse rx="345" ry="468" fill="none" stroke="url(#ribbon)" stroke-width="15" opacity=".30"/>
 <ellipse rx="354" ry="477" fill="none" stroke="url(#rim)" stroke-width=".8" opacity=".50"/>
</g>
<g fill="none" stroke="#bedbff" opacity=".14" stroke-width=".7"><circle cx="685" cy="570" r="38"/><path d="M631 570h108m-54-54v108"/></g>
<circle cx="146" cy="180" r="1" fill="#bedbff" opacity="0.5"/><circle cx="328" cy="944" r="1.2" fill="#bedbff" opacity="0.35"/><circle cx="590" cy="124" r="1" fill="#bedbff" opacity="0.4"/><circle cx="1374" cy="276" r="1.2" fill="#bedbff" opacity="0.4"/><circle cx="1690" cy="730" r="1" fill="#bedbff" opacity="0.25"/><circle cx="106" cy="654" r="1" fill="#bedbff" opacity="0.3"/><circle cx="1166" cy="928" r="1" fill="#bedbff" opacity="0.3"/><circle cx="1810" cy="164" r="1" fill="#bedbff" opacity="0.3"/><circle cx="464" cy="818" r="1.4" fill="#bedbff" opacity="0.5"/><circle cx="1130" cy="118" r="1" fill="#bedbff" opacity="0.3"/>
</svg>
```

## Light

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 1920 1080">
<defs>
 <linearGradient id="field" x2="1" y2="1"><stop stop-color="#f8fbff"/><stop offset="1" stop-color="#dbe3ef"/></linearGradient>
 <radialGradient id="haze"><stop stop-color="#566fc0" stop-opacity=".13"/><stop offset="1" stop-color="#566fc0" stop-opacity="0"/></radialGradient>
 <linearGradient id="ribbon" x1="0" y1="1" x2="1" y2="0"><stop stop-color="#9c8fd7" stop-opacity=".08"/><stop offset=".25" stop-color="#63a0b5" stop-opacity=".48"/><stop offset=".48" stop-color="#c5e9f4" stop-opacity=".84"/><stop offset=".58" stop-color="#d5ccff" stop-opacity=".25"/><stop offset=".86" stop-color="#6880b8" stop-opacity=".18"/><stop offset="1" stop-color="#9fcacb" stop-opacity=".58"/></linearGradient>
 <linearGradient id="rim" x1="0" y1="1" x2="1" y2="0"><stop stop-color="#7897b5" stop-opacity=".04"/><stop offset=".3" stop-color="#adccdc" stop-opacity=".35"/><stop offset=".5" stop-color="#f1ffff" stop-opacity=".85"/><stop offset=".8" stop-color="#ab97e5" stop-opacity=".28"/><stop offset="1" stop-color="#7ca0bf" stop-opacity=".08"/></linearGradient>
 <filter id="soft"><feGaussianBlur stdDeviation="55"/></filter>
 <filter id="glow"><feGaussianBlur stdDeviation="8"/></filter>
</defs>
<rect width="1920" height="1080" fill="url(#field)"/>
<ellipse cx="700" cy="520" rx="830" ry="620" fill="url(#haze)"/>
<g opacity=".11" stroke="#879cb7" stroke-width=".5">
 <path d="M0 879H1920M0 949H1920M0 1019H1920M99 0V1080M1821 0V1080"/>
 <path d="M96 90h24m-12-12v24M1797 90h24m-12-12v24M96 1000h24m-12-12v24M1797 1000h24m-12-12v24"/>
</g>
<g transform="translate(685 570) rotate(-29)">
 <ellipse rx="659" ry="316" fill="none" stroke="#7794e6" stroke-width="70" opacity=".07" filter="url(#soft)"/>
 <ellipse rx="639" ry="293" fill="none" stroke="url(#ribbon)" stroke-width="48"/>
 <ellipse rx="664" ry="317" fill="none" stroke="url(#rim)" stroke-width="1.3"/>
 <ellipse rx="616" ry="270" fill="none" stroke="url(#rim)" stroke-width="1"/>
 <ellipse rx="641" ry="295" fill="none" stroke="#c7e6f9" stroke-width="2" opacity=".20" filter="url(#glow)"/>
 <ellipse rx="531" ry="224" fill="none" stroke="#73879e" stroke-width=".7" opacity=".10"/>
</g>
<g transform="translate(706 554) rotate(46)">
 <ellipse rx="345" ry="468" fill="none" stroke="url(#ribbon)" stroke-width="15" opacity=".30"/>
 <ellipse rx="354" ry="477" fill="none" stroke="url(#rim)" stroke-width=".8" opacity=".50"/>
</g>
<g fill="none" stroke="#73879e" opacity=".14" stroke-width=".7"><circle cx="685" cy="570" r="38"/><path d="M631 570h108m-54-54v108"/></g>
<circle cx="146" cy="180" r="1" fill="#73879e" opacity="0.5"/><circle cx="328" cy="944" r="1.2" fill="#73879e" opacity="0.35"/><circle cx="590" cy="124" r="1" fill="#73879e" opacity="0.4"/><circle cx="1374" cy="276" r="1.2" fill="#73879e" opacity="0.4"/><circle cx="1690" cy="730" r="1" fill="#73879e" opacity="0.25"/><circle cx="106" cy="654" r="1" fill="#73879e" opacity="0.3"/><circle cx="1166" cy="928" r="1" fill="#73879e" opacity="0.3"/><circle cx="1810" cy="164" r="1" fill="#73879e" opacity="0.3"/><circle cx="464" cy="818" r="1.4" fill="#73879e" opacity="0.5"/><circle cx="1130" cy="118" r="1" fill="#73879e" opacity="0.3"/>
</svg>
```

