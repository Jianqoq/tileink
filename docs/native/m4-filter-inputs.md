# M4 input-combination filters

Three maintained HLSL entries implement blend, SVG composite inputs and region
mask. Kernel-specific resources are explicit: Blend/Composite carry source and
backdrop; Mask carries only its mask. The shared region recorder validates
resource ownership, 2D extent, non-aliasing with target, logical region, unique
active tiles and padded dispatch bounds. Two read-only inputs may alias. Basic
filters reuse the same recorder; kernel-specific parameter validation remains
with its stage. There is no mod.rs and no implicit shader resource dependency.

Mask reads and writes its invocation's own target pixel. Portable WGSL reference
snapshots the live target before each pass; repeated masks verify that a second
pass observes prior GPU output. HLSLI owns SVG operator constants and maps them
to the shared Porter-Duff implementation. Arithmetic coefficients must be finite.

## Numerical root cause

The expanded 224 blend/compose combinations exposed Hue + SourceOut: source
[6,60,78,138], backdrop [102,136,68,170], blue35 native vs36 wgpu. A permanent
single-pixel regression preceded the correction. The effective source color now
uses the same explicit FMA order in WGSL/HLSL, preserving the existing wgpu result
without a tolerance, backend branch or fixture exception. The minimal regression
passes all four routes, as does the complete numeric corpus.

## Acceptance

All 224 mix/compose combinations, six SVG operators with three coefficient sets,
dense/sparse clipped regions and repeated masks pass all four variants/routes.
Independent Porter-Duff integer and arithmetic f64 expectations pass. Full release
passes (972 library tests); 83 runtime tests, strict release Clippy, real editor
round trips, 53 SPIR-V modules and shader integration checks pass. Eight filter
GPU tests pass with both DXC executable paths missing, with no compilation.
Full SVG/examples produce all 3,471 PNGs unchanged. Both reviews are closed.

This adds 12 inventory variants, bringing verified entries to 79/179. Remaining
filters, full fine and NativeRenderer/Canvas integration are still required for M4.
