# M4 translated surface composition

The maintained HLSL surface entry composites a separately sized logical source at
its signed target origin. Source-over uses the shared integer pixel implementation.
Rust validates signed translation, source/target ownership, distinct textures,
logical dimensions and unique write ownership before recording.

The common filter resource contract now carries the source logical extent explicitly.
Allocations may contain padding, but source coordinates and output regions remain
logical. This is the required domain model for differently sized and pooled surfaces;
it does not infer live pixels from allocated capacity.

An independent integer oracle checks all four APIs and all four production variants.
It covers a 9x7 source allocation with a 7x5 logical image, a 35x21 target allocation
with a 33x19 logical image, colored padding sentinels, zero source dimensions,
positive/negative and fully clipped translations, compact tiles, nonzero regions,
and repeated source-over. Invalid dimensions, signed overflow, aliases and foreign
batch resources are rejected before any pass is recorded.

Release (983), native runtime (122), strict Clippy, editor, all 69 SPIR-V modules,
shader integration, SVG and examples pass. All 36 filter GPU tests pass with DXC
executables unavailable and no runtime compilation. Surface introduces no PNG
changes; the sole baseline difference is the separately reviewed and user-accepted
turbulence stitch fix. Both reviews are closed.

Validated production inventory is 143/179. Remaining filters, full fine and
NativeRenderer/Canvas integration keep M4 incomplete.
