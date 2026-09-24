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

The default `ProgressiveBlurQuality::Balanced` and optional `High` policy control
sampling accuracy independently of strength:

```rust
let blur = ProgressiveBlur::new(start, end, max_std_dev)
    .with_quality(ProgressiveBlurQuality::High);
```

1. Copy the bounded source into an immutable, tightly sized texture.
2. For device sigma <= 1, evaluate the requested normalized 7x7 Gaussian directly
   from the original. Sixteen bilinear fetches combine its adjacent positive taps.
   Sigma below 1/8 pixel is an RGBA8 identity (the omitted tails are negligible).
   This avoids the near-clear ghost edge caused by blending sharp and blurred images.
3. Build Gaussian levels at sigma 1 and successive powers of `2^(1/n)`, where
   `n=2` for Balanced and `n=3` for High. These are continuous variance brackets,
   not discrete strength steps. Incremental horizontal/vertical convolution shares
   a scratch image. Kernels are computed once per level on the CPU; paired taps
   use hardware bilinear filtering. Balanced uses at least 3-sigma support; High
   uses at least 4-sigma support.
4. Only halve resolution when target sigma at the new resolution is at least
   1.5 texels (Balanced) or 2 texels (High). Fine levels therefore stay at full
   resolution. Odd dimensions round up, including one-pixel axes.
5. Reconstruct and interpolate adjacent levels by variance in one output pass.
   Exactly clear pixels are untouched. Higher sigma retains the reduced pyramid
   instead of evaluating a large per-pixel kernel.

Calibration measures the discrete incremental kernel variance and adds the mean
bilinear reconstruction variance `(2*s*s+1)/12` for scale `s>=2`. Phase-aware
half-pixel kernels keep reduction centers aligned with reconstruction. Samples
outside the logical domain are transparent, never sampler-clamped pooled texels.

This fixes the premature-downsampling cause of shallow edge blockiness; it is
not a temporary overlay or opacity workaround. The scale space remains an
approximation to variable Gaussian convolution. Finite source edges, mean
reconstruction calibration, level interpolation and premultiplied RGBA8
quantization still contribute error. The working color space is unchanged.

## Integration and resource ownership

The backend-independent scheduler records a dedicated progressive operation.
Native encoding builds independently sized levels in the queue-owned surface
pool and binds them through the existing 64-entry texture-table ABI. HLSL serves
DX12/Vulkan; a matching Metal implementation uses the same uniform layout.
The source is immutable until the final resolve, preventing read/write aliasing.
Pyramid leases skip clearing because copy/reduction overwrites every logical texel
before sampling; bounds checks exclude uninitialized spare capacity.
Fine levels cost multiple full-resolution images; High stores more levels than
Balanced. One full-size horizontal scratch image is reused across all levels.
The table remains bounded to 64 entries through the maximum device sigma.

The support halo includes the upper bracketing level and reconstruction; a
Gaussian-only `3*sigma` bound is insufficient for accumulated finite kernels. The conservative bound is
`ceil(20*max_std_dev + 8)`. Source coordinates are translated when filtering on
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

Quality regression tests compare an independent double-precision Gaussian
against translated step edges and diagonal detail, and sweep strength across
level boundaries. The previous image-wide RMS test could hide visible local
artifacts. On DX12/RTX 4090 the old step-edge maximum error was 13.36/255 and
integer-translation change was 17/255. With paired Gaussian sampling:

| Policy | Step-edge max error | Translation change | Diagonal max error |
| --- | --- | --- | --- |
| Balanced | 2.53/255 | 1/255 | 3.83/255 |
| High | 1.93/255 | 1/255 | 2.33/255 |

These are fixture measurements, not universal bounds on arbitrary images.
Tests also cover near-zero and maximum sigma, premultiplied alpha, clear/uniform
plateaus, reverse/diagonal gradients, tiny/odd images, sufficient source halos,
DPI/local coordinates, pooled capacity, and retained dirty/clean frames.

The Criterion scenario measures warmed end-to-end submission plus completion,
without readback. It includes scene rendering, backdrop copies, blur, compositing,
CPU recording and GPU execution; it is not a GPU timestamp measurement.

```powershell
$env:TILEINK_BENCH_GPU = '<physical adapter identity>'
cargo bench --bench progressive_blur -- --save-baseline progressive
```

The benchmark covers both quality policies at 512x256 sigma 8/32 and 1920x1080
sigma 2/32/128. Sigma 2 concentrates work in the shallow range. Compare on the
same GPU/backend/driver. The quality implementation's unpaired-tap baseline is
`progressive-quality-scalar`; the optimized version pairs adjacent taps without
changing the Gaussian kernel. The older binomial implementation is a different
quality/cost tradeoff, not a like-for-like performance baseline.

Measured on Windows/DX12, RTX 4090 (`fe3e010000000000`), 2026-09-24;
30 samples, 1-second warmup, 2-second measurement. Times are Criterion means
for end-to-end submission and completion, not GPU timestamps:

| Scene / max sigma | Balanced | High |
| --- | --- | --- |
| 1920x1080 / 2 | 508 us | 561 us |
| 512x256 / 8 | 264 us | 307 us |
| 512x256 / 32 | 319 us | 386 us |
| 1920x1080 / 32 | 668 us | 845 us |
| 1920x1080 / 128 | 730 us | 942 us |

Criterion reported improvement in all ten cases against the same-quality
unpaired implementation (9.8–27.1% lower time). The old lower-quality binomial
version measured 370 us at 1080p/sigma32 and 378 us at sigma128 in this session.
Thus the improved default deliberately costs about 1.8–1.9x at those settings;
High costs about 2.3–2.5x. Quality improvement is not a free performance win.
These observations are machine-specific, not a latency promise.

Validation: full release CPU suite, focused DX12 and Vulkan GPU tests, strict
clippy on both backends, all examples built, and full SVG fixture rendering plus
pixel comparison. Metal has matching source/ABI but needs validation on macOS.
