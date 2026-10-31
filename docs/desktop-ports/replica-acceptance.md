# Acceptance for source-faithful desktop ports

A colour scheme and a similar existing renderer are an adaptation, not a replica. Deepin and openKylin refinement targets the original composition and interactions, including the components that the earlier adapters could not express.

## Reference and identity

Every component records its upstream repository, commit, source files, target release and design mode. Keep efficient/fashion dock variants and classic/island panel variants distinct. An original reference image must come from the upstream application or an identified official image, with its revision or release and file hash. A Decebal rendering generated from source measurements is useful evidence, but is not an original screenshot.

Record logical/physical dimensions, display/text scales, font, light/dark mode and component state. Compare matching crops of the panel, launcher, quick settings, decoration and widget separately. Different wallpaper, network status or application lists must not conceal a composition mismatch. Source-only measurements and original pixel comparisons are separate evidence types.

## Implementation contract

- Provide native component layouts where the original needs them. Selecting Windows, Fluent, Card or another desktop's launcher as the nearest existing look does not complete a Deepin/UKUI component.
- Match silhouette, padding, spacing, icon and text sizes, hierarchy, indicator placement, radii, material and state transitions. Record unexplained differences rather than averaging them into a favourable similarity score.
- Use genuine matching logos and asset packs where available. Verify source/licence identity and preserve attribution. A neutral fallback is a valid failure mode; it is not a matching-logo fidelity pass.
- Preserve user-selected icons, cursors, wallpapers and widget layouts through the established ownership/choice contracts. Test the complete matching asset set in a private profile without silently replacing the user's live choices.
- Use selectable native style/profile APIs rather than branches on a theme package ID. Other packages must be able to use the new components, and palette changes must continue to flow through resolved theme colours.
- All rendered controls use the same geometry for drawing, hit targets and keyboard/accessibility actions. Cosmetic copies with inert controls are not accepted.
- Keep system probes, font loading and asset decoding out of motion/paint paths. The visual refinement must retain the desktop's bounded worker/cache model.

## Evidence required for each accepted section

1. Before, original reference, and after captures with identities and matching conditions.
2. Source-derived measurements plus explicit remaining visual differences.
3. Functional search/launch or system actions, keyboard navigation, focus visibility and dismiss/reopen behaviour, using isolated action fixtures where appropriate.
4. Enabled/disabled applet and overflow behaviour with no missing target or overlap. Test ordinary and constrained output sizes.
5. Light/dark and 100/125/200% display/text-scale coverage. Qualify the theme's four-edge extension separately when upstream only supports one edge.
6. Exact tested executable/source identities, test output, owned-display/worker cleanup and preserved failure evidence.

First complete a visible section end to end, then proceed through the remaining sections. A section can be accepted while the full port remains in progress. Do not remove the full-port gap list or call the whole package a replica on the strength of one successful crop.

The original reference repositories, screenshots and assets retain their own licences. Any bundled material needs its actual attribution and licence; native Rust implementation and measurements do not make upstream artwork MIT-licensed.
