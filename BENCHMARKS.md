# Retained benchmarks

The retained performance matrix is implemented with Criterion so results remain comparable after
code changes. Every benchmark waits for the submitted GPU work before recording wall time.
Criterion stores reports and named baselines under `target/criterion`.

Create a baseline before a change:

```powershell
.\scripts\ps1\run_retained_benchmarks.ps1 -SaveBaseline main
```

Compare the current code with it:

```powershell
.\scripts\ps1\run_retained_benchmarks.ps1 -Baseline main
```

The complete run contains two suites:

- `retained_scale`: static, revision, variable-length content, variable-length image resources,
  late/sparse image-resource revision, all revisions, move, scene and tail/middle/nested layer
  add/remove, reparent, reorder, single/many layer update, arena fragmentation, full-canvas and
  cropped/offset filter-child revision, backdrop-background revision, and
  plain/cropped-filter/backdrop manual invalidation at 100, 1k, 5k, 20k, and 100k nodes. The
  cropped filter case keeps its pixel region fixed while scene size grows, detecting regressions
  that translate or upload unrelated draws. Tail, middle, and nested layer insertion/removal also
  have separate phase benchmarks so one cheap phase cannot hide a regression in the other.
  Stateful scale cases warm through a complete alternating mutation cycle before sampling, so
  Criterion measures stable updates instead of rebuilding a cold arena for every sample.
- `retained_dirty_ratio`: 13 dirty ratios from 0.5% to 100%, each measured with the persistent
  `RetainedScene` backend in Auto and ForceFull modes plus the preflattened-immediate lower bound.
- `retained_stress`: permanent wall/materialization/transaction scale series for deep hierarchy,
  many cascading backdrops, many root plan fragments, a single large variable-sized chunk, and
  rotating changes that cross the 256-frame delta-overlay boundary.
- `damage_tiles`: CPU-only damage-mask construction, full-mask initialization, intersection and
  population queries, coalesced-rectangle extraction, bounds union, and compact-list
  materialization across single tiles, sparse/disjoint damage, repeated tiny overlap,
  threshold-width rows, medium rectangles, wide strips, dense frames, repeated large overlap, and
  fragmented masks.
- `tile_draw_bins`: CPU-only persistent tile-membership updates for stable bounds, contiguous
  spatial changes, overlapping change ranges, and large changed-draw sets. It isolates reusable
  generation marks and dense tile/bin bitsets from renderer and GPU timing.
- `frame_diff`: CPU-only retained-frame comparison for static frames, one or many revisions,
  disjoint or overlapping painter-order changes, and insertion/removal. It detects repeated frame
  index construction, per-tile adjacency allocation, and duplicate reordered-pair comparisons.
- `tiles_for_bounds`: CPU-only spatial tile traversal for empty, clipped, single-tile, row, medium,
  and full-canvas bounds. It detects temporary collection allocation and large-range iteration
  regressions in retained spatial-index updates and queries.
- `node_draw_order`: CPU-only mapping from cached chunk-local painter order to current physical
  draw-arena slots for small, large, and repeatedly queried nodes. It detects accidental chunk
  recompilation, temporary draw vectors, and sequential-chunk traversal regressions.
- `glyph_capacity`: CPU-only incremental text dependency updates for stable, single-glyph,
  fragmented glyph, changed-run, changed-draw, and mixed edits. It detects temporary affected-draw
  hash sets and materialized/sorted dirty-index vectors.
- `arena_fill`: CPU-only insertion and replacement of zero-initialized scene scratch allocations
  across small tile pools through one-megabyte buffers. It detects temporary zero-vector
  allocation and the resulting second memory pass.
- `checkerboard`: CPU-only recording comparison between the constant two-draw analytic
  checkerboard and one SDF rectangle per visible cell.
- `dirty_ranges`: CPU-only repeated dirty-range collection for scene arenas, persistent path plans,
  and tile bins. It detects capacity loss when upload-owned vectors are taken from long-lived
  staging structures and then dropped.
- `retained_scale/local-scene-resource-cycle` (with `bench-internals`): same-binary A/B of creating
  the fixed buffers/targets in a local offscreen scene allocation set versus recycling them from
  the renderer pool. This conservative microbenchmark excludes workload-dependent scratch targets;
  the cropped-filter scale cases above remain the end-to-end frame measurement.
- `retained_scale/local-scene-resource-mixed-sizes` (with `bench-internals`): two differently sized
  sibling offscreen scenes per frame. It detects pool-order regressions that make stable filters
  exchange allocation sets and recreate their fixed and minimum scratch targets every frame.

Use filters for focused development runs without deleting any matrix entries:

```powershell
.\scripts\ps1\run_retained_benchmarks.ps1 -Benchmark scale -Filter "one-revision/5000"
.\scripts\ps1\run_retained_benchmarks.ps1 -Benchmark dirty-ratio -Filter "persistent-auto/10.0%"
.\scripts\ps1\run_retained_benchmarks.ps1 -Benchmark stress -Filter "many-backdrops-revision"
```

The existing `retained_scale_bench` and `retained_dirty_ratio_bench` examples use the same workload
definitions and remain useful for profiler stage counters such as materialization, prepare, uploaded
bytes, rewritten/compacted tile pages, plan fragments, and arena fragmentation. Alternating
insert/remove scenarios can be profiled independently:

```powershell
cargo run --release --example retained_scale_bench -- --counts 1000,100000 --frames 30 --warmup 3 --scenarios nested-layer-add-remove --phase insert
```

Run the damage-mask matrix directly; it requires the benchmark-only internal adapter:

```powershell
cargo bench --bench damage_tiles --features bench-internals
cargo bench --bench tile_draw_bins --features bench-internals
cargo bench --bench frame_diff --features bench-internals
cargo bench --bench tiles_for_bounds --features bench-internals
cargo bench --bench node_draw_order --features bench-internals
cargo bench --bench glyph_capacity --features bench-internals
cargo bench --bench arena_fill --features bench-internals
cargo bench --bench dirty_ranges --features bench-internals
cargo bench --bench checkerboard
cargo bench --bench retained_scale --features bench-internals -- retained_scale/local-scene-resource
```

## Root draw-batch submission

`cargo bench --bench root_batches` covers native and portable rendering with small targets, few
batches, multi-batch UI-sized frames, and larger frames. Each sample waits for GPU completion.
The renderer is reused across cases, with warmup after each scene/size change; initialization and
shader compilation are outside the measurement. The portable 1600×1000/18 case retains coverage of
the frame-wide ping-pong path previously measured by `portable_root_batches`.

```powershell
cargo bench --bench root_batches -- --save-baseline before
cargo bench --bench root_batches -- --baseline before
```

The early-submission regression tests compare translucent painter order with a single-batch image,
check sparse/unchanged retained history, and compare backdrop dependencies with the portable path.

The same matrix covers coarse allocation-prefix dispatch overhead. The 1600×1000/32 and
2560×1440/64 cases amplify per-batch prefix costs; 1601×1001/32 also exercises partial tiles and
the final partial prefix chunk during resizing. Adapter details are printed so comparisons can
verify that they used the same GPU and backend. Reuse this benchmark when changing shader
scheduling rather than introducing a duplicate scene harness.

`wgpu_coarse_prefix_preserves_ranges_across_chunk_boundaries` directly seeds coarse counts and
checks GPU ranges/chunk totals against a sequential CPU reference. It covers empty and partial
chunks, non-contiguous active tiles, untouched inactive records, and carry across 256/512 chunks
without allocating a large render target. The normal single-threaded release script runs it with
`TILEINK_RUN_WGPU_TESTS=1`.
