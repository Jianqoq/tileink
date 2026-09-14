# M4 fine shared math

The maintained HLSL pixel, coverage and pattern-transform helpers are the first
fine-interpreter building blocks. Validation adapters call these helpers directly;
wgpu adapters call the production WGSL helpers. No native shader is generated from
WGSL, and unknown reference programs still fail explicitly.

This is not fine-entry or M4 acceptance: the production inventory stays at 27/179.
The fine interpreter, textures, effects and production NativeRenderer remain pending.

## Numerical contract

- Packed premultiplied channel arithmetic preserves the integer division-by-255
  rounding rule. Interpolation stays in channel space and explicitly uses fused
  multiply-add before the single byte quantization.
- Row endpoints are evaluated directly relative to the pixel row. Computing
  `p0y + (p1y - p0y)` loses the residual of `p1y=0.50000006` when `p0y=7.75`,
  producing alpha 128 instead of 127. A real four-route failure and a separate CPU
  failure established this root cause. HLSL, WGSL and CPU debug now use the direct
  endpoint expression; this is a numerical fix, not a backend-specific tolerance.
- Coverage keeps base, running and partial accumulation in their original order.
  Two three-segment permutations explicitly require 127 and 128 respectively.
- Pattern transforms recover the rounded second product before translation. An
  independently checked exact-zero rotation must address cell zero, not minus one.
- Reusable HLSL declares its own includes and accepts every buffer explicitly.
  Only root adapters assign bindings. Workgroup size comes from canonical HLSLI.

## Verification corpus

All four APIs run on the same pinned physical GPU and compare every returned byte,
including guard records after padded dispatches. Pixel math covers 65,584 records
and 14 outputs, with exhaustive byte-factor pairs and independent CPU channel
oracles. Geometry covers 50,178 edge/pattern records. Ordered fill covers 23,554
requests: empty and nonzero ranges, negative/multiple backdrops, both fill rules,
reversed/sloping edges and independently specified rounding-order cases.

Release verification passed: 966 library tests plus integration tests, 63 native
runtime tests, strict all-target Clippy, independent header/DXIL/SPIR-V compilation,
real Shader Tools, all 34 SPIR-V modules and all SVG/examples. The immutable 3,471
PNG baseline is unchanged. The three helper GPU tests passed again after review,
then passed with nonexistent DXC paths and zero runtime pipeline compilations.
Both correctness/spec and standards reviews are closed. See the verification
receipt for source, artifact and local log hashes. These helper-level comparisons
do not claim full native SVG rendering.
