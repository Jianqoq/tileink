# M4 turbulence

The maintained HLSL entry shares immutable seeded lattice tables with the existing
renderer. Typed Rust uploads validate selectors, finite unit gradients, table
indices, buffer ownership and coordinate bounds. HLSLI owns the lattice dimensions
and octave limit; Rust and WGSL consume generated constants. Shader helpers take
all resources and parameters explicitly.

## Root causes

Stitching previously derived its wrap boundary from each sample position. It now
uses the fixed tile origin in noise space, including translated and negative
scales. Lattice origins use floor, and full Euclidean wrapping covers negative,
multi-period and single-cell periods without signed subtraction overflow. The
opposite edges therefore sample the same periodic lattice. Both maintained HLSL
and production WGSL use the corrected algorithm.

Zero octaves and zero frequency return their defined constant colors before
coordinate arithmetic. The final octave performs no unused update; accumulation
also stops when the f32 weight underflows. This prevents unused overflow and
unbounded work for constant outputs while preserving effective contributions.

## Verification

Independent unit-gradient colors and opposite-edge periodicity are checked in
addition to seeded production-reference cases. Coverage includes both noise modes,
stitching, sRGB conversion, zero/partial-zero frequency, signed scales, translated
negative lattice coordinates, compact tiles, clipped regions and constant cases
with extreme otherwise-unused parameters. All four API routes and all production
shader variants must match byte-for-byte.

Release (982), runtime (120), strict Clippy, editor, all 68 SPIR-V modules and
shader integration tests pass. No-DXC replay passes all 35 filter GPU tests
without compilation. Full SVG/examples pass; only stitchTiles=stitch.wgpu.png
changes among 3,471 outputs. Human review accepted this PNG on 2026-09-14. This kernel alone does not complete M4;
remaining filters, fine and NativeRenderer/Canvas integration are still required.
