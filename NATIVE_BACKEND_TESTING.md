# Native backend validation

## Windows M4 immediate corpus

The existing `wgpu_backend_parity` example supports `--native` to compare complete
SVG and example inputs through both wgpu APIs and both owned native renderers.
See [commands and evidence contract](docs/native/m4-corpus-runner.md). It shares
frozen scene/font resources, uses exact premultiplied RGBA8 comparison and rejects
incomplete output catalogs. This complements the focused kernel/lifecycle tests;
a successful minimum probe is not complete corpus acceptance.

The 2026-09-19 [M4 closeout](docs/native/m4-completion.md) records the complete
1,712 SVG / 45 example six-route pass, device scope and immutable evidence hashes.

## Windows M3 minimum native programs

The runtime now lives in `src/native/runtime/` with named `.rs` roots and separate
DX12/Vulkan directories. The current native test entrypoint is a library filter:

```powershell
cargo test --release --features native --lib native::runtime -- --include-ignored --test-threads=1 --nocapture
```

Use the explicit compiler/GPU/validation-layer settings documented in
[M3 closeout](docs/native/m3-completion.md). `TILEINK_NATIVE_GPU_REPORT` optionally
writes the three-repetition four-route case manifest, hashes and exact differences.
Full wgpu SVG/example regressions remain separate from the minimum native programs.
No performance comparisons are required for this continuation per user instruction.


## M0/M1 delivery scope (2026-09-13)

On 2026-09-13 the user instructed: **“不用比较性能了”** (stop performance comparisons). For this M0/M1 closeout, no further Criterion comparisons, resize timing or telemetry are required. Performance is not an acceptance gate for this delivery, and `performance_accepted` remains false. Historical regressions, rejected experiments and incomplete timing runs remain preserved. This does not waive functionality, exact pixels, feature/dependency checks, release tests or code review.


This document records M1 invariants and reproduction instructions. Audited results and remaining gates are recorded in the
repository-root NATIVE_BACKEND_PROGRESS.md; commands here do not themselves claim
completed validation.

## Explicit WGPU unit-test devices

The renderer unit-test helpers (`new_test_renderer` and `render_native_wgpu`) accept `TILEINK_TEST_API=dx12` or `vulkan`. An explicit
request uses the existing physical-GPU selector in `examples/common/benchmark_gpu.rs`.
It requires the selector's `TILEINK_BENCH_GPU` identity and, on DX12, the pinned
`TILEINK_PARITY_DXCOMPILER` library. It rejects an unavailable API/device/capability
instead of returning to the default adapter. Device slots are separate for each
API and texture mode. Pin these environment settings for each test process; do not
change the GPU identity or compiler while a process owns cached devices.

`TILEINK_WGPU_MODE=native|portable` is still only the WGPU texture mode. These unit
tests do not use the separate native DX12 or Vulkan renderer. Without
`TILEINK_TEST_API`, the existing default device selection remains available.

The SVG/debug/coarse suites have other device constructors; this setting alone
does not certify their API coverage. Continue using the explicit reference runners
for full SVG/example comparisons.

Run the four CPU selector contract cases independently of GPU initialization:

```powershell
cargo test --release --package tileink --lib wgpu::renderer::tests::common::device_selection:: -- --test-threads=1
```

Run the actual uploader regressions in separate processes for each API/mode:

```powershell
$env:TILEINK_RUN_WGPU_TESTS = '1'
# Example from the recorded 2026-09-09 boot; rediscover the LUID after reboot.
$env:TILEINK_BENCH_GPU = '9f3f010000000000'
$env:TILEINK_PARITY_DXCOMPILER = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\dxcompiler.dll'
foreach ($testApi in @('vulkan', 'dx12')) {
    $env:TILEINK_TEST_API = $testApi
    foreach ($textureMode in @('native', 'portable')) {
        $env:TILEINK_WGPU_MODE = $textureMode
        cargo test --release --package tileink --lib wgpu::renderer::tests::image_atlas:: -- --test-threads=1 --nocapture
        if ($LASTEXITCODE -ne 0) { throw "Atlas regression failed for $testApi/$textureMode" }
    }
}
```

Atlas growth must preserve old pages without reuploading clean standalone images.
The three uploader cases include an explicit full-upload control. A RED result
must be a reproduced semantic assertion failure, not setup or compilation failure.

Some renderer tests explicitly construct a portable oracle regardless of the
requested texture mode. Their logs must record the actual constructor modes;
do not report such a test as exercising native textures merely from the process
environment. Pipeline-cache creation also owns a default-adapter constructor.
Use the strict reference runner for complete pinned API/texture-route coverage.

