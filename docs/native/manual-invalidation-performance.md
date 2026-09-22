# Retained raster invalidation performance (2026-09-22)

[Subsequent cropped-filter spatial-query fix and measurements](cropped-filter-spatial-query.md) address the remaining cropped-filter bottleneck. The results below are its pre-fix baseline.

The materializer previously published `buffer_changes = None` after raster-only
invalidation. Native/shared staging interprets `None` as unknown dirty coverage,
requiring full preparation and upload. At 100,000 nodes a diagnostic DX12 frame
uploaded 39,814,532 bytes and spent about 16 ms recording, despite unchanged data.
The materializer now publishes an explicit empty delta with reusable structure.
The same diagnostic reduced upload to 113,016 bytes and recording to 10-23 us.
These instrumented observations explain the cause; the timings below were taken
with the temporary probes removed.

## Three independent repetitions

Median Criterion mean completed-frame time in milliseconds on RTX 4090, driver
610.62, Windows LUID `fe3e010000000000`. Routes run serially; the second repetition
reverses route order. Each ordinary timed iteration includes two alternating
mutations, rendering, submission and completion; means are divided by two.
Initialization, capture and readback are outside timing.

| Configuration | wgpu DX12 | Native DX12 | wgpu Vulkan | Native Vulkan |
|---|---:|---:|---:|---:|
| scale-backdrop-manual-invalidation-100000 | 0.5849 | 0.3401 | 0.4289 | 0.2761 |
| scale-cropped-filter-manual-invalidation-100000 | 0.9517 | 4.3891 | 0.7147 | 4.9766 |
| scale-manual-invalidation-100 | 0.3439 | 0.1371 | 0.1581 | 0.1126 |
| scale-manual-invalidation-1000 | 0.3119 | 0.1424 | 0.1726 | 0.1169 |
| scale-manual-invalidation-100000 | 0.3188 | 0.2086 | 0.2239 | 0.1499 |
| scale-manual-invalidation-20000 | 0.3480 | 0.2028 | 0.2152 | 0.1497 |
| scale-manual-invalidation-5000 | 0.3342 | 0.1771 | 0.2001 | 0.1391 |

Ordinary 100,000-node invalidation fell from the previous three-repeat native
DX12 median of 21.3442 ms to 0.2086 ms, and native Vulkan from 22.2667 ms to
0.1499 ms (99.0% and 99.3% lower). The earlier dataset is linked below; it is a
separate run, not simultaneous instrumentation or an identical execution context.
Both native routes beat their current wgpu counterpart for all five ordinary
invalidation sizes and the backdrop case.

**Cropped-filter invalidation remains slower than wgpu.** It improved from
29.7101/24.5740 ms to 4.3891/4.9766 ms on native DX12/Vulkan respectively, but
needs further profiling. Other retained outliers, including move/affine updates,
were not remeasured here. This is not a claim of universal native parity.

For ordinary 100,000-node invalidation, the median of the three separately
collected P95 values is 0.2847 ms (DX12) and 0.2258 ms (Vulkan). The largest
observed maxima across the three runs are 0.5119 and 0.4722 ms. Each run collects
64 completion samples for this case; these are bounded observations, not a
worst-case guarantee or window FPS. Cropped-filter P95 remains 7.5440/7.8393 ms,
with observed maxima 9.1736/12.1777 ms.

## Validation and evidence

- Seven configurations, four routes, three repetitions: all captured phases
  matched exact raw RGBA. No tolerance or reference image change.
- Four-route full SVG, example and retained corpus: 7,724 outputs, zero differences.
- Single-threaded release tests, strict all-target Clippy and formatting passed
  for wgpu, DX12 and Vulkan. Focused native GPU regression passed on both APIs.
- CPU regression first failed on the missing delta, then passed. GPU regression
  covers local/full invalidation, first upload and content commits followed by
  invalidation, comparing reused and fresh renderers.
- [Upload invariant and root fix](m1-empty-delta-storage.md#raster-only-upload-deltas).
- [Per-run statistics, pixel hashes and source receipt hash](benchmarks/manual-invalidation-2026-09-22.json).
- [Previous measurements](expanded-backend-comparisons.md).

Local complete evidence is under `target/manual-invalidation-final`,
`target/manual-invalidation-checks` and `target/remaining-corpus/manual-*`.
The exported artifact retains per-run summaries and captured pixel hashes;
raw latency arrays and full source snapshots remain in these local directories.

Reproduce with the existing `scripts/native/retained_benchmark.py`, pinning the
current GPU and DXC paths, `--runs 3` and
`--case scale-manual-invalidation-,scale-backdrop-manual-invalidation-100000,scale-cropped-filter-manual-invalidation-100000`.
