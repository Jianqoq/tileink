# Native implementation cleanup — October 2026

This audit is ongoing. Phase 1 removes obsolete scheduler architecture and silent substitutes; it does not claim that every repository module has been audited.

## Removed in phase 1

- The test-only `CommandBatch`, `BatchAdapter`, uniform arenas, resource readiness ledger, early-root submission policy and portable root ping-pong executor. The native renderer never used these models. Conformance tests now exercise production `ComputeBatch` submissions, immutable per-dispatch uniforms, rejected work and receipt ownership.
- The generic frame adapter with no-op preparation methods, impossible submission-policy branches and one-use `Option` state. Native frame recording now executes directly in its caller-owned batch. Output routing and synchronization retain their actual ownership boundaries.
- Boolean/optional filter recording status and its out-of-band native error latch. `Result` preserves the original graph/allocation/recording error. Scratch helpers release every acquired intermediate on recording and cleanup failures, including unpublished graph outputs. This fixes the actual lost-error issue; it is not a temporary bridge.
- Offscreen reconstruction from old draw lists when a selected batch was absent. Selection now represents every requested batch, including empty batches, and shares one membership map. Geometry translation no longer ignores invalid internal indices or replaces malformed encoded brushes with transparent draws. Geometry, selection and tests have separate modules.
- The `TILEINK_DXC_PATH` compatibility alias. Build-time and runtime compiler selection use `TILEINK_NATIVE_DXC_PATH` consistently.

## Validation

Focused release tests pass. The broader single-thread Metal release suite reports 956 passing tests and eight previously observed failures: six Metal reference/particle oracle tests and two tests requiring unavailable DXC. The previously failing scaled-turbulence test passed this run; this cleanup does not claim to have fixed it. Obsolete model tests were removed with their implementations, and meaningful ordering, partial-frame, error and ownership coverage was moved to the current paths.

All 1730 SVG render outputs and seven rendering examples are byte-identical to commit `2ea5d59c`. The Metal window presentation smoke test passes. Release Clippy, example builds and the backend-free check pass; existing unrelated lint warnings remain. DX12/Vulkan GPU execution is not available on this host.

## Real renderer performance

Apple M5, pinned registry `0000000100000575`, release mode, one CPU thread, validation disabled. Baseline commit `2ea5d59c` and the cleanup renderer are linked into the same process. Each workload warms 100 paired frames and measures 600 frames per version, alternating execution order for every pair. Timings include CPU recording, submission and explicit GPU completion; compilation and readback are excluded. Each scene also requires exact baseline/cleanup pixel equality.

The measured p50 changes range from -1.33% to +0.76%. These small variations do not establish a speedup or a material slowdown. The purpose of this phase is simpler ownership and accurate errors while preserving rendering cost.

| Workload | Before p50 ms | After p50 ms | Change | Before p95 ms | After p95 ms |
|---|---:|---:|---:|---:|---:|
| ordinary | 0.724 | 0.715 | -1.33% | 0.797 | 0.786 |
| blur_small | 1.162 | 1.161 | -0.10% | 1.247 | 1.259 |
| blur_full | 4.116 | 4.120 | +0.11% | 4.939 | 4.953 |
| blur_4k | 29.230 | 29.222 | -0.03% | 30.492 | 30.460 |
| glass_small | 0.955 | 0.960 | +0.54% | 1.026 | 1.022 |
| glass_full | 1.805 | 1.806 | +0.07% | 2.626 | 2.621 |
| progressive | 2.317 | 2.322 | +0.18% | 3.189 | 3.199 |
| shadow | 3.943 | 3.973 | +0.76% | 4.829 | 4.844 |
| color_chain | 1.619 | 1.617 | -0.15% | 1.827 | 2.445 |
| graph | 5.151 | 5.179 | +0.56% | 6.179 | 6.212 |
| custom_glass | 0.816 | 0.812 | -0.44% | 0.931 | 0.932 |
| isolated_blur | 0.952 | 0.952 | +0.06% | 1.029 | 1.029 |
| isolated_path_blur | 1.141 | 1.142 | +0.05% | 1.216 | 1.222 |

[Raw measurements and protocol](benchmarks/cleanup-phase1-metal-2026-10-11.json). Temporary measurement drivers are removed after validation.

## Phase 2: one native submission path

DX12 and Vulkan verification programs now record the same `ComputeBatch` resources,
per-entry binding layouts, pipeline cache and frame used by the renderer. The old
fixed-layout probe pipelines, per-probe native frames, texture upload helpers and
`Work::Probes` submission variants are deleted. Pending submissions own a compute
frame directly; Vulkan fences no longer pass through an optional/error conversion
that could omit a pending fence during teardown. This removes the duplicate
implementation, rather than retaining a compatibility bridge.

