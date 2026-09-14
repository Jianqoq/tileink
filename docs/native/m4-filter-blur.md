# M4 global and shared-memory blur

Two maintained HLSL entries implement Gaussian axis blur. Global sampling pairs
taps with logical-texel interpolation and explicit FMA recurrence boundaries.
Shared sampling cooperatively loads a tile plus halo, then executes one uniform
barrier before consuming it; radii above the shared limit use the global algorithm.
All resources are explicit arguments or entry-owned bindings/storage.

## Dispatch and input contract

The common region recorder now accepts explicit Pixels or Tiles geometry.
Existing filters retain Pixels. Dense shared blur uses the region's physical 2D
tile grid; compact shared blur linearizes the supplied unique tile list and guards
padded workgroups before any load/barrier. Logical counts, ownership, dimensions
and all raw address bounds remain validated. Sigma/axis/source bounds and padded
signed texel arithmetic are checked in the blur stage.

## Source-domain root fix

An independent regression placed the only nonzero source pixel outside the declared
sampling domain. All four global routes leaked [102,102,102,102] instead of transparent;
the shared algorithm correctly ignored it. The center is now checked against the
same source bounds as every other tap, in both WGSL and HLSL. This fixes the shared
semantic error instead of inheriting it from a GPU reference. Zero/negative sigma
continues the production copy bypass.

## Verification

Four focused tests pass, including independent f64 Gaussian impulses, the domain
regression, both axes, dense/compact partial tiles, separate source bounds and the
shared-radius limit/fallback. Every native/portable and texture-table production
variant is compared across wgpu/native DX12/Vulkan. Both independent reviews are closed. Full release passes 978 ordinary library
tests; native runtime passes 103 tests. Strict native all-targets release Clippy,
Shader Tools roundtrip, all 61 SPIR-V modules and shader compiler/inventory/artifact
tests pass. With both DXC executables unavailable, 22 filter GPU tests pass without
runtime compilation. Full SVG and examples preserve all 3,471 PNGs byte-for-byte.
Inventory advances to 111/179; 68 remaining entries, full fine and renderer
integration still keep M4 open. Evidence hashes are in m4-filter-blur-verification.json.
