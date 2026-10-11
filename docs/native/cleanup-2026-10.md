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

## Phase 8: resolve resource brushes from immutable source records

Retained transform updates change draw records without changing their local brush
payloads. Resource-brush packing previously reinterpreted the already patched GPU
header as a CPU record: its atlas coordinates occupied the original payload
offset/length fields. This silently cleared the staging brush. Later SDF arena
growth could copy that corrupted header into a repacked paint buffer, making an
otherwise unchanged image disappear.

Resource header resolution now takes an immutable CPU source and a separate GPU
destination. Sparse updates, shared draw records and relocated native shader
brush packing all use the same operation. Fresh filter brushes resolve their
header before overwriting their newly encoded record. This fixes the underlying
format ownership error without copying all brush data on transform-only frames.

Malformed private records no longer silently turn transparent, and draw ranges
already clipped by the retained arena journal are consumed directly. Genuinely
absent or empty images keep their specified transparent sampling behavior.
Host packing and actual retained GPU regressions both fail before the fix and
pass afterward. Additional semantic coverage exercises atlas/texture placements,
repeated shared draws, relocated records and missing images inserted later.
Production packing and its tests now live in separate modules.

All 8 paint-packing, 3 resource-brush, 8 public native-renderer, 8 native shader
packing/pipeline and 98 retained-scene focused release tests pass. The full
single-thread release suite reports 962 passing tests and the same nine known
failures. Release Clippy and example builds/tests pass without new warnings.
All 1730 SVG images and seven example PNGs remain byte-identical to the initial
cleanup baseline.

The benchmark now includes 19 paired real-renderer workloads, adding sparse and
dense resource-image translations. All preserve exact baseline pixels; image
translations also match fresh renderers. Paired p50 changes range from -0.57%
to +0.69%, with no material measured regression. A twentieth workload repeatedly
replaces paths with SDF geometry while moving image nodes: corrected output stays
equal to fresh rendering, and the original corruption reproduces. Its corrected
p50 is 0.435 ms; no before/after speed claim is made for incorrect baseline pixels.
Temporary measurement source is removed after validation.
[Raw measurements and protocol](benchmarks/cleanup-phase8-metal-2026-10-11.json).

## Phase 9: bind completed vector textures directly

Standalone vector images previously uploaded a full-size transparent CPU image,
copied the completed child rendering into it, then retained another native
snapshot. Image allocation now delays the texture table until children are ready
and binds each standalone child output directly. Repeated keys can share the same
child resource and cached native owner. GPU image retention clones an existing
native lease; no second allocation or copy is required for that output.

Atlas placements still copy their center and eight padded border regions, which
preserves clamp sampling without neighbor bleed. Their planner now returns the
fixed nine-region array directly; the obsolete standalone-copy branch and default
region filling are removed. Unused shader table slots retain their required
initialized one-pixel transparent binding.

The existing immutable-cache contract forbids writing an image after retention.
Native owners pin pooled storage, submitted batches retain their resources, and
cache acceptance still occurs only after successful submission. Tests exercise
queued readbacks, lease reuse, alias keys and an abandoned batch that must rebuild
its cache. Vector output dimensions, batch ownership, array shape and RGBA8 format
remain validated. The explicit format check preserves the rejection previously
provided by the deleted copy operation.

The direct-binding and native-retention regressions fail before the optimization
and pass afterward. Mixed raster/vector placement and border tests verify the
remaining atlas path. This removes the redundant work itself; it does not replace
it with a fallback allocation.

All 11 image-assembly, 4 native image-lifetime and 6 vector-planning focused
release tests pass. The full single-thread release suite reports 965 passing
tests and the same nine known failures. Release Clippy and example builds/tests
pass without new warnings. All 1730 SVG images and seven example PNGs remain
byte-identical to the initial cleanup baseline.

DX12 and Vulkan library/unit-test code also pass Windows Rust type checking using
the temporary artifact-free harness described in phase 2. The harness is removed;
this does not verify Windows GPU execution or shader compilation. Chinese and
English image guides describe direct vector texture binding. The website build
remains unrun because dependencies are not installed.

