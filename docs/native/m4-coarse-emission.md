# M4 Windows coarse particle emission

Three maintained HLSL entries complete the coarse inventory: `coarse_emit`,
`coarse_emit_bins`, and `coarse_emit_chunks`. The last explicitly corresponds to
`coarse/emit_web.wgsl::coarse_emit`; its native name distinguishes chunk dispatch
from tile dispatch. The reference runner explicitly selects that production module
and entry. There is no implicit variant fallback or runtime compiler fallback.

The migration inventory now has 27/179 kernel-validated programs. Fine/effects,
shared production resources and NativeRenderer/Canvas remain unfinished. This is
not full M4 acceptance or complete four-route SVG rendering.

## Implementation

- `emit_draw.hlsli` creates particle instructions, `paint.hlsli` owns solid/image/SDF
  eligibility, and `emit_stack.hlsli` emits forward begins and reverse ends while
  skipping proved no-op clips. All reusable resources and configuration are explicit.
- `text.hlsli` shares glyph hit testing between counting and storing source indices.
  Raw draw layout checks now cover 16 host facts, including brush/solid fields.
- Tile and chunk kernels share ordered paired prefix allocation. Chunk count,
  offsets and emission execute in one GPU batch without intermediate CPU readback.
- `tile_classification.hlsli` owns classification precedence for both the bin emitter
  and the chunk-kind reducer. Chunk membership flags use a workgroup OR reduction,
  equivalent to the reference's three membership sums.
- Physical particle/glyph capacity, logical glyph range and padded groups are tested
  separately. A red test demonstrated spare-capacity writes in the original WGSL
  chunk emitter. Both languages now check the contiguous live reference prefix
  before reading chunk records or entering barriers, while retaining the emitter's
  physical-capacity check. This fixes the pooled-buffer boundary error directly.

## Focused verification

All four routes use the same RTX 4090 LUID `9f3f010000000000` and compare complete
output buffers to independent CPU expectations, including untouched regions.

- Flat/linked lists at 0/1/255/256/257/513 draws, ordered records, elided/retained/
  invalid stacks and zero/truncated/sufficient physical particle capacity.
- Solid path/SDF/image eligibility, rotation/shear exclusions, shadow/unsupported
  kinds, nonzero brush base plus offset and negative winding.
- Glyph source-index storage, nonzero destination, independent logical/global
  capacity limits and carry across the 256-draw page boundary.
- Nested retained layers separated by an elided clip, exact payloads and reverse ends.
- 17x19 dense/sparse grids and edge bins. The chunk chain checks nonzero reference
  offsets, an empty final tile and stale spare records with padded 2D dispatch.

Full release and all 60 runtime tests pass. Strict all-target release Clippy,
formatting, independent header compilation, real editor diagnostic round trips,
all 31 embedded SPIR-V validations, and both code reviews pass. The shared
classification refactor additionally passes all 17 coarse GPU tests. A 60-test
runtime rerun with nonexistent DXC executable paths passes with zero pipeline
compilations. Full existing wgpu SVG/example comparisons preserve all 3471 PNG
hashes; these do not substitute for native full-frame acceptance at M4 exit.
See the [verification receipt](m4-coarse-emission-verification.json). Performance comparisons remain waived; real
Mac MSL compilation and GPU validation remain deferred.