The helper logs physical identity, API, driver, requested capabilities and runtime
compiler provenance. Keep those logs and source/binary hashes with every result.
These focused tests do not replace full SVG/example comparisons or native four-way
pixel acceptance. Run Criterion uploads separately from tests and other GPU jobs;
its timing boundary and controls are documented in BENCHMARKS.md.


## Retained backdrop budget invariant

During a partial root frame, an old backdrop's unfiltered painter-order source
must survive until that backdrop is visited. The final root texture cannot
reconstruct this source from clean tiles. Cache insertions therefore protect only
unvisited old Backdrop entries using the frame-start LRU clock boundary; ordinary
surfaces and current-frame entries remain evictable. If only protected history
could pay for a new cache entry, discard the new entry while keeping the budget.
Dropping a Backdrop cache still invalidates the next root history.

Protection is chosen from the final root DamagePlan, remains independent of nested
filter worklist suspension, and ends on every finish_frame path, including failure.
Budget changes occur between partial frames. Full redraws use ordinary LRU. Clock
overflow rebases ordering and the protected boundary only at overflow, with no
per-frame traversal of existing cache entries.

The frame-lifecycle and clock-wrap regressions exercise CPU state transitions.
The nonuniform halo GPU regression below verifies the corresponding actual pixel
failure. CPU results alone do not certify GPU pixels or performance.

## Filter read domains and nested damage

Output geometry and input reads are separate contracts. Erode, convolution and
lighting read neighbouring pixels without enlarging output bounds. A nonpoint
Wrap convolution, including one inside Chain/Graph, reads the original complete
filter domain. Never redefine its period by passing cropped dirty bounds. A dirty
WholeRegion filter reconstructs its local source/output; a clean complete cache
remains reusable. Partial work that changes scan/coarse worklists owns distinct
GPU storage from root draws in the same submission.

Root tile bins are an acceleration structure, not a source-visibility contract.
Local compaction preserves nested filter input domains and supplements batches
outside the indexed query/canvas in painter order. Keep intermediate damage
outside the root until all enclosing filters have consumed it; constrain final
root damage afterward. No explicit clip is identity, whereas an empty explicit
clip remains empty through ancestor expansion. Large finite dependency expansion
uses saturating bounds arithmetic before clipping, never integer wraparound.

These requirements fix input loss and stale update root causes. CPU tests under
shared/layer/filter, render/filter_scene, render/filter_pass, Canvas damage and
materializer damage enforce them. The six WGPU filter_dependencies GPU cases must
actually run with pinned hardware on all required API/texture routes; compilation
or default-skipped GPU tests are not pixel evidence. Full SVG/examples/parity remain required. Scoped Backdrop dependencies follow the separate
execution-domain invariants below, including local-to-root propagation and
painter-order transitions.


## Scoped Backdrop damage invariants (M1 candidate)

Resolved root damage events are separate from prunable node-state overlays. Each
scoped event is computed from real old/new local output around chunk rebuilding,
then contains only final root bounds and dirty Backdrop IDs. Root output must
never be replayed as local input. Independent successful renderer commits consume
these events; skipped materializer updates and state-page compaction do not lose
them. Event epochs are bounded to 256 steps. Missing/incomplete coverage requires
explicit history recovery, while each consecutive complete step remains partial.
A layer parameter transaction that switches between parent-target and isolated
input is an execution-domain transition even without hierarchy_changed.

Clip/ClipSdf coverage changes enter their child domain before Backdrop sampling.
Opacity/Blend use the same fusion predicate as command compilation; isolated
children cannot sample outside damage. Mask content and mask branches have
independent input damage. A Backdrop's own changed output precedes its children;
its children's changes cannot travel backwards to invalidate that same Backdrop.
Other group/filter/mask parameters affect their completed output. Layer chunks
contain empty shells, so parameter updates use the layer output domain rather
than the shell's empty visual child bounds. These are root-cause corrections.

Ordinary root Backdrops retain the spatial dependency index. Scope classification
is refreshed once with dependency metadata, not rescanned by each frame-patching
stage. Scoped bounds changes that rebuild frame metadata still publish complete
events; full redraw is not a replacement for this incremental path.

Validation remains candidate-specific: real CPU REDs precede each correction;
GPU RED/GREEN compare all RGBA bytes with ForceFull and independently assert
expected colors. Full SVG/examples and strict reference parity remain required before integration.
Criterion comparison is waived for this M0/M1 delivery; no performance acceptance is claimed.

## Same-frame backdrop input lifetime regression

A partial frame must retain each unvisited Backdrop source until its painter-order
execution consumes it. Cache pressure from an earlier growing filter can evict
ordinary reusable output, but cannot reconstruct a clean source halo from the
previous final image. The nonuniform halo regression places green input outside
the dirty tiles and yellow foreground after the backdrop at the same coordinates.
It compares the complete RGBA image against ForceFull and independently checks
that the green halo contributes at the dirty edge. A uniform blue backdrop is
insufficient because source-over can hide a missing transparent blur halo.

