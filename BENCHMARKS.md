# Retained benchmarks

## Native comparison follow-up

The later user request restores native-versus-wgpu performance work. The previous
closeout waiver below remains historical. The four-route Criterion procedure and
resource-reuse invariants are documented in
[native backend performance](docs/native/backend-performance.md).

## M0/M1 delivery decision (2026-09-13)

The same user decision applies to [M6 Windows acceptance](docs/native/m6-windows.md):
no Criterion or resize timing comparison is run or used as a completion gate.
Exact pixels, lifecycle semantics, API validation and build checks remain required.

On 2026-09-13 the user instructed: **“不用比较性能了”** (stop performance comparisons). For this M0/M1 closeout, no further Criterion comparisons, resize timing or telemetry are required. Performance is not an acceptance gate for this delivery, and `performance_accepted` remains false. Historical regressions, rejected experiments and incomplete timing runs remain preserved. This does not waive functionality, exact pixels, feature/dependency checks, release tests or code review.

The procedures below remain historical/method documentation; they do not request additional performance work for this closeout.



The retained performance matrix is implemented with Criterion so results remain comparable after

code changes. Every benchmark waits for the submitted GPU work before recording wall time.

Criterion stores reports and named baselines under `target/criterion`.



The retained, root-batch and range-scatter matrices select the API and physical GPU explicitly.

Set `TILEINK_BENCH_API` to `vulkan` (the default) or `dx12`, and `TILEINK_BENCH_GPU` to the

physical identity recorded by the parity runner. DX12 also requires the pinned

`TILEINK_PARITY_DXCOMPILER` DLL. An unavailable requested GPU/API fails instead of falling back.

Every process prints its actual API, GPU identity, driver, features, memory hints and compiler.

Retained measurements keep `MemoryUsage` and timestamp capabilities; root-batch and upload

measurements keep `Performance` without timestamp queries.



```powershell

$env:TILEINK_BENCH_API = "vulkan"

$env:TILEINK_BENCH_GPU = "<physical LUID from parity manifest; UUID on Linux>"

# Required when TILEINK_BENCH_API is dx12:

$env:TILEINK_PARITY_DXCOMPILER = "<absolute path to the pinned dxcompiler.dll>"

```



Create a baseline before a change:



```powershell

.\scripts\ps1\run_retained_benchmarks.ps1 -SaveBaseline main

```



Compare the current code with it:



```powershell

.\scripts\ps1\run_retained_benchmarks.ps1 -Baseline main

```



The complete run contains the following series:



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



The original retained wall series remain **profiled diagnostic measurements**: they include

GPU timestamp resolve/readback and profile parsing, and exclude scene transactions. Their

materialization/transaction component series remain available. They must not be compared directly

to production timings or used to claim production-frame speedups.



`retained_scale_production`, `retained_dirty_ratio_production` and

`retained_stress/*/production-wall` reuse every corresponding workload, size and insert/remove

phase with profiling fully disabled. Production wall spans the transaction, render and GPU

completion. Combined production measurements count one complete two-frame mutation cycle per

Criterion iteration, including odd iteration counts; their time and throughput are per cycle.

Divide cycle time by two only when reporting the derived mean frame cost. Independent insert/remove

phase series still count one timed frame per iteration. Combined scale and dirty-ratio

diagnostics also measure complete two-frame mutation cycles, including their component timings.

Their new names are `retained_scale_cycles`, `retained_materialize_cycles`, and

`retained_dirty_ratio_cycles`; their time and throughput are per cycle. This fixes unequal

mutation-phase weighting when Criterion chooses different odd iteration counts for two versions.

Do not compare these cycle results directly with archived diagnostic results in per-frame units.

Other stress diagnostic units are unchanged, including the twelve combined root-fragment

components that already use complete two-frame cycles as specified below.

The production `delta-rotation` case is a separate fixed trace:

each iteration creates the same initial state, warms 255 frames outside timing, and measures the

next 255 frames. Its time and throughput are per 255-frame trace, not a complete node rotation.

This fixes the node prefix and delta-merge boundaries independently of Criterion's iteration count.

