# Original eclipse field artwork

Copyright (c) 2026 Decebal contributors. MIT; see LICENSE.txt. Original vector artwork, not an upstream screenshot or copied wallpaper. It contains no simulated UI. These restrained eclipse studies support the right-side navigation rail and leave the upper-right work area quiet. The SVG source below is complete; render at 4K using `rsvg-convert -w 3840 -h 2160 source.svg -o dark.png`.

## Dark

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 1920 1080">
<defs>
 <linearGradient id="field" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#101d27"/><stop offset="1" stop-color="#070c14"/></linearGradient>
 <radialGradient id="halo"><stop offset=".55" stop-color="#366a7c" stop-opacity=".35"/><stop offset="1" stop-color="#366a7c" stop-opacity="0"/></radialGradient>
 <radialGradient id="world" cx=".22" cy=".18" r=".94"><stop stop-color="#253d4b"/><stop offset=".48" stop-color="#111b2b"/><stop offset="1" stop-color="#050a13"/></radialGradient>
 <linearGradient id="rim" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#aad7dc" stop-opacity=".90"/><stop offset=".25" stop-color="#aad7dc" stop-opacity=".30"/><stop offset=".55" stop-color="#aad7dc" stop-opacity="0"/><stop offset="1" stop-color="#aad7dc" stop-opacity=".06"/></linearGradient>
 <linearGradient id="orbit"><stop stop-color="#aad7dc" stop-opacity="0"/><stop offset=".48" stop-color="#aad7dc" stop-opacity=".20"/><stop offset="1" stop-color="#aad7dc" stop-opacity="0"/></linearGradient>
 <clipPath id="disk"><circle cx="585" cy="598" r="362"/></clipPath>
</defs>
<path fill="url(#field)" d="M0 0h1920v1080H0z"/>
<ellipse cx="500" cy="500" rx="680" ry="630" fill="url(#halo)"/>
<g fill="none" stroke="#8ca8b8" opacity=".045" stroke-width=".6"><path d="M0 180h1920M0 360h1920M0 540h1920M0 720h1920M0 900h1920M240 0v1080M480 0v1080M720 0v1080M960 0v1080M1200 0v1080M1440 0v1080M1680 0v1080"/></g>
<ellipse cx="650" cy="608" rx="660" ry="198" transform="rotate(-24 650 608)" fill="none" stroke="url(#orbit)" stroke-width="1"/>
<circle cx="585" cy="598" r="362" fill="url(#world)"/>
<g clip-path="url(#disk)" fill="none" stroke="#aad7dc" stroke-width=".7" opacity=".07"><ellipse cx="585" cy="598" rx="362" ry="120" transform="rotate(-24 585 598)"/><ellipse cx="585" cy="598" rx="362" ry="240" transform="rotate(-24 585 598)"/><ellipse cx="585" cy="598" rx="120" ry="362" transform="rotate(-24 585 598)"/></g>
<circle cx="585" cy="598" r="362" fill="none" stroke="url(#rim)" stroke-width="2"/>
<path d="M50 926C362 776 848 648 1198 325" fill="none" stroke="url(#orbit)" stroke-width="1"/>
<g fill="#aad7dc" opacity=".32"><circle cx="1150" cy="368" r="2.3"/><circle cx="294" cy="129" r="1.1"/><circle cx="1440" cy="822" r="1"/><circle cx="1680" cy="187" r=".8"/></g>
</svg>
```

## Light

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 1920 1080">
<defs>
 <linearGradient id="field" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#dfe9e8"/><stop offset="1" stop-color="#bcced3"/></linearGradient>
 <radialGradient id="halo"><stop offset=".55" stop-color="#87b8ba" stop-opacity=".35"/><stop offset="1" stop-color="#87b8ba" stop-opacity="0"/></radialGradient>
 <radialGradient id="world" cx=".22" cy=".18" r=".94"><stop stop-color="#dcebea"/><stop offset=".48" stop-color="#a8bec7"/><stop offset="1" stop-color="#647f91"/></radialGradient>
 <linearGradient id="rim" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#ffffff" stop-opacity=".90"/><stop offset=".25" stop-color="#ffffff" stop-opacity=".30"/><stop offset=".55" stop-color="#ffffff" stop-opacity="0"/><stop offset="1" stop-color="#ffffff" stop-opacity=".06"/></linearGradient>
 <linearGradient id="orbit"><stop stop-color="#ffffff" stop-opacity="0"/><stop offset=".48" stop-color="#ffffff" stop-opacity=".20"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/></linearGradient>
 <clipPath id="disk"><circle cx="585" cy="598" r="362"/></clipPath>
</defs>
<path fill="url(#field)" d="M0 0h1920v1080H0z"/>
<ellipse cx="500" cy="500" rx="680" ry="630" fill="url(#halo)"/>
<g fill="none" stroke="#5e7f8d" opacity=".045" stroke-width=".6"><path d="M0 180h1920M0 360h1920M0 540h1920M0 720h1920M0 900h1920M240 0v1080M480 0v1080M720 0v1080M960 0v1080M1200 0v1080M1440 0v1080M1680 0v1080"/></g>
<ellipse cx="650" cy="608" rx="660" ry="198" transform="rotate(-24 650 608)" fill="none" stroke="url(#orbit)" stroke-width="1"/>
<circle cx="585" cy="598" r="362" fill="url(#world)"/>
<g clip-path="url(#disk)" fill="none" stroke="#ffffff" stroke-width=".7" opacity=".07"><ellipse cx="585" cy="598" rx="362" ry="120" transform="rotate(-24 585 598)"/><ellipse cx="585" cy="598" rx="362" ry="240" transform="rotate(-24 585 598)"/><ellipse cx="585" cy="598" rx="120" ry="362" transform="rotate(-24 585 598)"/></g>
<circle cx="585" cy="598" r="362" fill="none" stroke="url(#rim)" stroke-width="2"/>
<path d="M50 926C362 776 848 648 1198 325" fill="none" stroke="url(#orbit)" stroke-width="1"/>
<g fill="#ffffff" opacity=".32"><circle cx="1150" cy="368" r="2.3"/><circle cx="294" cy="129" r="1.1"/><circle cx="1440" cy="822" r="1"/><circle cx="1680" cy="187" r=".8"/></g>
</svg>
```
