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
```