Dirty-ratio `preflat-immediate` remains a preflattened rendering lower bound; its

input construction is outside timing. No timing mode changes the renderer's pixel algorithms.



The custom GPU loops prime each selected case once before Criterion starts elapsed-time

calibration. Each measured batch still creates its own workload, renderer, resources, and history;

the benchmark context retains its device, queue, and supported driver pipeline cache in the process.

This fixes first-use initialization distorting the iteration estimate while leaving the reported

transaction/render/completion interval unchanged. Listing or filtering out a case does not prime

it. Initial work still occurs in the process, but these steady-state samples and logs do not

separately measure its cost. Large workloads may still need one iteration per sample when the

actual workload or batch preparation is expensive. Inspect the raw iteration counts and durations

before treating a requested measurement duration as achieved steady-state sample time.



Without `bench-internals`, the five existing GPU benchmark entries cover 761 logical cases per API:

390 scale (235 diagnostic + 155 production), 78 dirty-ratio (39 + 39), 212 stress

(160 numeric diagnostic + 12 presence-only + 40 production), 66 root-batch and 15 upload

cases. This is 749 numeric cases plus 12 independently observed absent scopes.

Preserve all logical entries for the M0 baseline; focused filters and

`--quick` are development aids, not the complete acceptance matrix. Keep identical benchmark and

support sources on the baseline and current renderer revisions, with separate `CRITERION_HOME`

folders and retained raw samples. Run complete baseline/current comparisons on each API, then

apply a declared repeat/control protocol to significant regressions without hiding initial results.



Range-scatter's three methods include GPU completion but operate on already packed inputs;

CPU `pack()` work is outside timing. Its `upload_ranges_256k_e2e` name denotes this upload operation,

not an entire scene frame. Root-batch timing similarly starts from an already constructed scene.



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



## Pattern sampling



`cargo bench --bench pattern_sampling` renders the existing rotated context-pattern SVG at

300 and 1600 pixels in native/portable WGPU texture modes. It measures completed full frames

into a transient external target. Parsing, pattern prerendering, shader compilation, warmup

and diagnostic readback are outside the measured interval. The adapter is printed in the log.

This guards the cost of the compensated pattern-coordinate calculation; it is not evidence

of a native-API speedup. Save and compare Criterion baselines with identical benchmark sources:



```powershell

cargo bench --bench pattern_sampling -- --save-baseline before

cargo bench --bench pattern_sampling -- --baseline before

```



## Numeric raster and filter changes



`numeric_raster` measures completed full frames for diagonal geometry, a star clip,

linear gradient, drop-shadow blur, turbulence, and specular lighting at widths 300

and 1600, in both WGPU texture modes. Parsing, pipeline creation, warmup and readback

are excluded; each measured frame waits for GPU completion. It explicitly selects

the API, prints adapter metadata, requires hardware and refuses missing native-texture

features. Run the APIs serially and use identical benchmark sources in both checkouts:



```powershell

$env:TILEINK_BENCH_API = "vulkan"

cargo bench --bench numeric_raster -- --save-baseline before

cargo bench --bench numeric_raster -- --baseline before

$env:TILEINK_BENCH_API = "dx12"

$env:TILEINK_PARITY_DXCOMPILER = "C:/path/to/pinned/dxcompiler.dll"

cargo bench --bench numeric_raster -- --save-baseline before

cargo bench --bench numeric_raster -- --baseline before

```



Use the same physical GPU and resolved DXC binary for each comparison. This benchmark

covers the changed numerical workloads; it does not measure swapchain configuration,

resize PMax, or native-API performance. Results and outstanding gates belong in

`NATIVE_BACKEND_PROGRESS.md` rather than being inferred from the shader instruction count.





## Portable retained history with offscreen layers



`retained_portable_history` measures completed incremental frames with an unchanged

offscreen filter and a small moving/edited leaf, at 300x201 and 1601x1001. It forces

the portable texture path, warms a complete mutation cycle, and includes transaction,

recording, submission and GPU completion in timing. Final full pixel comparison with