The isolated diagnostic counterfactual disables only same-frame cache protection;
it records actual eviction and the wrong blue edge `[0,0,255,255]` versus
`[0,82,173,255]` on Vulkan. Diagnostic logging is absent from this candidate.

## Shared frame and layer execution

The shared frame entry owns empty-damage handling, lazy prepared-plan access,
child preparation before root scan/clear, the existing first-quarter batch budget,
active root batch selection, direct/recursive choice and success-only history copy.
Adapter methods own resources and actual GPU operations. Empty damage still copies
valid history when requested without preparing pipelines or cloning a plan. Errors
stop dependent target work; execution/copy errors still publish diagnostic dispatch
counts, without publishing successful output/history.

Layer dispatch interprets Isolate/Opacity/Blend/Filter/Backdrop once in the shared
executor. Both fused clip variants are invalid offscreen operations and are rejected
before children or resource cursors are consumed. CPU contract tests exercise the
production shared entry points, including stage order, failure propagation, painter
order, active selection and early-submit exclusions. Native GPU conformance remains
a later adapter gate.

## Structural edits inside Backdrop input domains

Hierarchy edits must publish complete dependency damage when the scoped domain
continues. Resolve removed/reparented commands against old chunks, hierarchy and
painter order before mutation; resolve inserted/reparented commands against the
new tree afterward. Combine only final root bounds and dirty cache identities.
Groups contribute their affected descendant chunks. Replaying old root output as
new local input would apply outer filter transforms twice. This fixes unnecessary
DamageHistoryUnavailable recovery for ordinary deletion and reinsertion; actual
journal gaps, invalidation and unsupported domain transitions still recover.

The GPU regression requires partial frames and exact ForceFull pixels across
root, Filter, Isolate and Mask domains. CPU cases also cover off-canvas local
input, later root Backdrops and renderer cursors that skip structural updates.

## Shared scene preparation

`render::prepare` owns per-renderer outer-plan fingerprint and stack-depth metadata.
An exact fingerprint or retained structural-reuse certificate may keep the active
cached plan. A values-patched plan consumes Canvas's current compiled plan while
reusing only size metadata. Without a cached plan, neither certificate authorizes
metadata reuse. Filter-resource changes still refresh their tables when topology
is reused. A local scratch execution does not replace the outer preparation key.

The same module selects new text preparation, retained range updates, or immediate
reconciliation. `Some(empty changes)` from flat reconciliation preserves an incremental no-op;
`None` from flat reconciliation requests a full text refresh. Retained updates
continue to use Canvas's explicit ranges when the helper returns `None`. GPU resources, target sizing,
uploads and temporary scene-resource swaps remain adapter responsibilities.

This extraction closes the M1 preparation boundary. It does not implement native
GPU operations. Contract tests exercise missing plans, exact/structural reuse,
descriptor-value refresh, filter invalidation, independent state and text lifecycle.

## Shared filter programs

`render::filter_program` owns Chain/Graph traversal, input and SourceAlpha resolution,
primitive regions, cursor consumption, scratch lifetimes, full/partial blur passes,
downsampled worklists and glass composition strategy. The statically dispatched
adapter encodes typed `FilterKernel` operations; texture views, binding/uniform
lowering, snapshots and command buffers stay in WGPU. Kernel encoding must return
failure when required resources are unavailable, including pointwise operations.

Every graph output must be cleared before publication, and every acquired scratch
slot must be released on failed input resolution or recording. These fix existing
root causes: ignored clears/pointwise failures and a Merge invalid-edge leak. A
failed kernel stops later kernels and cannot publish cached output. Partial blur
restores the exact output worklist after its vertical halo; glass restores the
suspended incremental state on both success and failure. Downsampled sampled and
materialized glass share one scratch/worklist lifetime and differ only at the
upsample/composite tail. No additional submit, wait or pixel tolerance is introduced.

CPU contracts inject each recording/allocation failure and check order, SourceAlpha
reuse, cleanup, low-resolution work and halo restoration. The concrete WGPU regression
removes the pointwise pipeline and verifies rejection through both filter and opacity
group entries. GPU test enablement and full corpus parity still apply; the M0/M1 performance comparison waiver above remains in force.

## Permanent scoped correctness example

`scoped_benchmark_pixels` (features `wgpu,bench-internals`) is a non-timing
regression tool for scoped filter/backdrop updates. It compares successive
states with ForceFull and an independent immediate oracle, checks every RGBA
byte and asserts analytic colors. It is intentionally retained despite removal
from an earlier candidate inventory. This tool does not collect performance
measurements.