The benchmark now covers 22 paired actual renderer workloads. The five new SVG
cases use the public lowering/rendering APIs and swap pre-lowered resource content
every frame, exercising image-cache rebuilds. All preserve exact pixels. On M5,
large vector patterns improve p50 by 9.7%, repeated SVG images by 23.3%, nested
images by 16.8%, and mixed placements by 5.6%. The small-atlas control changes
+0.66%; the 17 existing cases range from -1.36% to +1.55%, without a material
measured regression. SVG parsing/lowering and readback are outside timed regions.
Temporary measurement source is removed after validation.
[Raw measurements and protocol](benchmarks/cleanup-phase9-metal-2026-10-11.json).

## Phase 10: encoded descriptors have explicit semantics

CPU brush reconstruction no longer turns unknown extend, sampling, resource
scope or mesh-interpolation values into valid defaults. Pattern opacity must fit
its encoded byte, and a resource payload must contain exactly one three-word
image ID. SDF cap/side/visibility fields must equal their actual enum values;
fractional and nonfinite values are rejected instead of truncated or thresholded.
The SDF reconstruction code now lives in a separate decoding module.

Canvas record access distinguishes an absent payload (`DrawRecord::NONE`) from
a present malformed record. A present record must decode; append and public
brush lookup can no longer quietly remove corrupted paint or geometry. This
matches the existing offscreen reconstruction contract. Renderer-owned headers
also reject an invalid resource scope rather than aliasing it to another resource.

Regression tests reproduce the old unknown-value substitutions, ignored trailing
resource words, and all three silent append omissions before their fixes. Valid
descriptor tests retain both resource scopes, every extend/sampling combination,
edge opacity values, all SDF cap/side/visibility values and both mesh interpolation
modes. These changes fix the descriptor and caller contracts directly; they do
not introduce a compatibility or temporary rendering path. Genuine missing-image
transparency, intentionally absent geometry, and floating-point geometry fields
retain their existing semantics.

All 9 brush, 3 SDF, 8 offscreen, 77 Canvas and 8 public native-renderer focused
release tests pass. The full single-thread release suite reports 972 passing
tests and the same nine known failures. Release Clippy and example builds/tests
pass without new warnings. All 1730 SVG images and seven example PNGs remain
byte-identical to the initial cleanup baseline.

The 24 paired actual renderer workloads preserve exact pixels. The two new cases
include reset/append of a reusable child containing line/arc/callout variants,
their shadows, and renderer/scene image patterns. On M5, append plus render changes
p50 by +0.68%; the same content through cropped isolated blur changes -0.34%.
The 17 established general/retained cases range from -0.89% to +0.71%, with no
material measured regression. The vector-image gains from phase 9 remain present.
Temporary measurement source is removed after validation.
[Raw measurements and protocol](benchmarks/cleanup-phase10-metal-2026-10-11.json).

## Phase 11: remove the retired WGSL verification chain

Two macOS runners still invoked deleted Cargo test targets. Their mock-cargo
test checked arguments without checking whether those targets existed, and the
Metal SDF test required archived output from the deleted WGSL generator. The
runners, orphaned WGSL sources and archive-dependent SDF test are now removed.
The current native test runner preserves the Cargo/test-harness argument boundary
and forces one harness thread and one Rayon worker. Regression tests first
reproduced the missing targets, duplicate separator and inherited parallelism;
they now verify current Cargo metadata, exact arguments, exit status and failure
diagnostics.

SDF verification now dispatches the current native probe through `ComputeBatch`
on each supported backend. It preserves the 672 exact identity rectangle/circle
oracle samples, adds independent authored-geometry f64 references under affine
transforms, and tests all 19 encoded kinds through 50 typed variants. Physical
coordinate changes, degenerate line caps, full/empty arcs, hidden callout bodies,
strokes/shadows and periodic dash phases provide additional semantic checks.
General affine comparisons permit one 8-bit coverage code at floating-point
rounding boundaries; the original exact oracle remains exact. Repeated submission
and trailing sentinels verify readback stability and logical-count bounds.
Validation request stride and affine offset come from the canonical shader header.
Unused Metal-only test helpers and obsolete runtime references are also removed.

All 5 runner, 4 constant-layout and 7 native SDF focused release tests pass; the
actual shell entrypoint also runs all 7 SDF tests successfully. The full
single-thread release suite reports 983 passing tests and the eight remaining
known failures, with no new failures. Release Clippy and example builds/tests
pass without new warnings. All 1730 SVG images and seven example PNGs remain
byte-identical to the initial cleanup baseline.

