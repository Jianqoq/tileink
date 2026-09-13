# M4 Windows coarse counting and classification

Six additional maintained HLSL entries implement tile/bin counting, per-reference
particle/glyph counting and offsets, tile totals, and tile-kind classification.
The migration inventory is 24/179 kernel-validated entries. Full particle emission,
fine/effects, production resource reuse and NativeRenderer/Canvas remain unfinished;
these kernel tests do not constitute M4 acceptance.

## Implementation and invariants

Scene loaders, transformed glyph bounds, analytic clip proofs, stack validation and
paged draw traversal are separate explicit HLSLI helpers. Every reusable helper
receives its resources and configuration; roots own register declarations. Raw draw,
text and layer layouts have 14 Rust size/offset checks, in addition to the 18 coarse
record layout checks. Constants are defined in HLSLI; no ABI JSON is required.

The per-reference dispatch uses the final tile's offset plus count as its live
reference bound. Prefix allocation produces contiguous ordered tile ranges; physical
capacity is not the live count. A regression first demonstrated that the old WGSL
kernel wrote stale spare-capacity records for padded groups, including a zero-reference
scene. WGSL and HLSL now reject those groups uniformly before record reads and barriers.
This fixes the underlying pooled-buffer boundary error, rather than clearing the
spare capacity or relying on robust out-of-bounds access.

Counts preserve wrapping-u32 arithmetic and only update their specified fields.
Full clips are elided only when the winding or analytic coverage proof succeeds;
retained or invalid stacks take interpreter precedence for tile classification.
GPU intermediates remain on the GPU; no new CPU readback is used by kernels.

## Focused verification

All four routes are pinned to the same physical RTX 4090 (LUID
`9f3f010000000000`): wgpu DX12, wgpu Vulkan, native DX12 and native Vulkan.
Each route must match independent expected complete buffer bytes, including guards.

- Flat/linked pages: 0/1/255/256/257/513 draws; path/SDF/shadow/vector glyph,
  batch filtering, elided/retained/invalid stacks.
- Bitmap glyphs: translation, boundary touch, reflection, empty/invalid images,
  tag filtering and disabled text. Analytic clips: rounded corners, translation,
  scale, shadow, short/non-rect records and empty bounds.
- Per-tile offsets and totals: zero/partial/multiple groups, nonzero packed-region
  bases, count overflow, untouched fields, empty chunks and stack precedence.
- 17x19 grid: both bin axes, partial edge bins, reordered sparse active tiles,
  nonzero reference offsets and an empty final tile carrying the live total.
- Tile kinds: color/SDF/other combinations, ignored unknown bits, empty tiles and
  valid/elided/retained/invalid stacks.
- Every HLSL header compiles independently for DXIL and SPIR-V. Every HLSL/HLSLI
  file completes a real Shader Tools invalid-to-valid diagnostic round trip.

Full release tests pass (965 library tests), all 52 native runtime tests pass,
and strict all-target release Clippy passes. Independent `spirv-val` accepts all
six new SPIR-V entries. A second 52-test runtime execution with nonexistent DXC
executable paths passes with zero pipeline compilations. Both review axes pass.
Full existing wgpu SVG and example native/portable texture comparisons pass;
all 3471 immutable baseline PNG hashes are unchanged. These existing-renderer
checks do not replace full native four-route SVG acceptance at M4 exit.
See the [verification receipt](m4-coarse-count-verification.json). Performance comparison remains
waived by the user; actual Mac MSL verification remains deferred.
