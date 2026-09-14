# M4 target-reading and color filters

Four maintained production entries add source-over, SVG alpha/luminance masks,
color filters and color matrices. Native kernels read/write only the pixel owned
by the invocation. The portable WGSL reference copies the live target on the GPU
before each pass, matching the production snapshot contract. A clear/over/over
chain checks that later passes observe earlier writes, not original upload data.

The encoder's source texture is optional. Only kernels that read source require
and validate it; clear/color/matrix operate with the target alone. Color amounts
and matrix coefficients must be finite. Existing region and unique-tile ownership
checks still apply. HLSLI owns the new selector constants and explicit helpers.

## Root-cause numerical corrections

The full color corpus exposed exact half-channel differences in invert, sepia and
matrices, including an existing wgpu DX12/Vulkan matrix disagreement. Independent
single-pixel regressions preceded fixes. Invert and sepia now evaluate in stored
byte channels with ordered FMA, eliminating unnecessary normalize/unpremultiply/
premultiply round trips. Duplicate old branches were removed.

Ordered matrix FMA alone did not fix the discrepancy. RGB matrix coefficients are
now applied in premultiplied byte units, then rescaled by new/old alpha. This is
algebraically equivalent to transforming straight RGBA, including alpha-to-RGB
coefficients and bias. For original alpha zero, only bias contributes. When alpha
is unchanged, the ratio is exactly one: a further permanent regression proved
that an approximate GPU alpha/alpha division otherwise moved 253.5 below its
rounding boundary on all four routes. This general identity is explicit in both
languages, with no backend-specific output or pixel tolerance.

Opaque alpha also bypasses division with a direct packed-byte branch. Selecting
an identity multiplicative factor was insufficient on three routes; the direct
branch passed the independent 256-pixel sweep. Saturated straight RGB is exactly
one and therefore writes output alpha directly. This fixes the root numerical
issue that initially changed two SVG PNGs, rather than accepting those changes.

## Verification

The corpus covers 67 color/matrix parameter sets over 771 premultiplied pixels,
all four actual production texture/table variants and all four API routes. Four
independent half-channel cases plus 16 f64 straight-RGBA matrix cases cover
RGB-to-alpha, alpha-to-RGB, zero alpha, signed cross coefficients, bias and clamping.
Independent integer oracles also cover source-over and SVG masks. All 80 native
runtime tests, ordinary release tests, real editor round trips, strict Clippy and
50 SPIR-V modules pass. Both independent reviews are closed. Full SVG and examples pass; all 3,471 PNGs
match the immutable pre-change baseline. All six filter GPU tests also pass with both DXC executable paths absent;
no runtime shader compilation occurs. Shader compiler, inventory and artifact
integration tests pass.

This slice adds 16 production inventory variants, bringing the verified count to 67/179. M4 remains
incomplete: remaining filters, full fine and NativeRenderer/Canvas are outstanding.