HLSL probe artifacts carry their actual reflected per-entry layouts, like Metal
and every other shader family. Only requested compute entries create pipelines.
Resource/device limit validation, uncertain-submission quarantine and original
GPU ownership checks remain required. Vulkan resource-limit tests now target
production compute descriptors, including uniform ranges and launch dimensions.
The driver-cache header check remains with the compute pipeline cache and tests
reject truncated, wrong-device and wrong-UUID data.

Shared probe oracles cover zero counts, workgroup tails, layouts, buffer and
texture sampling, mixed dispatches, uniform-free scatter and ordered readbacks.
They are defined once and run through actual compute recording. Focused Metal
release tests pass. The final full release suite reports 959 passing tests and
nine previously observed failures: six Metal reference/particle oracles, two DXC
tests and the intermittent scaled turbulence test. There are no new failures.
All 1730 SVG images and seven example images match `2ea5d59c` byte for byte.
Release Clippy passes without new warnings.

DX12 and Vulkan Rust libraries and unit-test code were type-checked separately
for Windows. A temporary harness supplied an empty shader artifact table solely
for Rust type checking, because DXC is unavailable here. This does **not** validate
DXIL/SPIR-V compilation or GPU execution; those checks require the native GPU
matrix. The harness is removed after verification.

The same 13 paired real-renderer workloads compare the cumulative cleanup with
`2ea5d59c`: p50 changes range from -0.33% to +1.57%, with exact pixels in every
case. This does not establish a speedup or material regression. The Metal renderer
execution path is unchanged in phase 2; probe-only allocations no longer upload
unused source buffers. [Raw measurements and protocol](benchmarks/cleanup-phase2-metal-2026-10-11.json).

## Phase 3: explicit scratch extraction ownership

`SurfaceAdapter::take_scratch` now returns the occupied surface directly, matching
its scheduler ownership contract. The native adapter no longer turns a slot error
into `None`, and group/mask caching no longer silently disappears behind two
optional results. Filters and backdrops use the same extraction contract. An
attempt to extract a released slot identifies an internal scheduler bug instead
of concealing it as a cache miss. This fixes the silent error conversion itself;
it is not a temporary fallback.

The regression test failed before the change and passes afterward. All 241
focused layer scheduling tests pass. The complete single-thread release suite
reports 960 passing tests and the same nine known failures described in phase 2.
All 1730 SVG outputs and seven example images remain byte-identical to the initial
cleanup baseline. Release Clippy passes without new warnings.

The 13 paired real-renderer workloads again preserve exact pixels; p50 changes
range from -0.84% to +0.73%. These measurements do not establish a material
regression or speedup. [Raw measurements and protocol](benchmarks/cleanup-phase3-metal-2026-10-11.json).

## Phase 4: native surface allocation has one storage contract

Root and scratch surfaces now require the renderer's native resource pool.
Missing pools return an error instead of allocating and uploading zero-filled
CPU images. Progressive blur levels and custom brush outputs use that same
required allocator; shader-owned buffers always use the native shader resource
pool. The contextless CPU alternatives are deleted, rather than kept as another
implementation to maintain.

A retained surface always owns a `NativeTexture`. Frame targets must import
persistent storage, and installing a retained surface validates its logical
device before importing it into the new batch. Same-device cross-batch history
remains supported. Standalone transfer batches still support explicit CPU
uploads, native imports and readbacks; these are actual transfer operations.

The missing-pool regression failed before the change and passes afterward.
Allocation tests now exercise actual native textures and GPU readbacks rather
than inspecting surrogate CPU pixels. Coverage includes root/scratch clear
colors, sibling isolation, retained ownership, cross-batch imports, foreign-device
rejection, resize capacity, logical bounds, shader sampling and progressive blur.
The release suite reports 963 passing tests and the same nine known failures;
two tests initially lacked their separately named GPU environment variable and
passed after correcting the runner environment. All 1730 SVG outputs and seven
example images match the initial cleanup baseline exactly. Example tests and
release Clippy pass without new warnings. This removes the allocation fallback
itself and is not a temporary bridge.

The same 13 paired real-renderer workloads preserve exact pixels, with p50
changes from -0.25% to +1.16%. These small variations do not establish a material
regression or speedup. [Raw measurements and protocol](benchmarks/cleanup-phase4-metal-2026-10-11.json).

## Phase 5: cached resources require native storage

Cached scene buffers and the renderer image cache now require the native resource
pool. A missing pool returns an error before recording resources; it no longer
selects CPU buffer snapshots or bypasses image retention. The unused uncached
vector-image wrapper is removed. This removes the alternative implementations
at their source, rather than adding a temporary compatibility path.

