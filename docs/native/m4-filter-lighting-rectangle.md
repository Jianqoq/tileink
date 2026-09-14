# M4 lighting and rectangle composition

Maintained HLSL implements diffuse/specular lighting with distant, point and spot
lights, rectangle masks, masked/unmasked direct composition, rounded-rectangle
composition, and rectangle-clipped upsampling. Shared SDF and resampling helpers
receive their geometry and resources explicitly. Rust encoders validate finite
geometry, signed-coordinate limits, light direction magnitude, sampling bounds,
resource ownership and unique region writes before dispatch.

The Direct pass accepts an optional mask and derives the shader flag from it.
Without a mask its unused static read binding aliases the source; callers do not
need placeholder textures or a second independent enable flag. Upsampling takes
integer source bounds separately from the rectangle used for coverage.

## Lighting root fix

An independent orthogonal-spot regression exposed the shared `pow(0, 0)` corner:
all four routes produced black where zero exponent requires unit intensity.
Both WGSL and HLSL now return one explicitly for nonpositive exponents, retaining
the existing negative-to-zero normalization. Signed focus is tested before power:
back-facing spot lights remain dark, while orthogonal lights follow the exponent
and optional cone limit. This preserves the [SVG spot-light direction rule](https://drafts.csswg.org/filter-effects/#feDiffuseLightingElement).
A zero-length specular half vector remains dark. This fixes the numerical root
cause instead of patching individual fixtures or choosing one backend as truth.

## Verification scope

Tests cover an independent flat-light oracle, orthogonal/back-facing spot lights,
zero/negative exponents, opposite half vectors, degenerate directions, both output
modes, compact/dense regions, and the ordinary light corpus. Rectangle tests use
independent f64 signed distance and integer premultiplied composition, repeated
writes, reversed bounds, distinct corner radii, optional masks and clipped tiles.
Upsampling additionally uses a varying two-dimensional field with source bounds
independent of SDF bounds, nearest/linear sampling and clamped edges; constant and
empty-domain cases remain covered. Every GPU test runs all native/portable and
texture-table variants across wgpu/native DX12/Vulkan with exact output bytes.


Full release passes 980 ordinary library tests, native runtime passes 111 tests,
and 28 filter GPU tests run with both DXC executables unavailable and no runtime
compilation. Strict native all-targets release Clippy, Shader Tools roundtrip,
all 66 SPIR-V modules, and shader compiler/inventory/artifact tests pass.
Full SVG/examples preserve all 3,471 PNGs byte-for-byte. Both reviews are closed.
Inventory advances to 131/179; remaining filters, full fine and NativeRenderer/
Canvas integration keep M4 open. Evidence hashes are in the adjacent verification JSON.
