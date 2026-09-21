# Extended Windows backend benchmarks (2026-09-21)

The clips scenario has a further measured native optimization; see [clip dispatch performance](clips-performance.md).

The existing 39 Cargo benchmark targets have now been exercised: 38 ordinary
targets contain 970 cases, and the remaining target compares 11 retained scenes
across wgpu-DX12, native DX12, wgpu-Vulkan and native Vulkan. These counts are
execution coverage, not 970 native-versus-wgpu comparisons.

| Target family | Targets | Cases | Interpretation |
|---|---:|---:|---|
| CPU / general | 18 | 131 | Shared host algorithms |
| Native microbenchmarks | 5 | 13 | Backend allocation, staging and target capacity |
| wgpu suites | 15 | 826 | Dirty ratios, stress, scale, uploads, filters and other existing GPU suites |
| Four-route comparison | 1 | 11 per route | Same completed-frame contract and exact phase pixels |

The retained-scale main series now preserves its renderer, scene and mutation
cursor between samples; initial warmup is excluded. This avoids repeatedly warming
100,000-node fragmentation inputs. It changes benchmark history, not production
performance. Old setup-heavy partial runs remain in the local evidence directory.
The full coverage baseline is `coverage-followup-persistent-wgpu`.

## Comparison results

Windows, NVIDIA RTX 4090 (LUID `0f42010000000000`), driver 610.62, Ryzen 9 9950X3D.
Three repetitions per route; the second reverses route order. Renderer/benchmark
sources were frozen through sampling and corpus validation. Documentation and the
compact result artifact were added afterward.

All 22 native/wgpu **median mean** comparisons meet `mean_parity`. DX12 image
replacement and circular clips are effectively ties (ratios 0.9934 and 0.9947),
not evidence of a meaningful speed advantage. DX12 image replacement was 0.8%
slower in repetition three. Do not extend the median claim to every repetition,
all hardware, every workload or every latency percentile.

The eleventh case replaces all 384 16x16 raster images on every frame. Content
revision is monotonic and does not repeat with the 16-frame geometry cycle.
Captures use identical revisions 1-16 on every route. Criterion chooses iteration
counts independently, so timed revision numbers are not identical across routes;
operation counts, dimensions and mutation rules are identical. The other image
case measures warm retained translation, not replacement.

Each route matched all 176 raw RGBA phase images in every run. Transparent RGB
is included. Device/pipeline initialization, shader compilation and captures are
outside timing. These are completed offscreen frames, not window presentation or
multiple frames in flight. Each latency series has 64 warmup and 400 measured
frames after Criterion analysis.

Means below are the median of three Criterion per-frame means, in milliseconds.
The second table uses the median of the three p95 values and the **worst observed
maximum** across all three runs. No latency samples were removed.

| Case | wgpu DX12 | Native DX12 | wgpu Vulkan | Native Vulkan |
|---|---:|---:|---:|---:|
| unchanged | 0.000295 | 0.000199 | 0.001158 | 0.000200 |
| sparse | 0.290058 | 0.166256 | 0.197480 | 0.120706 |
| full | 0.630247 | 0.495471 | 0.513119 | 0.419630 |
| resize | 0.357982 | 0.199414 | 0.242531 | 0.156162 |
| blur | 3.035436 | 1.358898 | 2.349119 | 0.982821 |
| text | 0.752174 | 0.547061 | 0.644542 | 0.496724 |
| images | 0.629766 | 0.519938 | 0.565226 | 0.484477 |
| image_replace | 9.057691 | 8.997745 | 9.622788 | 8.936033 |
| clips | 24.348548 | 24.219311 | 24.812905 | 17.992730 |
| paths | 0.870910 | 0.753240 | 0.770913 | 0.741202 |
| large | 5.099383 | 4.466919 | 5.602954 | 4.936016 |

| Case | wgpu DX12 p95 / max | Native DX12 p95 / max | wgpu Vulkan p95 / max | Native Vulkan p95 / max |
|---|---:|---:|---:|---:|
| unchanged | 0.0004 / 0.0301 | 0.0003 / 0.0007 | 0.0017 / 0.0100 | 0.0003 / 0.0006 |
| sparse | 0.4290 / 1.3894 | 0.2199 / 0.5704 | 0.2456 / 1.9258 | 0.1547 / 1.8155 |
| full | 0.9379 / 1.9209 | 0.7535 / 1.4976 | 0.8163 / 1.4604 | 0.7504 / 1.5312 |
| resize | 0.5637 / 1.0661 | 0.2503 / 0.6414 | 0.4123 / 0.6910 | 0.1932 / 0.4828 |
| blur | 3.5478 / 6.5062 | 1.5824 / 2.0927 | 2.7893 / 4.6184 | 1.1427 / 2.1876 |
| text | 1.3398 / 2.1603 | 0.7901 / 1.8906 | 0.8786 / 1.6077 | 0.7253 / 2.1687 |
| images | 0.9286 / 1.8491 | 0.8805 / 1.8858 | 0.8287 / 2.3798 | 0.7560 / 2.1964 |
| image_replace | 11.8183 / 16.2260 | 11.0753 / 14.7328 | 11.5864 / 15.6912 | 11.7685 / 15.7054 |
| clips | 25.7087 / 28.8047 | 24.7108 / 26.2830 | 26.2647 / 31.4455 | 18.7786 / 19.9714 |
| paths | 1.2177 / 1.8600 | 1.0181 / 1.7376 | 0.9740 / 2.7757 | 1.0912 / 2.5137 |
| large | 7.1330 / 8.7542 | 6.4494 / 8.3377 | 7.3429 / 10.9292 | 7.5901 / 10.4537 |