DX12 and Vulkan library/unit-test code pass Windows Rust type checking with the
temporary artifact-free harness, which is removed afterward. This does not
verify Windows GPU execution or shader compilation. This batch permanently
replaces the broken verification chain; it does not change application rendering
operations or make a performance improvement claim.

## Phase 12: remove nearest-sampling glyph expectations

Five native coarse tests still computed glyph candidates using the nearest-
sampling bounds that preceded fractional glyph motion. The current Metal and
HLSL shaders deliberately expand bitmap candidate bounds by one local pixel for
bilinear sampling. Consequently the glyph at x=16 belongs in the first tile's
candidate list; the old tests expected only two of the three authored glyphs.
The renderer behavior is correct, and no shader or production rendering logic
changes in this batch.

The obsolete expectations are replaced with the current sampling contract across
count, scalar/parallel emission, paged lists and the chunk count/offset/emission
chain. Tests retain exact stream ordering, sentinels, truncated logical ranges
and physical capacity guards. Additional positive/negative fractional positions
and strict contact with the expanded boundary distinguish actual candidate
overlap from boundary-only contact. Typed glyph/run/image metadata replaces
duplicated raw text fixtures, and glyph page fixtures live in a separate module.
A focused host regression first reproduced the two-versus-three discrepancy.

All 29 Metal coarse focused release tests pass. The full single-thread release
suite reports 989 passing tests and three remaining known failures: the two
tests requiring an explicitly pinned DXC executable, and the 1600-pixel canonical
turbulence comparison. The five coarse failures are resolved without dropping
their assertions. Release Clippy and example builds/tests pass without new
warnings; all 1730 SVG images and seven example PNGs remain byte-identical to the
initial cleanup baseline. This permanently corrects stale test semantics; it
does not change application execution or claim a performance improvement.

## Phase 13: validate retained hierarchy once

An empty child list previously ignored an explicit `before` sibling and silently
appended. Scene leaves also accepted retained children even though materialization
treats their immutable Canvas as a leaf and never renders those children. Invalid
mask-branch errors reported the target's parent (or invented owner zero at the
root), and moving a missing node before itself returned success.

The transaction model now has one child-branch contract: groups and layers own
content children, only mask layers own mask children, and Scene leaves own neither.
An explicit sibling must exist even in an empty branch. Self-reorder validates
node existence first, while remaining a no-op for existing nodes. Errors identify
the actual requested parent. Four regressions reproduced the old failures before
the fixes and now cover root/group/mask branches, both leaf branches, insert and
reparent rejection, rollback, order keys and subsequent valid operations.

With that invariant enforced before publishing transactions, command-container
and sibling traversal no longer treat impossible missing ancestors, branches or
order keys as an empty result or a reason to recompile. Actual structural changes
that need a different compiled plan retain their required reconciliation logic.
Transaction application moves out of scene storage/rollback into its own module;
36 existing tests move unchanged into focused modules instead of one 2088-line
file. This fixes the hierarchy contract directly and removes its downstream
fallbacks permanently, without a compatibility or temporary implementation.

All 102 retained focused release tests pass. The full single-thread release suite
reports 993 passing tests and the same three known failures described in phase 12.
Release Clippy and example builds/tests pass without new warnings. All 1730 SVG
images and seven example PNGs remain byte-identical to the cleanup baseline.
DX12 and Vulkan library and test types pass Windows Rust type checking using a
temporary artifact-free harness, removed afterward; this does not verify Windows
GPU execution or shader compilation. Chinese and English retained API guides
document the validated branches and sibling rules. The website build remains
unrun because dependencies are not installed.

Seven paired actual retained-renderer workloads compare against the immediate
parent commit `8c030b29`, including sparse/dense translation, opacity, resize,
sibling reorder with order-key rebalance, insertion/removal, and reparenting.
Every case preserves exact initial, first/second mutation and final pixels. On
M5, p50 changes +0.23% to +1.57% (0.69 to 3.25 microseconds), without a material
measured regression or a speedup claim. Temporary measurement source is removed.
[Raw measurements and protocol](benchmarks/cleanup-phase13-metal-2026-10-11.json).

## Remaining audit work

- Continue auditing retained-scene reconciliation, SVG/text semantics, native resource lifetime boundaries, shaders, examples and scripts. Necessary SVG paint/font behavior, degenerate geometry semantics and unsafe GPU ownership/bounds validation must remain correct; a keyword match alone does not establish an obsolete implementation.
