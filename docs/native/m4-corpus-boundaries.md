# M4 corpus integration regressions

The complete SVG runner exposed integration cases absent from the per-kernel
inventory. Focused four-API Canvas regressions now retain the affected inputs.

## Morphology axis

The native frame encoder assigned a morphology pass's axis to `blur_axis` instead
of `morphology_axis`. The second separable pass repeated horizontal erosion or
dilation. The actual morphology field now receives the shared program's axis.
An asymmetric 11 by 9 scene with radii 1 and 2 reproduces the failure before the
fix; both operators and the SVG zero-radius fixture then match four APIs. This is
an encoder mapping correction, not a change to the morphology algorithm or SVG data.

## Empty tile source

A disjoint SVG filter graph input can carry inverted source bounds. Such an input
has no repeatable tile cells. Passing it to the raw tile kernel's validated signed
coordinate contract rejected an otherwise valid empty filter result. The frame
encoder now clears the requested output region for empty source bounds, retaining
active tile coverage and keeping the low-level coordinate checks intact. The SVG
empty-region fixture reproduces the original rejection and verifies transparent
output through all four APIs.

## Correctness-only examples

The liquid-glass example previously ran profiling loops unconditionally after its
three image outputs. The shared image catalog now only renders those same outputs;
unused profiling-only scene helpers were removed. Standalone benchmarks remain
separate. This follows the user's instruction to stop performance comparisons and
prevents native certification from invoking a wgpu-only profiling constructor.

## Convolution arithmetic

Implicit contraction and per-tap alpha normalization crossed half-byte boundaries
between shader targets. A fractional-weight alpha ramp reproduced differences even
between wgpu DX12 and Vulkan. Both shader languages now accumulate straight RGB
and byte-domain alpha with explicit fused multiply-add, apply divisor/bias in the
same order, and quantize once after premultiplication. Existing independent numeric
oracles, signed weights, sparse regions and preserved-alpha cases remain covered.

Finite subnormal divisors are recognized by their bits instead of a GPU comparison
that can flush them to zero. The exceptional path scales numerator and denominator
by 2^24; exponent-bit scaling prevents fast-math from folding that into an infinite
reciprocal. Very large finite bias uses normalized alpha to avoid overflowing the
intermediate bias times 255. Large divisors scale both operands down through
exponent bits, so a flushed subnormal reciprocal cannot erase a visible quotient. Focused tests include zero kernels, signed tiny weights,
preserved alpha and cancellation of large opposing values. These changes fix the
arithmetic contract rather than adding image-specific tolerances.

## Downsampled blur and liquid glass

The complete example catalog exposed four downsampled blur/glass differences. A
1024 by 64 four-stage regression reads back Box downsample, horizontal Gaussian,
vertical Gaussian and upsample separately. It reproduced the first discrepancy in
the 4 by 4 Box mean: normalized accumulation rounded 73.5 to 73 on wgpu and 74 on
native. Both shaders now accumulate stored channels, derive the sample count from
cell bounds, and perform one explicit fused division/quantization at the end. The
Box output is checked against independent integer sums; subsequent stages retain
exact four-API checks. Nearest sampling is unchanged.

The simple/mixed glass scenes also exposed a scheduler mismatch: native declined
every direct backdrop while wgpu sampled the low-resolution blurred texture
directly. Materializing the upsample first inserts an extra RGBA8 quantization;
it is not pixel-equivalent. Native now invokes the same shared direct blur/glass
schedule. The exact simple example reproduced 12,683 different pixels before the
fix and passes both native APIs afterward. `try_direct_backdrop` returns a result
so recording failures abort the frame instead of silently retrying a different
schedule. A shared scheduler regression verifies no fallback or foreground on
failure. This is a scheduling root-cause fix, with no shader approximation.

## Shared frame policy

Both renderers now use `render::scratch_slots::ScratchSlots` for first-free leases,
occupancy, release and context reset. Physical surfaces and retained displacement
spares remain backend-owned. Native allocation failure rolls back the acquired
lease; resources already recorded remain owned by the ordered batch. Tests cover
exhaustion, holes, growth, transfers and reset. This removes duplicate API-neutral
pool policy without introducing M5 cross-frame GPU allocation reuse.

`render::frame::SubmissionPolicy` explicitly selects `Single` or `EarlyRootBatches`.
Native immediate frames use one ordered submission; wgpu retains its existing
caller-controlled early-root policy. Both enter the same frame budget scheduler.
Existing frame-order/failure tests now exercise the explicit policy.

Both full immediate corpora pass all six routes. See [M4 closeout](m4-completion.md)
for final evidence, approved PNG changes and the remaining M5/platform scope.
