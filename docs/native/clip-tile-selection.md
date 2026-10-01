# Shared native clip tile selection

`program/scene/clip_tiles.rs` selects conservative clip-local dispatch lists for
Metal, DX12 and Vulkan. The retained path previously collected every matching
active tile before deciding that the list was too dense and discarding it.
Scene preparation became a larger CPU branch in maximized AAPL Replay following
the filter-validation optimization. Retained selection now classifies dense and
empty rectangles using the exact damage bitset before collecting any tile IDs,
and collects each stack once. This removes repeated traversal and discarded
storage at their source without changing frame pacing or GPU coverage.

Related: [active-batch selection](active-batch-selection.md) removes repeated
CPU membership work before clip scheduling, with the same batch set.

## Required invariants

- Only pure clip stacks may restrict dispatch. Clip bounds intersect the viewport;
  coarse/fine kernels still determine actual coverage.
- The sparse budget remains `ceil(original_tile_count / 3)`. Exactly-at-budget
  selection remains sparse. Retained selection uses the existing exact damage
  bitset to count the viewport-clipped rectangle a machine word at a time. A
  count above the budget returns `None`, selecting regular dispatch with the
  original damage list; zero returns `Some(empty)`. Neither case allocates or
  scans individual tile IDs. Only nonempty sparse output filters the original
  list, preserving insertion order, with capacity reserved for the exact count.
  Damage dimensions must match the Canvas tile dimensions, including edge tiles.
- Retained sparse output preserves active-list order. An explicit empty active
  list returns `Some(empty)`, distinct from the regular-dispatch `None` result.
  Partial edge tiles and nonmonotonic IDs retain their existing semantics.
- Current child bounds cannot restrict retained damage: removed or reparented
  content and empty clip batches can still require masks. Child bounds constrain
  only complete repaints with known content. Dense full-repaint generation keeps
  its conservative rectangle-count bound and sorted, deduplicated output.
- Each layer-stack range is collected once per frame in first-use order, including
  stacks whose selection returns regular dispatch (`None`). All complete-repaint
  child bounds from all batches, nested offscreen children, mask content and mask
  branches are still accumulated. Retained damage never uses those child bounds.
  Recording only successful sparse selections must not be used as the visited set:
  otherwise every batch sharing a dense stack repeats the same damage scan.
- Particle allocation, clip masks, inactive history, resource ownership and GPU
  schedules are unchanged. The optimization belongs to the common recording
  layer; no Metal, DX12 or Vulkan shader change is needed.

## Earlier bounded-list experiment, 2026-10-01

The previous traversal regression failed before the bounded-list fix: 10,000 matching inputs were
visited with a sparse budget of three, versus the required four. A second test
covers the exact threshold, zero budget, empty input, order and real clip geometry.
All ten focused clip tests passed then in serial release mode. The list iterator
is now replaced by bitset classification; its threshold/order/empty behavior is
covered through the actual selection entry point and an independent list oracle.

Historical comparisons below used a driver that recorded the first nonempty
chart before viewport prefetch finished. A later audit found 390/780-bar metadata
in some comparison sets; these values are raw observations, not validated
same-workload gains. The latest results require matched preparation in both
binaries.

Real AAPL.POLYGON Replay targets 780 candles, native maximization to 3420x1966 backing
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

## Validated bitset classification and unique stack collection, 2026-10-01

The corrected driver prepares the viewport with a round-trip drag, waits for at
least 780 candles and no loading indicator, then records the current count after
screenshot readback. Both frozen builds contain those same app fixes and the same
optional-element snapshot race fix. All 24 unprofiled runs record exactly 780
candles, 3420x1966 backing pixels, 120 Hz timed input and 400 accepted presents
following two seconds of warmup. No compiler or profiler runs during recording.
Base is `2204718fb0ed57d71c13861a17e7abe0c2862605`.

Three alternating pairs run at native 60 Hz and diagnostic 90/120 Hz. Because the
first 120 Hz set worsens tails, another three pairs run with opposite ordering.
Times are ms, baseline → candidate. Means/P95 are medians of per-run statistics;
maximum is the worst observed interval across all selected runs.

| Target / pairs | Submission mean | Render total mean | Interval P95 | Interval maximum |
|---|---:|---:|---:|---:|
| Native 60 Hz / 3 | 2.723 → 2.582 | 4.287 → 4.173 | 17.900 → 18.066 | 20.169 → 21.005 |
| Diagnostic 90 Hz / 3 | 2.294 → 2.121 | 3.511 → 3.411 | 12.498 → 12.692 | 14.602 → 14.662 |
| Diagnostic 120 Hz, first / 3 | 1.929 → 1.680 | 5.027 → 6.154 | 11.719 → 13.806 | 15.332 → 16.990 |
| Diagnostic 120 Hz, confirmation / 3 | 1.830 → 1.778 | 7.870 → 7.530 | 11.684 → 11.550 | 70.449 → 13.225 |
| Diagnostic 120 Hz, combined / 6 | 1.895 → 1.713 | 6.451 → 6.168 | 11.701 → 12.673 | 70.449 → 16.990 |

The native submission mean improves 5.2% and total mean improves 2.6%. This CPU
headroom is the basis for retaining the change. Combined 120 Hz submission mean
improves 9.6%, but P95 is worse and neither candidate tail meets 8.333 ms. No run is
excluded; the baseline's 70.449 ms outlier does not prove a reliable maximum gain.
Its precise cause is unproven. Opposite tail directions between sets and unchanged
GPU work prevent attributing all acquisition changes to the CPU optimization.
The physical display still reports 60 Hz. Production keeps native refresh and
Vsync, with all temporary refresh/presentation controls removed. DX12/Vulkan share
this CPU path but their hardware gains have not been measured.

Twelve focused clip regressions pass. The collector regressions fail before
unique-range collection. Actual selection tests preserve exact-threshold, order,
empty and dense semantics; an independent per-ID oracle covers holes, reverse
order, non-word-aligned row strides and partial edge tiles. Serial release
all-targets has 901 passes and the same six existing failures (five coarse
glyph/clip oracles and turbulence canonical mismatch), with three unavailable
DXC/historical-reference checks excluded. All 1713 SVG PNG outputs and both blur
qualities match the preceding frozen build byte-for-byte. Examples and native
Metal API-validated presentation pass; formatting and release all-targets Clippy
pass with existing warnings.

The app's English/Chinese `docs/replay-pan-metal-performance.md` reports preserve
all six 120 Hz pair results, remaining hotspots and source/binary provenance.
Frozen baseline SHA-256 is
`504bae14b966d19d6dd9685127bc18fc589b7f59815a9d716c290a03a7297bfe`;
candidate is
`dac3dbf451f5477513299d2eb7acbe5ac8d29ea2d0f9832faab58d81c46912e4`.
