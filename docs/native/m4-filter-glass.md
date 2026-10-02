# M4 liquid glass

Both maintained HLSL entries implement sharp/blurred refraction and rectangle
composition. Geometry, sampling, color conversion and final shading are separate
modules with explicit resource/configuration arguments and direct includes.
HLSLI owns float constants; the build emits WGSL declarations from the unchanged
literal text and rejects ambiguous expressions, duplicate names and nonfinite values.

The host enforces normalized tint/percentage controls documented by the public API,
checks the actual angle and rectangle arithmetic domain, and validates logical blur
bounds. Large finite refraction factors remain supported.

Two numerical root fixes apply to WGSL and HLSL:

- A zero normal component produces zero displacement before multiplication. This
  prevents reassociation of overflowing refraction distances into `0 * infinity`.
  The extreme transparent-input four-API test failed before the fix and passes after.
- The monotonic fifth-power highlight profile clamps its base before `pow`, defining
  negative-base behavior while preserving valid positive-base evaluation. Fresnel
  and glare share that implementation.

Tests cover tint, Fresnel, glare, dispersion, both kernels, dense/compact dispatch,
transparent pixels, maximum refraction, degenerate rectangles, asymmetric radii,
empty blur domains and preserved target padding. A separate integer oracle checks
nonconstant two-dimensional downsampled blur, nonzero source origin and nearest/
linear interpolation. Negative highlight intensity has an independent black-pixel
oracle. Invalid angle/tint/geometry acceptance was observed failing before its fix.

Validation passes: 989 ordinary release tests, 137 native runtime tests, strict
Clippy, Shader Tools, shader integration and 76 SPIR-V modules. Four GPU tests replay
without DXC and hit disk pipeline caches. Full SVG/examples have no new differences
across 3,471 PNGs beyond accepted turbulence. Both review axes are closed.

This slice adds eight validated variants, reaching 167/179. Fine, brush-dependent
effects and NativeRenderer/Canvas integration remain M4 work.
