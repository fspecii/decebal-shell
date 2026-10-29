# Original wallpaper artwork

These SVG compositions are original Decebal artwork, released under MIT with the theme. They contain no app UI, widget labels, logos, third-party illustrations or embedded fonts. The packaged PNGs are rendered at 3840 × 2160 with `rsvg-convert`. SVG is kept in this document because `.svg` files are intentionally not accepted inside theme packages.

## Light

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 1920 1080">
<defs>
 <pattern id="grain" width="17" height="19" patternUnits="userSpaceOnUse"><circle cx="3" cy="4" r=".4" fill="#756b5a" opacity=".09"/><path d="M12 11h1m-4 6h.6" stroke="#756b5a" stroke-width=".4" opacity=".07"/></pattern>
 <pattern id="ledger" width="48" height="48" patternUnits="userSpaceOnUse"><path d="M48 0H0V48" fill="none" stroke="#756b5a" stroke-width=".5" opacity=".10"/></pattern>
 <clipPath id="sheet"><rect x="76" y="70" width="1768" height="940"/></clipPath>
</defs>
<rect width="1920" height="1080" fill="#eee7da"/>
<rect width="1920" height="1080" fill="url(#grain)"/>
<rect x="76" y="70" width="1768" height="940" fill="#f7f1e6"/>
<g clip-path="url(#sheet)">
 <rect x="76" y="70" width="1768" height="940" fill="url(#ledger)"/>
 <circle cx="528" cy="501" r="306" fill="#b65336"/>
 <path d="M76 825L855 250L1032 528L388 1010H76Z" fill="#f7f1e6"/>
 <path d="M76 850L865 268" stroke="#b65336" stroke-width=".7" opacity=".7"/>
 <path d="M76 881L886 284" stroke="#b65336" stroke-width=".7" opacity=".45"/>
 <path d="M76 913L905 300" stroke="#b65336" stroke-width=".7" opacity=".25"/>
 <path d="M76 1010L1032 528L1125 675L469 1010Z" fill="#616c55" opacity=".70"/>
 <path d="M76 1010L1125 675L1160 730L730 1010Z" fill="#323d39" opacity=".80"/>
 <g fill="none" stroke="#756b5a" stroke-width=".8" opacity=".25">
  <path d="M1250 1010c-30-70 4-122 78-153s122-64 110-140-109-137-90-234"/>
  <path d="M1300 1010c-30-75 9-108 75-147s121-77 98-159-98-137-77-221"/>
  <path d="M1350 1010c-25-70 15-106 78-144s114-89 79-170-82-143-62-213"/>
  <path d="M1400 1010c-18-65 22-99 81-143s103-103 56-179-64-149-43-205"/>
  <path d="M1450 1010c-11-55 35-98 84-143s91-119 32-187-46-151-24-197"/>
  <path d="M1500 1010c-3-54 43-95 87-144s73-137 6-194-26-151-4-189"/>
 </g>
</g>
<g stroke="#756b5a" opacity=".5" stroke-width=".6" fill="none">
 <path d="M51 70h50M76 45v50M1819 70h50M1844 45v50M51 1010h50M76 985v50M1819 1010h50M1844 985v50"/>
</g>
<g stroke="#756b5a" opacity=".2" stroke-width=".6"><path d="M76 1010H1844M76 70H1844"/></g>
<rect width="1920" height="1080" fill="url(#grain)"/>
</svg>
```

## Dark

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 1920 1080">
<defs>
 <pattern id="grain" width="17" height="19" patternUnits="userSpaceOnUse"><circle cx="3" cy="4" r=".4" fill="#8c8d83" opacity=".09"/><path d="M12 11h1m-4 6h.6" stroke="#8c8d83" stroke-width=".4" opacity=".07"/></pattern>
 <pattern id="ledger" width="48" height="48" patternUnits="userSpaceOnUse"><path d="M48 0H0V48" fill="none" stroke="#8c8d83" stroke-width=".5" opacity=".10"/></pattern>
 <clipPath id="sheet"><rect x="76" y="70" width="1768" height="940"/></clipPath>
</defs>
<rect width="1920" height="1080" fill="#171b1c"/>
<rect width="1920" height="1080" fill="url(#grain)"/>
<rect x="76" y="70" width="1768" height="940" fill="#212728"/>
<g clip-path="url(#sheet)">
 <rect x="76" y="70" width="1768" height="940" fill="url(#ledger)"/>
 <circle cx="528" cy="501" r="306" fill="#d68761"/>
 <path d="M76 825L855 250L1032 528L388 1010H76Z" fill="#212728"/>
 <path d="M76 850L865 268" stroke="#d68761" stroke-width=".7" opacity=".7"/>
 <path d="M76 881L886 284" stroke="#d68761" stroke-width=".7" opacity=".45"/>
 <path d="M76 913L905 300" stroke="#d68761" stroke-width=".7" opacity=".25"/>
 <path d="M76 1010L1032 528L1125 675L469 1010Z" fill="#8f9b84" opacity=".70"/>
 <path d="M76 1010L1125 675L1160 730L730 1010Z" fill="#67746e" opacity=".80"/>
 <g fill="none" stroke="#8c8d83" stroke-width=".8" opacity=".25">
  <path d="M1250 1010c-30-70 4-122 78-153s122-64 110-140-109-137-90-234"/>
  <path d="M1300 1010c-30-75 9-108 75-147s121-77 98-159-98-137-77-221"/>
  <path d="M1350 1010c-25-70 15-106 78-144s114-89 79-170-82-143-62-213"/>
  <path d="M1400 1010c-18-65 22-99 81-143s103-103 56-179-64-149-43-205"/>
  <path d="M1450 1010c-11-55 35-98 84-143s91-119 32-187-46-151-24-197"/>
  <path d="M1500 1010c-3-54 43-95 87-144s73-137 6-194-26-151-4-189"/>
 </g>
</g>
<g stroke="#8c8d83" opacity=".5" stroke-width=".6" fill="none">
 <path d="M51 70h50M76 45v50M1819 70h50M1844 45v50M51 1010h50M76 985v50M1819 1010h50M1844 985v50"/>
</g>
<g stroke="#8c8d83" opacity=".2" stroke-width=".6"><path d="M76 1010H1844M76 70H1844"/></g>
<rect width="1920" height="1080" fill="url(#grain)"/>
</svg>
```