ForceFull is outside timing and is required; a fast result with wrong history fails.

All four mutation phases are checked before timing, and each Criterion iteration

is one full four-frame cycle (with frame throughput), preventing phase-weight bias.

Set `TILEINK_BENCH_GPU` to the same physical identity used by the reference runner.



```powershell

$env:TILEINK_BENCH_API = "vulkan"

cargo bench --bench retained_portable_history -- --save-baseline before

cargo bench --bench retained_portable_history -- --baseline before

$env:TILEINK_BENCH_API = "dx12"

$env:TILEINK_PARITY_DXCOMPILER = "C:/path/to/pinned/dxcompiler.dll"

cargo bench --bench retained_portable_history -- --save-baseline before

cargo bench --bench retained_portable_history -- --baseline before

```



Use identical benchmark sources in both revisions and serialize GPU jobs. This

scenario checks the general offscreen execution path; direct-root ping-pong remains

covered separately by `root_batches`. It is not a swapchain/resize benchmark.





## Retained target resize



`retained_resize` measures a fixed 64-frame grow/shrink sequence from 961×601 to

1217×761. Retained geometry stays unchanged; the transaction changes the output

size and the renderer recreates owned target/history resources as required. The

scene has 384 translucent rectangles and an offscreen blur. Both WGPU texture

modes run, pipeline compilation is excluded and checked after warmup, and every

measured frame completes on the GPU. Before timing, every size in one complete

cycle must match ForceFull. One Criterion iteration is a 64-frame cycle, so

samples always contain identical size weights; time is reported per cycle and

throughput in frames. Divide cycle time by 64 only when reporting mean frame time.



```powershell

$env:TILEINK_BENCH_API = "vulkan" # or dx12 with pinned TILEINK_PARITY_DXCOMPILER

$env:TILEINK_BENCH_GPU = "<physical LUID from parity manifest; UUID on Linux>"

cargo bench --bench retained_resize -- --save-baseline before

cargo bench --bench retained_resize -- --baseline before

cargo run --release --example wgpu_resize_profile -- vulkan target/resize-new-run.json

```



The profile example uses the same scene/cycle, with 256 recorded frames per

texture mode without enabling the profiler. It reports nearest-rank P50/P95, PMax

and its exact scene-update, render/record/submit and GPU-wait partition. A separate

256-frame pass enables profiling for diagnostic CPU/GPU stages, including its

query resolve/copy/map overhead. Those stages overlap and belong to different

frames; never present them as the production PMax breakdown. No diagnostic

readback enters the production distribution. Use fresh output paths,

alternate API/revision runs and keep the recorded binary/adapter identity. This

measures Tileink-owned target changes, not window acquire/configure/present.



### Longer numeric-raster comparisons



`numeric_raster` keeps 20 samples, 2 seconds of warmup and 4 seconds of measurement

as its defaults. Its configuration is set before Criterion parses command-line

arguments, so controlled repeats can increase observation time without changing

inputs, GPU completion, significance or noise thresholds:



```powershell

cargo bench --bench numeric_raster -- --sample-size 60 --warm-up-time 5 --measurement-time 15

```



Apply identical arguments to both binaries, preserve every forward/reverse run,

and report the observed sampling parameters. This does not replace the ordinary

matrix or permit rerunning until a favorable classification appears. Same-binary

control runs help identify environmental drift independently of code changes.





### Retained benchmark pipeline cache



Set `TILEINK_BENCH_PIPELINE_CACHE=1` for a matched before/current steady-state

matrix. The device helper requests `PIPELINE_CACHE` only when the selected adapter

supports it and records `pipeline_cache_requested` and

`pipeline_cache_feature_enabled`. These describe device capabilities only.

`BenchContext` separately records `shared_pipeline_cache_present` when it

creates and supplies the cache; other benchmarks do not imply cache reuse. `0` (the default) preserves

uncached setup. Original rendering capabilities, timestamp policy and memory

hints are retained; cache support is an additional setup capability, not a change

of texture path. Unsupported APIs explicitly record cache disabled.



`BenchContext` owns one process-local driver cache when supported and requested.

