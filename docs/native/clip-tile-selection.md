# Shared native clip tile selection

`program/scene/clip_tiles.rs` selects conservative clip-local dispatch lists for
Metal, DX12 and Vulkan. The retained path previously collected every matching
active tile before deciding that the list was too dense and discarding it.
Scene preparation is now a larger CPU branch in maximized AAPL Replay, following
the filter-validation optimization. Stop selection once dense dispatch is certain;
this removes unnecessary traversal and storage at their source, rather than
changing frame pacing or GPU coverage.

## Required invariants

- Only pure clip stacks may restrict dispatch. Clip bounds intersect the viewport;
  coarse/fine kernels still determine actual coverage.
- The sparse budget remains `ceil(original_tile_count / 3)`. Exactly-at-budget
  selection remains sparse. The first matching tile beyond that budget returns
  `None`, selecting the existing regular dispatch with the original damage list.
  No remaining matching tiles need collecting after that decision is certain.
- Retained sparse output preserves active-list order. An explicit empty active
  list returns `Some(empty)`, distinct from the regular-dispatch `None` result.
  Partial edge tiles and nonmonotonic IDs retain their existing semantics.
- Current child bounds cannot restrict retained damage: removed or reparented
  content and empty clip batches can still require masks. Child bounds constrain
  only complete repaints with known content. Dense full-repaint generation keeps
  its conservative rectangle-count bound and sorted, deduplicated output.
- Particle allocation, clip masks, inactive history, resource ownership and GPU
  schedules are unchanged. The optimization belongs to the common recording
  layer; no Metal, DX12 or Vulkan shader change is needed.

## Validation, 2026-10-01

The traversal regression failed before the fix: 10,000 matching inputs were
visited with a sparse budget of three, versus the required four. A second test
covers the exact threshold, zero budget, empty input, order and real clip geometry.
All ten focused clip tests pass in serial release mode.

Real AAPL.POLYGON Replay uses 780 candles, native maximization to 3420x1966 backing
pixels, 120 Hz timed drag input and 400 accepted presents after two seconds of
warmup. Apple M5, macOS 27.0 (26A428), Rust 1.98.1. Three alternating candidate/base
pairs at each scheduler target run without compilation or profilers. Both frozen
executables have identical temporary benchmark-only refresh controls, subsequently
removed from source. Base is `131bcfde`; no GPU kernels change.

| Scheduler target | Median submission mean, ms | Median render total mean, ms | Median interval p95, ms |
|---|---:|---:|---:|
| 60 Hz | 2.838 → 2.777 | 4.203 → 4.210 | 18.270 → 18.140 |
| 90 Hz diagnostic | 2.465 → 2.386 | 3.594 → 3.555 | 12.329 → 12.468 |
| 120 Hz diagnostic | 1.927 → 1.887 | 7.369 → 5.094 | 13.639 → 13.561 |

All six 60/90 Hz submission means improve, but the gain is small: 2.2% at 60 Hz and 3.2%
at 90 Hz. Total rendering and interval tails do not consistently improve. The
physical display reports 60 Hz; 90/s accepted-present throughput does not certify
90 visible FPS or stable 11.11 ms pacing. At 120 Hz, submission and tails are mixed
between pairs; candidate mean cadence is 8.330 ms, median p95 13.561 ms and worst
observed interval 16.766 ms. Acquisition falls 4.557 → 2.234 ms, but the unchanged
GPU workload and variable queue/drawable pressure prevent attributing that larger
wait reduction to this small CPU change alone. Stable 120 FPS and DX12/Vulkan
hardware gains are not certified. The final production build retains native
refresh querying and Vsync, with no diagnostic controls.

Metal API-validated serial release all-targets: 896 passes and the same six
existing failures (five coarse glyph/clip oracles and the 1600-pixel turbulence
canonical mismatch). Two unavailable DXC tests and one absent historical WGSL
reference are excluded. All 1712 SVG fixtures plus tiger and both blur qualities
match the prior frozen build byte-for-byte. Native presentation completes eight
frames and two sizes.
Formatting and release all-targets Clippy pass with existing warnings.
