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

## Zero-coverage rectangle-stroke particles

The follow-up application investigation re-enables rectangle-stroke interior
rejection in Metal and the shared DX12/Vulkan coarse path. It removes redundant
fine evaluation at the particle source; it does not change stroke coverage or
presentation policy. The first experiment's worse application tails motivate
separate present/queue attribution, rather than invalidating its measured GPU gain.

- Only brush rectangle strokes can be rejected. Glyph routing keeps priority;
  layer descriptors, fills and other shapes keep their original routing.
- Require a complete 13-word stroke record, no shadow alias, identity linear
  transform, and raw rectangle/radius/half-width fields within ±2^16. NaN,
  infinity, incomplete geometry, scale/shear/rotation and uncertain coordinates
  retain the original particle. The shared proof also checks the inner rectangle
  and translation's numeric domain.
- Fine's serialized half widths are top/right/bottom/left, clamped to zero. Inset
  the normalized outer rectangle by those widths; subtract the maximum adjacent
  widths from each inner corner radius, then clamp as in fine. A collapsed inner
  rectangle cannot justify rejection.
- Stroke coverage is `clamp(outer - inner, 0, 1)`. All pixel centers of the complete
  16×16 tile must be in the inner rectangle's full-alpha region. The shared
  corner/rectangle proof uses a one-pixel inset for stroke interiors, including
  a half-pixel rounding reserve; fills/clips retain the half-pixel inset. Partial
  viewport tiles still use the complete-tile proof.
- All three count routes and draw emission use the same immutable paint/draw
  predicate. Prefix allocation, emitted stream bounds and classification must
  agree. Clip/opacity/blend ordering and the loaded/clear target base remain owned
  by the existing empty-stream handling.

`native_routes_coarse_rect_stroke_interiors_have_no_particles` covers ordinary,
asymmetric, clamped-negative and translated widths, rounded interiors, reversed
bounds, AA/thin/collapsed boundaries, fills, unsupported transforms, shadow aliases,
truncated records, nonfinite fields and large coordinates across all three count
routes. It fails on source without culling. The independent pixel oracle above
checks removal against fine's original calculation, rather than a duplicated CPU
formula. Real application frame tails and GPU work are measured separately.

`native_routes_coarse_stroke_proof_preserves_glyph_priority` checks glyph counts
and both emission routes with and without an aliased empty-stroke record. It
reproduces a restoration error where Metal applied the stroke predicate before
glyph emission, despite count routing selecting the glyph. Restoring glyph
priority fixes that count/emission disagreement at its source.
