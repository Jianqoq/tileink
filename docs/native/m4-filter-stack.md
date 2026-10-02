# M4 filter stack composition

The maintained HLSL stack kernels implement ordinary, blend and translated-surface
composition using explicit scene buffers. Path and transformed analytic SDF coverage
share the layer geometry helpers. Nested opacity/blend groups preserve the production
WGSL clip inheritance, integer alpha rounding and overflow behavior.

`Stack` owns validated logical layer references; `Geometry` owns immutable scene
bindings. Upload rejects opacity outside 0–255 before integer pixel arithmetic and
canonicalizes invalid draw references. Encoding validates logical subranges and
signed surface coordinates. Over derives mask presence from the optional auxiliary
texture, binding the read-only source to the unused descriptor when absent. Blend
requires its auxiliary texture; Surface uses signed translation and logical extent.
These are host-boundary fixes, not shader fallbacks or temporary workarounds.

The group capacity is defined once in `shared/stack_constants.hlsli`. Build-time
WGSL and Rust test declarations derive from that definition; no ABI JSON is used.

Tests cover independent integer oracles, nested opacity/blends, capacity overflow,
clips following overflow, poisoned records outside a nonzero subrange, stale mask
flags, absent masks, transformed SDF clips, all blend modes, translated surfaces,
compact tile lists, target padding and repeated writes. Both host regressions were
observed failing before their fixes.

Validation passes: 987 ordinary release tests, 131 native runtime tests, strict
Clippy, Shader Tools, shader artifact/inventory integration and 74 SPIR-V modules.
Both GPU tests replay without DXC and hit disk pipeline caches. Full SVG/examples
have no new differences across 3,471 PNGs beyond the accepted turbulence image.
Standards and Spec reviews are closed.

The three entries add twelve validated variants, reaching 159/179. M4 remains open
for remaining effects, fine rendering and NativeRenderer/Canvas integration.
