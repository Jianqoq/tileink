# Native rectangle full-tile proofs

Coarse can replace a fully covered rectangle fill with a color particle, or omit
an all-one rectangle clip mask. Both Metal and the DX12/Vulkan HLSL path use
`sdf_clip_covers` for this proof.

## Required invariants

- The proof accepts a complete rectangle SDF without shadow data, identity linear
  transform, and rectangle coordinates, corner radii and translation within
  ±2^16. The bound comparisons also reject NaN and infinity. Other inputs retain
  fine evaluation. Conservative draw bounds are not a coverage proof.
- Keep the complete 16×16 tile pixel centers inside the existing half-pixel inset
  and rounded-corner coverage tests. Partial viewport tiles use the same complete
  tile proof. Size/bounds rejection happens before the numeric guard.
- This guard fixes the proof's numerical assumption, rather than changing fine
  coverage. Finite coordinates near 1e7 can round away the half-pixel inset;
  values near 1.7e38 can overflow fine's rectangle center calculation. Previously
  coarse could substitute full color despite fine having partial or zero
  coverage. Unsafe geometry now follows the fine computation used as the oracle.
- Fill/clip routing, count/emission agreement, layer stacks, loaded targets,
  retained damage, bindings and shader ABI keep their existing semantics.

## Regression and validation

`native_rect_tile_proofs_match_fine_only_pixels` compares complete output bytes
with an independent stream that emits every rectangle SDF into every tile,
bypassing coarse classification. It includes fractional geometry, asymmetric
rounded corners/strokes, translucent brushes, small/collapsed shapes, partial
viewport tiles, translation, scale, rotation, positive/negative numeric-domain
boundaries, and large translated fills that previously failed. The renderer is
reused across cases to exercise persistent resources.

The two large-fill cases fail before the numeric guard, at the first differing
byte 8256. The test is shared by native backend test modules. Run physical GPU
tests serially in release mode with validation and an explicitly selected GPU.
Metal is hardware validated here; matching HLSL source does not establish DX12
or Vulkan hardware results. Full SVG/examples and real application performance
validation are required for changes to this proof.

The related experiment that removed zero-coverage rectangle-stroke particles was
rejected because real application frame tails regressed, despite reducing the
identified batch's GPU time. It is not part of the implementation.
