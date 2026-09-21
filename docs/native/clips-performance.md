# Clip dispatch performance

The 384 moving path clips in `backend_comparison_cycles/clips` now take about half
as long on native DX12 and Vulkan, with identical pixels. This follow-up is against
`d2d596ad` and supersedes the clips row in [the previous results](backend-performance-followup.md).

## Measurements

RTX 4090 (driver 610.62), Ryzen 9 9950X3D, 1280 × 800, release builds, the same pinned
physical adapter. Three rounds alternate forward/reverse route order. The table
uses the median of three Criterion per-frame means and three p95 values. PMax is
the worst observed value across all three runs, not a guaranteed latency bound.
Each round separately records 400 frame latencies after 64 warm-up frames; no
outliers are removed. Criterion iterations each complete a 16-frame geometry cycle.

| Backend | Mean before → after (ms) | Reduction | p95 before → after (ms) | PMax before → after (ms) |
|---|---:|---:|---:|---:|
| Native DX12 | 24.48 → 10.91 | 55.5% | 24.93 → 11.41 | 27.38 → 12.52 |
| Native VULKAN | 17.94 → 8.20 | 54.3% | 18.40 → 8.91 | 19.61 → 9.88 |

Both direct Criterion comparisons report `Performance has improved` with p < 0.05.
Current wgpu means are 23.39 ms (DX12) and 24.08 ms (Vulkan).
The native clip scheduler is faster than both corresponding wgpu routes. This is
an offscreen completed-frame benchmark; it does not measure window presentation,
resize, or multi-frame throughput. Neither native result yet meets a 6.94 ms budget
for this stress scene.

## Root cause and implementation

Different clip stacks remain separate painter-ordered batches. Each batch used the
whole frame's active tile list and repeated count/prefix/emit allocation passes,
even though its clip covered only a small region. The fix removes this redundant
work without approximating the paths, changing antialiasing, or merging composites.

- `scene/clip_tiles.rs` intersects pure clip bounds with the viewport and existing
  damage, then uploads immutable per-stack tile lists. Coarse and fine consume the
  same list. Empty intersections dispatch nothing.
- For a complete pure-clip schedule with no glyphs, tile bins already provide a
  conservative particle bound. Disjoint tile ranges reserve one particle per
  candidate draw, two per maximum clip depth, and one terminator. Existing scene
  capacity bounds every range. Uploading these headers once removes GPU count and
  prefix allocation: an eligible batch goes from five coarse dispatches plus fine
  to one coarse dispatch plus fine.
- Regular draw, group, offscreen, and glyph schedules retain regular allocation.
  This prevents an intervening normal coarse pass from overwriting the fixed
  headers. Upload-only CPU vectors are released after recording.
- A clip rejecting a tile explicitly writes the end particle. Returning without
  doing so could expose the previous batch's stream in the reused range. The HLSL
  and WGSL emitters share this requirement. Tile kind values originate in HLSLI.
- Metal retains its existing scheduling; these optimizations are enabled for the
  Windows native backends. There is no new shader compilation dependency or cache
  bypass.

The initial DX12 timing probe separated approximately 7.09 ms before submission
from 17.21 ms waiting afterward. A fixed-slot trial measured 7.18 ms and 4.71 ms,
respectively. Waiting includes synchronization/retirement and must not be described
as pure GPU execution time. CPU submission remains the main next target.

## Validation

- All three Windows feature configurations pass release tests, formatting and
  strict all-target Clippy (`bench-internals` enabled for lint coverage).
- Host regressions cover clipped tile/damage intersections, nested disjoint clips,
  disjoint bounded particle slots, and pure-to-mixed schedule fallback.
- The native GPU regression compares fixed slots with regular allocation and
  repeatedly switches schedules. Removing the rejected-clip terminator makes this
  test fail; restoring it passes on DX12 and Vulkan with validation enabled.
- Eighteen comparison runs each match all 16 clip phase images exactly. A separate
  full 11-scene run matches 176 phase images per native backend, 352 total.
- SVG, examples and retained acceptance passes on native DX12/Vulkan and
  wgpu-DX12/Vulkan: 1,931 outputs per route, 7,724 total, zero differing pixels.
- Standards and correctness/spec reviews have no outstanding findings.

[Compact measurements and verification](benchmarks/clips-2026-09-21.json).
Detailed local evidence is under `target/clips-followup/`: `comparison/receipt.json`,
`checks/receipt.json`, `final-verification/receipt.json`, the four `corpus-*`
receipts, and `rejected-tile-red.log`. Benchmark executables are hash-pinned in the
comparison receipt. The existing Criterion clips fixture remains the performance
regression scenario.
