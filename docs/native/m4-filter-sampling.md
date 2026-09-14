# M4 morphology, displacement and component transfer

Three maintained HLSL entries use independent stage modules and the common
region recorder. They add 12 production variants after full acceptance.

## Morphology

The algorithm takes straight-RGB and alpha extrema separately, then premultiplies.
It samples the full source row/column while the region and compact tile list
restrict writes. Erosion crossing a surface edge returns transparent; dilation
clips its scan interval. Host checks axis/operator and pos+radius overflow rather
than silently changing the radius. An independent rational CPU oracle compares
channel fractions by integer cross-products and rounds the final premultiplication.
Both axes/operators, zero/large radius, transparency, clip and sparse tails pass
all four API routes and production variants.

## Displacement

The stage explicitly binds source and map textures. Channel and finite-scale
checks precede recording; both languages test floating source coordinates before
integer conversion, so overflow from finite scales produces transparent output.
The corpus covers all channel pairs, sRGB/linear conversion, signed/zero/maximum
scales and dense/sparse regions, including arbitrary raw map bytes.

An actual mismatch at x7/y1 with map[56,184,8,136], blue displacement scale17,
was reduced to a permanent regression. Native evaluated x=0 while wgpu evaluated
x=-1. Explicit FMA fixes the coordinate multiplication/addition order before
rounding, matching an independent float32 CPU FMA result. Separate independent
half-even/outside tests cover positive/negative scales, including f32::MAX. This
fixes the general contraction difference; no tolerance or pixel exception is used.

## Component transfer and raw-buffer contract

TransferTables owns a private buffer/count pair. Upload rejects empty tables,
values outside0..255 and overflowing raw byte addresses. Recording checks logical
table index and batch ownership. The common ReadBindings keeps texture extent
validation separate from read-buffer type/owner checks; table contents and logical
address bounds remain the stage's responsibility. No unchecked GPU-derived count
is used. Portable WGSL snapshots the live target before each transfer pass.

Table size/channel count/length are canonical in HLSLI. The generated Rust constants
and WGSL assembly use those definitions; public usize constants retain only typed
aliases. No duplicated WGSL numeric declarations or ABI JSON are introduced.
Independent integer tests cover three tables, nonzero indices, alpha zero, straight
index clamping, partial tiles, clipped regions and consecutive transformations.

## Verification

Full release: 975 library tests; full native runtime: 91 tests. Strict native
all-targets release Clippy, Shader Tools roundtrip, all 56 SPIR-V modules and shader
compiler/inventory/artifact tests pass. With both DXC executable paths unavailable,
13 filter GPU tests pass without runtime compilation. Full SVG and examples keep
all 3,471 baseline PNGs byte-identical. Both independent reviews are closed.
Inventory advances to 91/179; the remaining 88 entries, full fine and
NativeRenderer/Canvas integration still block M4 completion. See the adjacent
m4-filter-sampling-verification.json for source and log hashes.