Native p95 is lower in 19/22 comparisons. Vulkan image replacement, paths and large
scenes retain higher p95 values. Native worst maxima are lower in 18/22: DX12 warm
images and Vulkan full updates, text and image replacement retain higher maxima.
Therefore the mean target is met, but a claim that all tail latencies beat wgpu
would be incorrect. Observed maxima are not hard latency bounds.

## Changes and their evidence

- Fragmented DX12 buffer uploads: 1,542 small CopyBufferRegion calls previously
  moved only 144,128 bytes. Coalescing adjacent ranges and dispatching the existing
  range-scatter shader for sufficiently fragmented updates removes the measured
  CPU recording bottleneck. Small/unaddressable updates keep ordinary copies;
  Vulkan retains its batched-copy path. The isolated Criterion path trial improved
  from 1,088 to 717 microseconds with exact pixels.
- Native image storage: upload directly into pooled persistent images instead of
  allocating a transient image and copying it into a second snapshot. Pool keys
  preserve extent, layer count and 2D/array kind. Pinned versions are never reused;
  queue ordering preserves earlier submissions. Initialization/content versions
  are published only after accepted submission. Metal retains its existing GPU
  snapshot path; no Mac validation is claimed.
- DX12 image staging: write rows directly into mapped, retired upload buffers,
  honoring per-layer footprints. This removes the extra padded CPU Vec and fresh
  upload allocation. The mapping guard unmaps during normal return and unwind;
  cached buffers are available only after confirmed GPU completion.
- Native CPU pixels: retain immutable Rc-owned pixels for single-page atlases and
  independent textures, removing another complete CPU copy. Multi-page arrays
  still assemble contiguous layers once. Byte conversion borrows through bytemuck;
  allocation ownership is never reinterpreted. wgpu keeps its existing owned Vec
  representation because it consumes upload data directly.
- Benchmark compilation: native bench-internals builds now expose the intended
  uniform and dirty-bin helpers and use the correct HashSet. Normal dependency
  tests prevent cfg(test) from hiding these gate errors.

Isolated image-replacement trials, each with 16 exact images, established the
individual changes' direction: image storage retention reduced DX12 by 15.4% and
Vulkan by 31.3%; DX12 direct staging reduced 13.48 to 7.96 ms; native shared CPU
pixels reduced DX12 7.67 to 4.47 ms and Vulkan 7.74 to 4.33 ms. These isolated
numbers are not interchangeable with the full-sequence table above.

The earlier 11-case full matrix exposed native image replacement at 2.074 times
wgpu on DX12 and 1.442 times on Vulkan. It is preserved under
`target/backend-followup/scatter-final-three-rounds/`; its `passed` field certifies
execution/pixels, while `mean_parity` is false. Later interrupted diagnostic runs
are explicitly marked incomplete, not final acceptance.

A dense per-dispatch state table experiment measured 24.365 versus 24.369 ms
(Criterion: no change) and was removed. Barrier batching was also removed after
no significant benefit. No temporary profiling instrumentation remains.

## Verification and limitations

Release tests ran single-threaded for all three Windows features; strict release
Clippy passed for all targets with bench-internals. Both native APIs passed the
six focused image GPU regressions; Vulkan synchronization validation was enabled.
Regression coverage includes fragmented queued uploads/gaps, initialized arrays,
old pinned versions, abandoned submissions, row-pitch changes, shared CPU pixel
ownership, multi-page layer order and persistent benchmark cursors.

Each of all four routes matched the approved corpus: 1,712 SVGs, 45 examples and
174 retained outputs, totaling 7,724 outputs with zero differing pixels. No new
human PNG approval was needed. This includes full Windows validation, not macOS
compilation or GPU execution.

The 14 legacy image-upload microbenchmarks exclude CPU packing and time allocation,
upload, submission and completion. An attempted shared-pixel representation for
wgpu was not retained. Adjacent and reversed binary controls also exposed large
cross-run variability: the 2048x512/16 incremental case changed from a 9% apparent
new-version regression to a 61% old-version regression after reversing order.
All logs are retained; these suites are coverage evidence, not a claim of stable
speedups or 14 passing native parity comparisons. The final wgpu storage policy is
the original owned representation.

Remaining comparison coverage includes native equivalents of the wgpu-only
stage/dirty-ratio/filter/scale matrices, window presentation and resize, pipelined
throughput, end-to-end cold app startup, other physical GPUs, Linux and macOS.
The wgpu suite's native/portable *texture* modes do not mean native API backends.
The startup microbenchmark measures renderer-to-first-frame with an existing
device; it is not cold process startup. No 144 FPS window claim follows from these
offscreen numbers.

## Evidence

- [Compact results and coverage inventory](benchmarks/windows-2026-09-21.json).
- `target/backend-followup/native-pixels-three-rounds/receipt.json`: complete
  source/executable hashes, all Criterion estimates and raw latency samples.
- `target/backend-followup/native-pixels-final-verification/receipt.json` and
  `shared-images-corpus-<route>/receipt.json`: final execution and exact corpus checks.
- `coverage-summary.json`, `wgpu-coverage/receipt.json`: 38-target coverage.
- `image-retention-trial`, `image-staging-trial`, `shared-pixels-trial`: isolated
  before/after evidence. `native-pixels-upload-adjacent` and
  `native-pixels-upload-reversed` retain the legacy controls.

The paths above are under `target/backend-followup/` unless fully specified.
Final Standards and Spec reviews found no unresolved code issues. Performance
limitations and remaining coverage are explicitly retained above.