Default diagnostic entry points request their device directly with the same cache

policy, preserving the original default adapter and capability selection. Each measurement batch still

creates a fresh Renderer, scene/materializer, upload buffers and output history.

Where driver caching is supported, this reduces repeated untimed driver

compilation without carrying mutable frame state or capacity from one Criterion

batch into another. Existing iteration

counts, warmup, production/diagnostic timing boundaries and completion waits are

unchanged. The same cache policy and benchmark source must be used on both sides.



This setup adjustment was prompted by actual instruction-pointer samples: the

busy dirty-ratio preflight thread accumulated about 619 CPU seconds, and all

eight instruction-pointer samples of that thread were in `nvgpucomp64.dll`. Such setup cost is

separate from the frame times returned to Criterion. Report cold initialization

separately; a faster benchmark run is not a renderer frame-time improvement.





### Root-fragment timing and absence evidence



The four `retained.root_fragment.*` scopes instrument insertion in

`append_root_plan_fragment`. Removal still performs plan, spatial-index and

metadata work, but those operations are not timed by these insertion scopes.

An absent entry is not a zero-cost removal measurement.



`retained_stress` retains 212 logical case IDs: 200 have numeric Criterion results

and 12 `many-root-layers-remove/root-fragment-*/{8,32,128}` IDs require independent

`NotExecuted` evidence. The explicit `retained_stage_presence` example records

profiled frame count, entry count, timed-entry count and nullable elapsed time,

with insertion as a positive control. Unobserved profiles, missing durations and

executed zero-duration scopes fail. Criterion performs no presence preflight

outside its own selection; its actual profiled removal batches verify that the

insertion scopes remain absent, including later frames.



Only the 12 **combined**

`many-root-layers-add-remove/root-fragment-*/{8,32,128}` numeric metrics now use a

complete two-frame insertion/removal cycle per Criterion unit. Their time is the

insertion scope's total within that cycle, not whole-cycle production time. Their

throughput covers both frames. Independent insertion metrics remain one timed

frame per unit; the other diagnostic and production units are unchanged. Never

compare these twelve combined metrics with an old per-frame `before` directory.

Use a fresh unit-versioned baseline on both revisions and retain the old results

as historical evidence. Stable case IDs alone do not prove compatible units.



```powershell

$env:TILEINK_BENCH_API = "vulkan" # repeat with dx12 and the pinned compiler

$env:TILEINK_BENCH_GPU = "<physical LUID from parity manifest>"

$env:TILEINK_BENCH_PIPELINE_CACHE = "1"

$env:TILEINK_BENCH_OBSERVATION_FRAMES = "2048"

cargo run --release --example retained_stage_presence

```



The complete comparison runner must verify all 24 presence/control IDs for both

revisions on the selected physical GPU and each API, and retain the declared

frame prefix. A default 20-frame observation alone is not long-prefix evidence.

Validate actual positive raw samples and saved baseline files after each Criterion

process: Criterion can log an error and still exit zero. Neither exit status nor

`Collecting` line counts alone certify a completed baseline. Missing data is an

error; it is never classified as unchanged or converted into a synthetic duration.



## Isolated M1 image-resource upload scope



`image_resource_uploads` measures the production WGPU image uploader during a

one-page to two-page atlas growth. Four padded 1022x1022 raster images occupy a 2048x2048 atlas;

a fifth adds a page while all standalone images remain clean. Seven fixed input

combinations cover no standalone image, and 1/4/16 standalone images at 2048x32

or 2048x512. A width of 2048 plus padding exceeds the atlas page but fits

the device limit, so the production packer selects independent textures. Each runs incremental upload and an explicit full-upload control,

for 14 cases per explicitly selected API.



The measured operation includes atlas allocation growth, uploads, queue submission

and GPU completion. Image packing, initial allocation/upload/completion and input

destruction are outside timing (`iter_batched_ref`, `PerIteration`). There is no

shader rendering, profiling readback or production-frame speedup claim. The largest

case starts with 64 MiB of clean standalone texels; unnecessary retransmission is

therefore observable without changing the requested atlas update.



