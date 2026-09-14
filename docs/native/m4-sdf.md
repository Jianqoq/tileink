# M4 analytic SDF coverage

Maintained HLSL modules in `shared/sdf` implement all 19 encoded shape kinds,
including strokes, shadows, dashed lines, arc caps, callout tails and checkerboard
coverage. Geometry, affine distance conversion, primitive evaluation and encoded
record loading have explicit interfaces. The paint buffer is passed to every raw
record reader; absent words return zero without unsigned address overflow.

The 17-word shape layout is canonical in HLSLI and feeds the existing Rust encoder.
Probe-only request layout lives in `validation/sdf_config.hlsli`; its generated Rust
constants are included only in Windows native tests. No ABI JSON or runtime shader
translation is involved. The existing rectangle mask uses the same primitive math.

Four-API tests exposed two root causes at half-alpha boundaries: independently
rounded affine/Jacobian terms and a rounded product in floating remainder. Both
WGSL and HLSL now explicitly fuse the same operations. This fixes arithmetic order
instead of tolerating differing output bytes or special-casing a fixture.

The GPU corpus includes all shape kinds, identity/nonuniform/reflected/rotated/sheared
transforms, integer and half-pixel samples, three line and arc caps, negative/zero/full
arc sweeps, zero-length lines, zero radii, reversed bounds, every callout direction,
and hidden tails. Independent double-precision rectangle/circle coverage oracles
anchor the cross-API comparison. Over-dispatch and output sentinels check tail guards.
The production native/portable and texture-table variants must all match exactly.

This is shared coverage infrastructure, not another production inventory entry.
Inventory remains 143/179; remaining filter kernels, fine and NativeRenderer/Canvas
integration are still required for M4. Final release validation passes: 983 library tests, 123 native runtime tests,
strict Clippy, shader integration, editor roundtrip and all 70 current SPIR-V modules.
Full SVG/examples preserve all 3,471 outputs apart from the previously accepted
turbulence stitch fix. Both reviews are closed; evidence hashes are in the adjacent receipt.

All 36 filter GPU tests run with both DXC executables unavailable. New shader identities
create driver pipelines from embedded bytecode on their first use. Separate basic-filter
and SDF processes subsequently hit disk pipeline caches without compiling pipelines.
Driver pipeline creation and HLSL source compilation are distinct cache layers.
