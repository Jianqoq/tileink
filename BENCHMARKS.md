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

- `retained_scale`: static, revision, variable-length content, all revisions, move, scene and
  tail/middle/nested layer add/remove, reparent, reorder, single/many layer update, arena
  fragmentation, full-canvas and cropped/offset filter-child revision, backdrop-background
  revision, and manual invalidation at 100, 1k, 5k, 20k, and 100k nodes. The cropped filter case
  keeps its pixel region fixed while scene size grows, detecting regressions that translate or
  upload unrelated draws. Middle and nested layer insertion/removal also have separate phase
  benchmarks so one cheap phase cannot hide a regression in the other.
- `retained_dirty_ratio`: 13 dirty ratios from 0.5% to 100%, each measured with persistent Auto
  and ForceFull, the flat retained snapshot adapter, the remaining mixed-command legacy fallback,
  their ForceFull variants, and the preflattened-immediate lower bound.

Use filters for focused development runs without deleting any matrix entries:

```powershell
.\scripts\ps1\run_retained_benchmarks.ps1 -Benchmark scale -Filter "one-revision/5000"
.\scripts\ps1\run_retained_benchmarks.ps1 -Benchmark dirty-ratio -Filter "auto/10.0%"
```

The existing `retained_scale_bench` and `retained_dirty_ratio_bench` examples use the same workload
definitions and remain useful for profiler stage counters such as materialization, prepare, uploaded
bytes, tile pages, plan fragments, and arena fragmentation.