```powershell

$env:TILEINK_BENCH_API = 'vulkan' # repeat with dx12

$env:TILEINK_BENCH_GPU = '<physical LUID from parity manifest>'

$env:TILEINK_BENCH_PIPELINE_CACHE = '1'

$env:TILEINK_PARITY_DXCOMPILER = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\dxcompiler.dll'

cargo bench --release --features bench-internals --bench image_resource_uploads -- --noplot

```



Defaults are 10 samples, 1-second warmup and 2-second measurement. They are set in

Criterion's top-level configuration so CLI follow-up settings can override them.

Freeze identical benchmark/support source and each binary for before/current

comparisons; use separate Criterion homes per API/order. The before revision for

the upload-scope optimization must already initialize clean atlas pages correctly;

the original lost-page implementation is a correctness RED control, not an

acceptable performance baseline. Both orders and unchanged explicit-full controls

remain part of the comparison. The completed [atlas review](docs/native/m1-atlas-uploads.md)
retains all initial flags and their follow-ups. The fixed balanced process experiment
found current/old −0.8166%, approximate 95% CI [−2.4904%, +1.0013%], p=0.4571;
the local gate was accepted under the plan's no-significant-regression criterion.
The upper bound exceeds +1%, so this is not a strict equivalence or noninferiority
claim. Whole-M1 acceptance still requires the final combined-production matrix.





## Filter sampling during resize



`cargo bench --bench filter_sampling` runs the production WGPU liquid-glass filter over a

checkerboard at fixed 1280×960 size and through four nearby window sizes. It prebuilds scenes

and targets, warms pipelines/capacity growth, then measures complete serial render/queue-completion

cycles. No scene creation or pipeline compilation belongs to the timed region.



Set `TILEINK_BENCH_API=dx12|vulkan`, `TILEINK_WGPU_MODE=native|portable` and the explicit

`TILEINK_BENCH_GPU` physical identity. `TILEINK_SAMPLING_BENCH_OUTPUT` selects an isolated

Criterion directory for archived baseline/control comparisons. Each iteration is one complete

cycle, and the throughput records its frame count. Run paired baseline/current in both orders

with same-binary controls, on an otherwise idle GPU; a completed run alone is not acceptance.





## Lazy filter factory measurements



`filter_compilation` measures first-filter factory work and calls to an already

initialized kernel separately. The renderer uses an existing device; initial clear

setup and input destruction are outside the timed operation. Entry points share

shader modules only within the owning device/texture variant and complete binding

mask; compute pipelines remain independently lazy.



These CPU factory measurements are not complete frames and are not process-cold

startup. Separate fresh-process observations include the actual first render and

GPU completion; driver cache state is recorded only where observable. Warmed

whole-frame comparisons wait for GPU completion and omit diagnostic readback.

See [the filter module cache report](docs/native/m1-filter-module-cache.md) for the

measured scopes, controls, source versions and cache-state limitations.



## Bulk-removal damage sources



The production `retained_scale_production/arena-fragmentation` benchmark measures

a complete two-frame removal/insertion cycle, including scene mutation, rendering

and GPU completion. Both versions use the same workload and calibrated harness.

It detects the cost of constructing command-tree damage sources that a connected,

already-resolved delta will never consume. The optimization preserves all target

tile coverage and indexed backdrop IDs, while collecting oracle sources only when

the renderer needs that propagation path.



The stage profiler is a separate diagnostic: its transaction and CPU stages are

per-frame means, while its wall column is a per-frame median with profiling work.

Neither is a substitute for the unprofiled Criterion cycle duration. See [the

damage source report](docs/native/m1-damage-sources.md) and [the original-M0

comparison](docs/native/m1-original-final.md). Resize traces likewise retain their

actual production PMax and its frame; diagnostic stage timings must never replace

that maximum or be presented as application swapchain measurements.

## Expanded backend comparisons

See [the 181 retained / 62 immediate / 44 burst four-route report](docs/native/expanded-backend-comparisons.md). Execution and exact-pixel coverage passed; broad performance parity has not been achieved.
