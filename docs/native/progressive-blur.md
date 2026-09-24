# Progressive blur

`Filter::ProgressiveBlur(ProgressiveBlur::new(start, end, max_std_dev))` implements
the spatial blur discussed in the implementation brief: clear before the start,
smoothly increasing along the start/end vector, and constant after the end. It is
available in ordinary filter layers and backdrop layers. Coordinates and sigma
are logical pixels; canvas scale converts both to device pixels. Reversing the
vector reverses the gradient; coincident endpoints select uniform maximum blur.
Zero sigma is an identity. Invalid endpoints, negative/nonfinite sigma, or a
device sigma above 65536 produce a render error when the filter executes.

## Algorithm and semantics

For a pixel center `p`, `t = clamp(dot(p - start, end - start) / |end - start|²)`
and `sigma = max_std_dev * t² * (3 - 2*t)`. This describes an isotropic variable
Gaussian reference, approximated by the following discrete scale space:

1. Copy the bounded source into an immutable, tightly sized texture.
2. Create a full-resolution `[1,2,1]/4` separable binomial level (variance 0.5).
3. Repeatedly reduce each dimension by two using `[1,3,3,1]/8`, centered at
   `2*p + 0.5`. Ceil extents retain odd dimensions and one-pixel axes.
4. Select the two levels bracketing the desired variance, reconstruct them, and
   interpolate by variance in one output pass. Pixels at sigma zero are untouched.

The binomial kernels use four hardware bilinear samples. Samples crossing the
logical source boundary use explicit transparent reconstruction; allocation
capacity and sampler clamp behavior never define the logical domain.

Level calibration accounts for accumulated reduction variance and the mean
bilinear reconstruction variance over the original integer pixel grid. For scale
`s >= 2`, the latter is `(2*s² + 1)/12`. The first effective variances are
`0, 0.5, 2, 7, 27, ...`. This is a multiscale Gaussian approximation, not an exact
variable-kernel Gaussian and not a clear/max-blur opacity crossfade. Its working
space matches the enclosing filter pipeline, using premultiplied RGBA8 throughout.
Small-radius kernels, finite source edges, reconstruction phase and RGBA8
quantization all contribute to approximation error.

## Integration and resource ownership

The backend-independent scheduler records a dedicated progressive operation.
Native encoding builds independently sized levels in the queue-owned surface
pool and binds them through the existing 64-entry texture-table ABI. HLSL serves
DX12/Vulkan; a matching Metal implementation uses the same uniform layout.
The source is immutable until the final resolve, preventing read/write aliasing.
Pyramid leases skip clearing because copy/reduction overwrites every logical texel
before sampling; bounds checks exclude uninitialized spare capacity.
For a large image the copied source and fine level each occupy N texels, and
reduced levels approach N/3 additional texels (odd dimensions add rounding).

The support halo includes the upper bracketing level and reconstruction; a
Gaussian-only `3*sigma` bound is insufficient. The conservative bound is
`ceil(8*max_std_dev + 4)`. Source coordinates are translated when filtering on
localized offscreen surfaces and when appending/moving canvas content.

Dirty retained filters rebuild their whole stable domain, preserving the
decimation phase; clean outputs reuse the existing retained cache. This is the
intentional correctness policy for this implementation, not sparse pyramid
updates or shared pyramid caching between separate panels.

## Validation and performance

Run ordinary tests in release mode and on one test thread:

```powershell
cargo test --release progressive -- --test-threads=1
$env:TILEINK_NATIVE_GPU = '<physical adapter identity>'
cargo test --release --lib progressive_gpu -- --ignored --test-threads=1
cargo test --release --test progressive_blur_gpu -- --ignored --test-threads=1
cargo run --release --example progressive_blur
```

The independent Gaussian reference uses vertical stripes to evaluate a precise
one-dimensional Gaussian gather at each output pixel, away from boundaries. Its
acceptance threshold is RMS below 12/255 for the documented 0–8 sigma ramp. This
is a focused quality check, not a universal image-error guarantee. Other tests
cover clear/uniform plateaus, premultiplied alpha, reverse/diagonal directions,
tiny/odd extents, DPI, localized surfaces, and retained dirty/clean frames.

The Criterion scenario measures warmed end-to-end submission plus completion,
without readback. It includes scene rendering, backdrop copies, blur, compositing,
CPU recording and GPU execution; it is not a GPU timestamp measurement.

```powershell
$env:TILEINK_BENCH_GPU = '<physical adapter identity>'
cargo bench --bench progressive_blur -- --save-baseline progressive
```

The benchmark covers 512×256 at sigma 8/32 and 1920×1080 at sigma 32/128.
Compare on the same GPU, backend, driver and power state. Kernel optimization is
validated against the initial explicit-tap version using Criterion baselines.

Measured on Windows/DX12, NVIDIA RTX 4090, physical identity `fe3e010000000000`
on 2026-09-24 (30 samples, one-second warmup, two-second measurement):

| Scene | Optimized 95% interval | Criterion change vs explicit taps |
| --- | --- | --- |
| 512x256, sigma 8 | 179.67-185.58 us | -32.52% |
| 512x256, sigma 32 | 189.52-190.97 us | -26.30% |
| 1920x1080, sigma 32 | 367.74-377.76 us | -16.86% |
| 1920x1080, sigma 128 | 377.97-380.27 us | -23.14% |

Criterion reported improvement in all four cases. These are machine-specific
end-to-end observations, not latency promises for other GPUs.