Full snapshots after allocation growth or an abandoned submission remain required:
dirty ranges can depend only on GPU contents from an accepted upload. Tests now
exercise persistent native storage, actual submissions and GPU readback, including
growth, abandoned work, unchanged data, sparse updates, dispatch ordering and
retained scene identity. Both missing-pool regressions failed before the change
and pass afterward. The complete single-thread release suite reports 967 passing
tests and the same nine known failures. Focused cache, scene, scan, cumsum and
persistent-image tests pass; example builds, example tests and release Clippy
pass without new warnings.

All 1730 SVG outputs and seven example PNGs match the initial cleanup baseline
exactly. The 13 paired real-renderer workloads also preserve exact pixels; p50
changes range from -0.32% to +1.53%. These variations do not establish a material
regression or speedup. [Raw measurements and protocol](benchmarks/cleanup-phase5-metal-2026-10-11.json).

## Phase 6: remove disconnected diagnostics and preparation models

The CPU profiler's session owner was compiled only for tests. Production scopes
therefore never recorded timings, but retained unused TLS access and closure
wrappers throughout preparation, scheduling and retained materialization. Those
hooks, their test-only session model, the private clock and its sole `web-sys`
dependency are removed. This removes the disconnected implementation itself;
it does not introduce a substitute timer.

The old public debug capture/options types had no native renderer consumer or
producer. They and the English guide's calls to absent profiling/debug methods
are removed. The guide now documents API validation, explicit image readback and
incremental statistics, matching the existing Chinese guide. Active tile capture
continues to follow the explicit incremental configuration, with a semantic test
covering both settings.

Preparation also contained test-only scratch/upload decisions and a second
prepared-scene identity model. Those are deleted along with the unused filter
resource change signal and its classification work. Current plan/depth reuse,
native scene buffers, text reconciliation and filter resources retain their
production tests. Group execution shares scratch acquisition and child recording
while preserving cached mask ownership and error cleanup.

The final single-thread release suite reports 957 passing tests and the same nine
known failures. The count removes obsolete model tests and adds the active-tile
configuration test. All 231 scheduling, 98 retained-scene and 19 native renderer
focused tests pass. All 1730 SVG outputs and seven example PNGs remain byte-identical
to the initial cleanup baseline. Example tests, backend-free checking and release
Clippy pass without new warnings. The website text and MDX references were checked
against the current Rust API; a website build was not run because its dependencies
are not installed.

The same-process benchmark now covers 17 actual renderer workloads, including
retained sparse/dense translations, group opacity updates and continuous resize.
Every case preserves exact pixels, and p50 changes range from -0.76% to +0.70%,
with no material measured regression.
[Raw measurements and protocol](benchmarks/cleanup-phase6-metal-2026-10-11.json).

## Phase 7: fixed layouts do not need missing-value substitutes

LCD rasterization always merges exactly three channel masks. Their bounds now
use that fixed shape directly, without defaults for an impossible empty channel
collection. Brush and SDF decoders also extract their validated fixed parameter
arrays directly, removing impossible fallible length conversions and the private
partial-parameter zero-fill helper.

Actual record bounds, payload lengths, unknown shape kinds and descriptor
validation remain enforced. New semantic tests cover nontrivial gradient
parameters, truncated headers/payloads, invalid variable color counts, unknown
SDF kinds and exact floating-point bit preservation. Both tests pass before and
after the refactor. The brush and SDF tests now live in separate test modules,
keeping production files smaller. This removes redundant branches, rather than
masking a malformed-data error with a temporary substitute.

All 14 brush, 2 SDF, 38 text and 8 localized-offscreen focused release tests pass.
The complete single-thread suite reports 959 passing tests and the same nine known
failures. All 1730 SVG images and seven example PNGs remain byte-identical to the
initial cleanup baseline. Example tests and release Clippy pass without new warnings.
The same 17 paired actual renderer workloads preserve exact pixels, with p50 changes
from -0.97% to +0.59%. These variations do not establish a material regression
or speedup. [Raw measurements and protocol](benchmarks/cleanup-phase7-metal-2026-10-11.json).

## Remaining audit work

- Standalone vector-image placements still upload a zero-filled placeholder before
  copying the completed child image. Trace direct native retention and lifetime
  semantics before removing that upload and copy.
- Continue auditing retained-scene reconciliation, SVG/text semantics, native resource lifetime boundaries, shaders, examples and scripts. Necessary SVG paint/font behavior, degenerate geometry semantics and unsafe GPU ownership/bounds validation must remain correct; a keyword match alone does not establish an obsolete implementation.
