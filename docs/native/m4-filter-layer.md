# M4 layer masks

The maintained layer-mask entry evaluates analytic SDF/shadow or path coverage from
explicit draw, path, backdrop, segment-range, segment and paint resources. It uses
the same shared SDF and coverage algorithms as the other native stages. Generic draw
and affine decoding now live under shared, while coarse-specific tags remain local.
All consumers declare their includes directly.

Rust uploads a logical Scene into immutable Geometry handles. It validates raw address
ranges, finite parameters, shadow offsets and the path data_len before recording work.
A regression first demonstrated that allocation capacity alone allowed a zero-length
path to read geometry. The fix checks the logical path span and its containing buffer.
Empty storage bindings contain only type-sized padding; draw_count preserves the empty
logical domain. Foreign batch resources are rejected before dispatch.

GPU tests compare all four APIs and all four production variants. Independent integer
oracles cover nonzero/even-odd backdrops, rectangle masks and a fractional vertical edge.
Additional cases cover nonzero shadow base/offset, primary SDF precedence, reflection,
rotation, shear, invalid draws, compact tiles, nonzero regions and target padding.
An empty scene overwrites live pixels with zero coverage, without reading binding padding.

Release validation passes: 984 library and 126 native runtime tests, strict Clippy,
editor roundtrip, shader integration and 71 current SPIR-V modules. Both layer GPU
tests pass in a separate process with DXC unavailable and disk pipeline cache hits.
All SVG/examples pass; 3,471 PNGs have no new differences beyond the previously
accepted turbulence stitch fix. Both reviews are closed.

This entry adds four validated production variants, reaching 147/179. Remaining
effects, fine and NativeRenderer/Canvas integration still keep M4 open.
