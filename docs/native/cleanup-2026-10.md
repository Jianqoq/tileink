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

## Phase 14: remove impossible retained-storage states

Every retained chunk allocates a text-run slot, including empty content. The slot
is now required rather than wrapped in an `Option` with unused zero-base defaults.
A live glyph allocation uses its existing arena instead of attempting to recreate
one. Chunk dependency refresh reads its known live chunk directly. The image
reference index stores counts only; the published canvas already owns each image
source, so the extra never-read source and its repeated clones are removed.
Arena allocations and registered chunk resources are released exactly once.
Missing ownership now fails at the invariant rather than silently returning an
unused boolean or skipping a resource release. Empty allocations, optional glyph
storage, shared-image references and inactive dirty GPU slots keep their actual
semantics. Chunk encoding/remapping moves out of lifecycle orchestration into a
separate module.

The new text lifecycle test exposed two existing rendering bugs. CPU glyph-list
capacity still used nearest-sampling bounds, whereas Metal and HLSL already include
one local texel around the bitmap for bilinear sampling. Underallocation dropped
later glyphs or tiles. Both full and incremental capacity calculations now reserve
that same local footprint before applying the draw affine. Translation append also
moved path/glyph geometry and world bounds without moving their local bounds;
later retained or nested affine placement could then cull translated content.
Append now preserves both coordinate domains. These are direct contract fixes,
with no compatibility or temporary rendering implementation.

Six new regressions cover duplicate/missing ownership, empty/growing/cleared and
reinserted text, arena compaction, affine bilinear bounds, positive/negative and
fractional append offsets at two scales, and actual native text readback against
fresh immediate rendering with and without nested append. Ownership, candidate
bounds and both GPU/CPU placement failures were reproduced before their fixes.
Text length tests move to a focused module and load the bundled font explicitly
instead of passing without assertions when system font layout is unavailable.

All 206 relevant focused release tests pass. The complete single-thread release
suite reports 999 passing tests and the same three known failures described in
phase 12. Release Clippy has no new warnings; example builds and all 11 example
tests pass. All 1730 SVG images and seven example PNGs remain byte-identical to
the cleanup baseline.

DX12 and Vulkan library and test types pass Windows Rust type checking using a
temporary artifact-free harness, removed afterward. This checks Rust types only;
Windows GPU execution and shader compilation are not verified.

Twenty-four paired actual renderer workloads compare against the immediate
parent commit `03359fec`, with 100 warmup pairs and 600 samples per version.
They cover general filters, retained mutations, path/source revisions, shared
vector resources and mixed SDF/pattern append. Every comparable case preserves
exact pixels. On M5, p50 changes -1.59% to +1.51%, with no material measured
regression. The added nested-text revision workload reproduces the old rendering
error and now matches an independently recorded immediate reference; its corrected
p50 is 0.249 ms. Its incorrect baseline is excluded from timing comparisons.
Temporary measurement source is removed after validation.
[Raw measurements and protocol](benchmarks/cleanup-phase14-metal-2026-10-11.json).

## Phase 15: remove retired shader-instance modes and binding defaults

Native brush instances no longer carry the input mode and sample radius from the
removed single-program filter implementation. Their CPU scene payload contains
only the uniform count, exact entry/source bytes and raw uniform words; affine
placement stays in the brush header. The internal type is now `BrushInstance`,
and its invocation accepts no unused input texture or compact-tile parameters.
Filters continue to use ordinary ShaderPipeline resources. The old payload is
not decoded through a compatibility branch. Source and entry validation now
belongs to the public kernel constructor once, and brush decoding reads each
record/header/payload once instead of repeating record validation through helpers.

Draw tags originate from the typed enum, including inactive retained records.
Unknown internal words no longer silently become ordinary brushes during CPU
allocation planning. Build-validated writable texture sizes map exactly to
RGBA8, RGBA16Float or RGBA32Float; unknown sizes no longer default to RGBA8.
Texture format conversion runs only for writable texture bindings, preserving
uniform sizes, sampler metadata and all other native resource kinds. Unused
fine/filter binding modules, an unused CPU mask, the blanket dead-code allowance
and commented-out geometry code are removed. Current reflected native interfaces
remain authoritative. These are permanent removals of dead modes and defaults.

Three regressions first reproduced the old brush payload, unknown-tag conversion
and unknown-format conversion. Five added tests cover those contracts, exact
single/both-backend and Unicode source bytes, empty/raw-bit uniforms, offsets,
truncated/malformed/retired payload rejection, all texture precisions and
nontexture/internal binding mappings. All 48 focused and 262 broader release
tests pass. The complete single-thread release suite reports 1004 passing tests
and the same three known failures described in phase 12. Clippy has no new
warnings; example builds and all 11 example tests pass. All 1730 SVG images and
seven example PNGs remain byte-identical to the cleanup baseline. DX12/Vulkan
Windows library and test types and the backend-free library compile successfully;
the temporary Windows harness checks Rust types, not shader compilation or GPU
execution.

Two complete paired Metal rounds cover 29 real workloads, including four native
procedural-brush scenes, append, shadow and retained mutation. Each round warms
100 pairs and records 600 samples per version with alternating execution order
and validation disabled. Initial, mutated and final pixels match exactly. Across
the combined 1200 samples per version, p50 changes range from -0.35% to +0.93%;
the four brush cases range from -0.12% to +0.34%. Retained resize changes from
+1.73% in the first round to +0.37% in the second. These small variations do not
establish a speedup or a material regression. Both rounds, scene definitions and
hashes are preserved in
[`cleanup-phase15-metal-2026-10-11.json`](benchmarks/cleanup-phase15-metal-2026-10-11.json).
An incomplete capacity diagnostic is excluded. Temporary measurement, snapshot,
validation and Windows typecheck sources were removed after recording results.

## Phase 16: represent retained placement once and reuse resolved paths

Scene chunks now store one optional `ScenePlacement` containing their source
Canvas and affine bits. Layers have no placement. The previous independent
options permitted combinations no producer created and required a redundant
presence check plus an unwrap in the transform-only path. Matching two placements
now directly checks source identity and changed affine bits, updating the stored
bits without another source clone. Full content replacement still re-encodes the
chunk. The single-use tuple-construction helper is removed.

Path backdrop allocation passes its already-resolved PathRecord to the geometry
bound calculation. The second lookup and impossible missing-index empty bound
are removed. Actual empty paths and layer paths without an ordinary draw in the
allocation range still use their correct geometry. An unrelated tautological
assertion in the touched path test is removed. These are permanent structural
simplifications; public APIs and supported rendering semantics are unchanged.

Three added semantic tests cover content replacement followed by repeated
transform-only edits, reuse of one node ID across Scene/Layer/Scene transitions,
and empty/fractional/partly offscreen path-only allocations. The content and path
tests also pass before this restructuring. All 22 placement-focused, one
path-focused, 79 Canvas and 107 retained release tests pass. The complete serial
release suite reports 1007 passing tests and the same three known failures.
Example builds and all 11 example tests pass; release Clippy has no new warnings
and the backend-free library compiles. All 1730 SVG images and seven example PNGs
remain byte-identical to phase 15.

Two paired Metal rounds measure 29 real workloads against commit `39a0c44d`,
with 100 warmup pairs and 600 alternating samples per version in each round.
All initial, mutated and final pixels match. Combined p50 changes range from
-0.28% to +1.86%; retained cases range from -0.16% to +0.82%, and mixed append
cases change by +0.08% and +0.24%. Four brush cases show small positive changes,
so six-case diagnostics reverse construction order without changing either
renderer artifact, then use the baseline renderer on both sides. Reversal changes
ordinary-scene p50 from +0.62%/+1.16% to -1.63% and reduces brush changes to
+0.26%..+0.96%. Same-version controls also show small timing differences. These
results show sensitivity to creation order; no speedup is claimed, and no
material regression is identified in the affected retained/append workloads.
Both complete rounds and both controls, including raw samples and artifact
hashes, are retained in
[`cleanup-phase16-metal-2026-10-11.json`](benchmarks/cleanup-phase16-metal-2026-10-11.json).
Temporary snapshot, measurement and validation sources were removed afterward.

## Phase 17: canonical glyph images and direct cache entries

Rasterization, prepared text and GPU upload now share one glyph-content type:
one-byte alpha, three-byte Harmony LCD and four-byte RGBA color. Swash renders
only alpha/color images; Harmony owns LCD generation directly. The retired
Swash RGBA LCD conversion, malformed-data copy/skip defaults, duplicate content
mapping, temporary pixel wrapper and redundant empty-image preparation branch
are removed. Image creation verifies the byte contract once with checked size
arithmetic. Invalid internal content or disabled LCD mode identifies an invariant
violation instead of silently dropping coverage. Valid empty masks remain empty,
and color font selection and RGB/BGR filtering retain their existing semantics.
This permanently removes the alternate image representation and silent defaults.

Glyph image and outline caches use direct entries instead of contains/insert/get
lookups. Unavailable glyph results remain cached, and explicit cache clearing
still invalidates prepared atlas generations. Outline conversion no longer
accepts an origin argument that every caller fixed at zero; actual per-glyph
translation and Y inversion remain. Text tests now load bundled fonts into an
empty database, require glyph/color data instead of returning before assertions,
and live in separate layout, recording, preparation, LCD and cache modules.

Six regressions first reproduced the old silent acceptance/skip behavior. Fourteen
added tests cover canonical byte layouts, retired format rejection, invalid native
data/content, RGB/BGR and disabled LCD modes, empty masks, checked byte-size
arithmetic, cache reuse/options, missing glyph results and outline translation.
The size-overflow regression caught an issue during review of the new constructor
and now passes. All 52 focused and 34 broader glyph release tests pass. The full
single-thread suite, including explicitly enabled GPU/compiler tests, reports
1021 passing tests and the same three known failures described in phase 12.
All 11 example tests pass, release Clippy has no new warnings and the backend-free
library compiles. All 1730 SVG images and seven example images match phase 16
exactly. No DX12/Vulkan GPU execution is claimed on this Metal host.

Two paired Metal rounds measure nine real workloads against `9b77dbca`, with
100 warmup pairs and 600 alternating samples per version in each round. Timings
include layout extraction and Canvas recording for the changing-text cases,
native submission and GPU completion. Initial, mutated and final pixels match;
retained nested text also matches independent immediate references. Combined p50
changes range from -2.77% to +0.35%. RGB/BGR/alpha layout and recording decrease
by 1.58%/1.95%/1.65%, path-outline recording by 2.77%, color text by 1.51%, and
cache invalidation by 0.98%. Cached text changes by -0.04%, retained mutations by
-0.56%, and the nontext control by +0.35%. Layout/path medians decrease in both
rounds; the small cached/retained/control differences do not establish a general
renderer speedup. No material regression is identified in these workloads.
Both complete rounds, raw samples, workload definitions and artifact/source
hashes are saved in
[`cleanup-phase17-metal-2026-10-11.json`](benchmarks/cleanup-phase17-metal-2026-10-11.json).
Temporary measurement, snapshot and validation source is removed after recording
the results.

## Phase 18: initialized window state, explicit completion and owned Git pipes

The native-window example constructs its complete State before entering the event
loop. Its impossible absent-state branches, unwraps and redundant smoke draw cap
are removed. Target preservation and `finish` are required host methods instead
of default policies. DX12 explicitly retires host copy/present fences and rendering
receipts; Vulkan shares its actual device/receipt retirement with resize. Normal
shutdown now propagates failures. Metal continues its existing explicit drain.
Minimized/out-of-date surfaces and ownership retention on unknown GPU completion
remain required native lifecycle semantics.

Unsupported platform/backend builds still compile, but executing the example
now returns an error instead of printing a requirement and exiting successfully.
Win32 client-size and resize failures propagate through the shared window API;
failed geometry queries no longer invent a zero-sized window. Frame insets are
computed separately and added to requested extents with wide arithmetic before
conversion to native signed dimensions, avoiding truncation and rectangle-size
overflow. The rectangle calculation follows the documented
[AdjustWindowRectEx contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-adjustwindowrectex).
These fix the actual error-reporting paths permanently.

The PNG comparison tool owns its required ChildStdin at batch-process creation.
Each blob read writes directly to that pipe instead of rechecking Child's optional
stdin. The spawn, I/O and Git protocol errors and process kill/wait ownership
remain explicit. No missing-pipe compatibility path is retained.

The featureless CLI regression first reproduced the old false-success exit.
A new real Git-stream test covers empty, binary, oversized-buffer, repeated and
missing objects followed by a valid read; it passes before and after the ownership
refactor. All eight focused release tests pass. Three additional Windows tests
cover invalid geometry handles, unrepresentable/frame-expanded extents and real
hidden-window resizing. DX12/Vulkan Windows library/example/test types compile
through a temporary type-only harness; its empty shader artifact table does not
validate shader compilation or GPU execution. The three Win32 tests still require
Windows runtime execution. The harness is removed after checking.

The full serial Metal release suite reports 1023 passing tests and the same three
known failures described in phase 12. All 12 example tests pass. Release Clippy
has no new warnings, and the backend-free library compiles. The real Metal host
completes eight frames and the 640x360 to 480x270 resize with validation enabled.
All 1730 SVG images and seven example PNGs remain exactly equal to phase 17.

Actual application measurements compare the before/after release executables,
alternating process order per pair with validation disabled. Six timed pairs of
the full 3442-PNG repository check produce identical PASS reports: p50 changes
from 1681.844 to 1687.511 ms (+0.34%). Ten timed pairs of native window startup,
eight frames, resize and shutdown change from 233.515 to 233.747 ms (+0.10%),
with identical successful completion reports. These whole-process measurements
do not establish GPU frame throughput or a speedup; no material regression is
identified in the affected applications. Warmup counts, raw samples and executable/
source hashes are saved in
[`cleanup-phase18-metal-2026-10-11.json`](benchmarks/cleanup-phase18-metal-2026-10-11.json).
Temporary measurement and validation source and copied executables are removed
after recording results.

## Phase 19: direct native resource contracts and shared constants

The native interface catalog reads common algorithm constants through the existing
shared constant cache. Per-family reparsing and constant-map plumbing are removed.
Texture-table consumers construct their resource directly instead of building a
complete validation interface and extracting one entry. Table capacity is parsed
once from its authoritative HLSLI declaration, including progressive blur, which
previously repeated the literal. Samplers share their actual zero-sized descriptor
constructor. The impossible empty built-in uniform default and the infallible
binding formatter's Result wrapping are removed. These are permanent internal
simplifications; application shader source, native ABI and public APIs are unchanged.

Two semantic tests cover every catalog family and globally unique entry, binding
order, unknown families, and the five production/validation texture-table consumers.
Both pass before and after the refactor. All 38 focused release tests pass. The
full serial Metal suite, including ignored GPU/compiler tests, reports 1025 passing
tests and the same three known failures described in phase 12. All 12 example
tests pass. Release Clippy has no new warnings and the backend-free library compiles.
Actual DXC compilation and DX12/Vulkan GPU execution remain unverified on this host.

A temporary catalog probe compares all 51 families and 84 entries: cache ABI bytes
and generated binding declarations are exactly equal to `ff1bef43`. The actual
application's Metal manifest is also unchanged. Running the complete before/after
release build scripts produces identical 40 Metal libraries, manifests, host/test
constants and declarations after normalizing only their output-directory paths.
The window example executables are byte-identical. All 1730 SVG images and seven
example PNGs match phase 18, and the validated Metal host completes eight frames
and the 640x360 to 480x270 resize successfully.

The real release build-script workload uses the complete warm-cache Metal catalog,
including compiler/SDK identity hashing, source expansion, cache validation and
artifact publication. With three warmup pairs and 12 alternating timed pairs,
p50 changes from 2585.442 to 1857.563 ms (-28.15%). A second round reverses the
starting order (two warmup/eight timed pairs): 2462.255 to 1837.507 ms (-25.37%).
The same-version control changes by +0.18%. Individual tool/hash/process timings
vary substantially, so these measurements do not isolate constant parsing or
represent total Cargo/rustc build time. No material build regression is observed.

Ten timed native-window pairs after two warmups change from 235.063 to 233.967 ms
(-0.47%), including startup, eight frames, resize and shutdown. Because both release
executables are byte-identical, no runtime speedup is claimed. Raw samples, controls,
ABI/compiled-output/executable hashes and the exact measurement scope are saved in
[`cleanup-phase19-metal-2026-10-11.json`](benchmarks/cleanup-phase19-metal-2026-10-11.json).
Temporary catalog probes, copied build sources, measurement scripts and presentation
executables are removed after validation.

## Phase 20: explicit cached-compiler errors and read-only SPIR-V buffers

The runtime DXC path no longer converts every cached-artifact validation error
into a cache miss and recompiles. Container, reflection subprocess, encoding and
ABI errors propagate to the caller. Truncated/checksum-invalid cache framing
still causes a genuine miss, and a fresh compile is validated before publication.
One interface description and work directory serve each invocation; the entry
comes directly from NativeSource. This permanently fixes the silent retry, with
no alternate shader or error suppression.

SPIR-V byte-buffer reflection now requires NonWritable on a read binding's own
variable or its sole buffer member. BufferBlock and uint-array layout alone
cannot distinguish ByteAddressBuffer from RWByteAddressBuffer. This follows the
[pinned DXC mapping](https://github.com/microsoft/DirectXShaderCompiler/blob/v1.8.2502/docs/SPIR-V.rst).
Read/write bindings still allow read-only optimization. The shared validator
enforces this contract for built-in and application kernels.

Three runtime regressions first reproduced the extra compilation after cached
process, encoding and ABI failures. A fourth covers source-key reuse/change,
truncated cache recovery and uncached compile failures. They execute the actual
runtime compiler/cache path with a controlled subprocess protocol; that fixture
does not compile HLSL or prove DXC conformance. Four SPIR-V tests cover absent,
correctly placed and unrelated NonWritable decorations, and read/write
optimization. The two rejecting cases failed before the fix. A catalog test
checks buffer permissions against all 51 production HLSL families. A separate
real-artifact Vulkan regression is added but requires execution on a Vulkan host.

All 36 focused serial release tests pass. The full Metal release suite, including
ignored GPU/compiler tests, reports 1034 passing tests and the same three known
failures described in phase 12. All 12 example tests pass; release Clippy adds no
warnings, and the backend-free library compiles. DX12/Vulkan Windows library,
example and test types pass through the temporary type-only harness. Actual DXC
compilation, Vulkan artifact execution and DX12/Vulkan GPU execution remain
unverified on this host.

All 1730 SVG images and seven example PNGs match phase 19 exactly. The validated
Metal window completes eight frames and the 640x360 to 480x270 resize. Its release
executable and the actual Metal shader manifest remain byte-identical. These
changes correct compilation/error semantics; no GPU performance improvement or
actual DXC performance measurement is claimed. Public shader error documentation
is updated in both website languages. The temporary type-check harness and copied
presentation executable are removed after verification.

## Phase 21: Vulkan driver errors, opaque cache data and explicit ownership

Vulkan cache construction no longer converts every driver error into a cold retry.
Cache-object and compute-pipeline failures propagate with their original result.
The host's repeated header/device/UUID parser is removed: the existing persistent
key already includes API, driver, device, UUID, shader and layout identity, and
[Vulkan handles incompatible initial cache data](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineCacheCreateInfo.html).
Disk framing/checksum validation remains shared with other native caches.

A cache hit creates the pipeline from the opaque bytes without exporting the
cache again. Cold creation exports once for publication. A small owned pair keeps
the cache and pipeline alive until publication succeeds, retires partial results
and releases both objects on creation/export/write failures. Pipeline construction
and cache ownership now live in separate short files. This permanently removes
the incorrect retry and manual compatibility policy.

Seven tests execute that production cache module and the real persistent-cache
wrapper through a controlled ash function table on every host. Before fixing,
five cases failed: cached cache-object errors, cached pipeline errors, redundant
hit export, host header policy and repeated creation after device-identity changes.
The tests also cover cold failures, partial native objects, publication failure,
source/device/driver/API/UUID key separation and zero live handles after retirement.
They verify calls and ownership without a Vulkan loader or GPU; they do not prove
driver conformance or actual Vulkan throughput. The obsolete header-policy unit
test is replaced by the current opaque-data contract.

All 23 focused serial release tests pass. The full Metal suite, including ignored
GPU/compiler tests, reports 1041 passing tests and the same three known failures
described in phase 12. All 12 example tests pass. Release Clippy has no new warnings;
the backend-free library compiles. Windows DX12/Vulkan library/example/test types
pass through the temporary type-only harness, which is removed after checking.
Actual DXC compilation and Vulkan/DX12 GPU runtime validation remain unavailable.

All 1730 SVG images and seven example PNGs match phase 20 exactly. The validated
Metal window completes eight frames and the 640x360 to 480x270 resize. Its executable
and shader manifest remain byte-identical. No actual Vulkan performance improvement
is claimed. The bilingual renderer docs now describe the error contract. The DXC
discovery doc's obsolete claim that runtime never needs DXC is corrected for
application-provided HLSL.

## Phase 22: direct sampler recording and one Vulkan diagnostic registration

Sampler recording only appends an immutable descriptor to the CPU batch. Both
the internal and public methods now return their resource handle directly,
removing the impossible `Result` and repeated error conversion at every caller.
`NativeShaderBatch::sampler` no longer requires `?`; native allocation failures
still propagate from submission. The native API guide and both website languages
document this permanent API simplification.

The two Vulkan diagnostic registration paths now share one constructor borrowing
the live message owner. The callback no longer tests the callback-data and
registered owner pointers for null: Vulkan supplies valid callback data and the
registered user pointer. The optional message check remains because
[device-address messages can omit text](https://docs.vulkan.org/refpages/latest/refpages/source/VkDebugUtilsMessengerCallbackDataEXT.html).
Poisoned-lock recovery retains diagnostics without unwinding through the native
callback; it is an FFI collection requirement, with a dedicated semantic test.

Four production-callback tests passed before and after the cleanup, covering UTF-8,
empty/absent text, ordering, independent owners and poisoned-lock collection. A
fifth verifies the shared registration's owner, filters and return contract. A
real Metal test passed before and after the sampler API change: nearest/linear
pixels match expectations and foreign batch sampler handles are rejected. These
are semantic preservation tests, not reproductions of a rendering defect. A
redundant clone in the phase-21 Vulkan cache test is also removed.

All 33 focused serial release tests pass. The full suite, including ignored tests,
reports 1047 passes and the same three known failures described in phase 12. The
12 example tests, backend-free check, formatting and release Clippy pass. Windows
DX12/Vulkan library/test/example type checks pass; actual DXC compilation and
Vulkan/DX12 GPU execution remain unavailable. The temporary type harness is
removed. Callback tests execute the production function without a Vulkan loader.

All 1730 SVG images and seven example PNGs match phase 21 exactly. The validated
Metal window completes eight frames and its 640x360 to 480x270 resize; the shader
manifest is unchanged. The actual application's alternating before/after
[whole-process measurement](benchmarks/cleanup-phase22-metal-2026-10-11.json)
has a median change of -0.09%, with two warmup and ten timed pairs. This does not
isolate sampler recording or establish a GPU throughput improvement. Temporary
measurement artifacts are removed; raw timings remain in the benchmark record.

## Phase 23: represent native buffer byte limits once

Persistent buffer construction already rejects zero, unaligned and above-u32
byte extents before native allocation. Its owned state now stores that extent as
`u32`, and upload packing accepts the same type. CPU allocation/indexing converts
it losslessly to `usize`. The upload no longer stores another destination-size
field, and scatter no longer declines an impossible above-u32 destination. The
private oversized-destination test is removed with that unreachable branch.
This permanently represents the existing addressing contract; public APIs and
the range-scatter shader wire format are unchanged.

Two new tests passed before and after the representation change. They exercise
the highest aligned destination bytes, adjacent-range coalescing, range overruns,
host addition overflow, and scatter descriptors/payloads near the address limit,
without allocating a multi-gigabyte destination. The real Metal persistent-buffer
test additionally verifies that invalid sizes report the original validation
error before allocating. Copy/scatter fragmentation thresholds, packed source
size checks, upload ordering, holes, ownership and queued readbacks remain covered.

All 24 focused serial release tests pass. The full suite including ignored tests
reports 1049 passes and the same three known failures described in phase 12.
All 12 example tests pass, as do the backend-free check, formatting and release
Clippy with no new warnings. After simplifying two test constants flagged by
Clippy, all ten upload-packing/scatter tests pass again. Windows DX12/Vulkan
library/test/example type checks pass; actual DXC and those GPU runtimes remain
unavailable. The temporary type-only harness is removed.

All 1730 SVG images and seven example PNGs match phase 22 exactly; the shader
manifest is unchanged. The validated Metal window completes eight frames and
the 640x360 to 480x270 resize. Its actual retained-rendering
[whole-process measurement](benchmarks/cleanup-phase23-metal-2026-10-11.json)
changes the median by -1.09% across two warmup and ten alternating timed pairs.
This application check does not isolate sparse-upload recording or GPU throughput;
no rendering speedup is claimed. Temporary measurement copies are removed.

The audit also reads the website configuration, homepage and translation data,
plus the independent native artifact/scatter/retirement tests. They use current
native APIs and contain no retired execution path. The HLSL editor test's generic
successful reply to incoming client requests is recorded for a focused protocol
followup. The whole-repository audit remains in progress.

## Phase 24: explicit editor protocol errors and smaller application shaders

The HLSL editor test no longer replies successfully to every server request.
It advertises no client request handlers, so unsupported requests now receive
the [JSON-RPC method-not-found error](https://www.jsonrpc.org/specification).
In particular, it does not fabricate an empty configuration array without
advertising [configuration support](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/workspace/configuration.md).
Server request IDs are independent of client request IDs: an incoming request
can no longer satisfy the predicate waiting for the initialize response.
The message loop also uses one deadline and reports a consistent timeout without
extending short waits. These are permanent protocol fixes, not a substitute
language server or shader compiler.

Nine focused tests execute the production message loop, covering unsupported
methods, configuration, numeric/string/null IDs, bidirectional ID collisions,
notifications, server errors, reader/write failures and timeouts. Before fixing,
four test functions reproduced seven failing subtests and one error. All nine
now pass. The actual ShaderTools server remains unconfigured and its end-to-end
editor test is explicitly skipped; protocol tests do not establish editor or
HLSL compiler compatibility.

Complete reads of all sixteen application shader files found copied helpers,
unused bindings and unused coordinate calculations. Removing them deletes 73
lines across ten Metal/HLSL files. Applications still provide complete native
shader source. The uniform ABI and every live shader formula, tiled dispatch,
sampling and color conversion remain unchanged. No source generator or legacy
execution path is introduced.

All ten focused real Metal shader tests pass. The full serial release suite,
including ignored tests, reports 1049 passes and the same three known failures
described in phase 12. All twelve example tests pass, as do formatting and release
Clippy with no new warnings. Actual DXC compilation and DX12/Vulkan GPU execution
remain unavailable; deleting unused HLSL declarations is not claimed as an actual
Windows compiler check.

All 1730 SVG images and seven example PNGs match the baseline exactly. The
validated Metal window completes eight frames and its 640x360 to 480x270 resize;
its executable and the Metal shader manifest remain byte-identical. The actual
960x640 procedural-paint/scanline-filter application has a median change of
+0.22% across two warmup and ten alternating timed pairs in the
[whole-process measurement](benchmarks/cleanup-phase24-metal-2026-10-11.json).
Every measured PNG matches the baseline. Timings include initialization, warmed
shader-cache loading, rendering, GPU readback and PNG output; they do not isolate
shader parsing or GPU throughput. No runtime speedup is claimed. Temporary
measurement copies are removed.

Native interop boundaries, the remaining renderer GPU tests and shader sampling
tests were also read completely. Their ownership, synchronization and transparent
sampling rules implement current semantics. The whole-repository audit continues.

## Phase 25: one filter default and no unused example forwarders

The filter uniform's 120-line initializer repeated zero for every scalar/vector
except `downsample = 1`. Runtime initialization now uses its existing `Zeroable`
implementation and explicitly sets that unit factor. The layout-only source
remains usable by build-time reflection without importing runtime dependencies.
No dependency, unsafe implementation or compatibility shim is added. This is a
permanent removal of duplicate initialization; filter algorithms and ABI remain
unchanged.

A semantic preservation test passed before and after the change. It checks the
512-byte extent, alignment, signed/float/vector offsets and every default word,
including positive floating zero and explicit padding. All 29 focused serial
release tests pass: the default test, fourteen native filter tests, eleven ABI/
catalog tests and three real GPU progressive-blur tests. After the final cleanup,
the full release suite including ignored tests reports 1050 passes and the same
three known failures described in phase 12. All twelve example tests pass, as do
formatting, the backend-free library check and release Clippy with no new warnings.
Actual DXC, ShaderTools and DX12/Vulkan GPU runtimes remain unavailable.

Complete reads of the twenty shared example files found three unused forwarders:
`save_image`, `save_example_image` and `new_font_system`. Repository-wide symbol
search confirms they have no callers. They and their unused import are removed;
live font snapshots, SVG family resolution, path/bitmap text examples and shader
programs keep their current behavior. The remaining nine independent integration
test files were also read completely. Their explicit compiler errors, pinned
backend selection, fixture pixels and font behavior are current test contracts.

All 1730 SVG images and seven example PNGs match the baseline exactly. The
validated Metal window completes eight frames and its 640x360 to 480x270 resize;
the actual Metal shader manifest is unchanged. The real 960x600 progressive blur
application's [whole-process measurement](benchmarks/cleanup-phase25-metal-2026-10-11.json)
has a median of 62.31 ms before and 60.55 ms after (-2.82%), across two warmup and
ten alternating timed pairs. Every measured PNG matches. Timings include warmed
native-cache loading, initialization, rendering, GPU readback and PNG output;
they do not isolate uniform initialization or GPU throughput. No speedup is
claimed. Temporary measurement executables/output are removed. The repository
audit has now read 408 files; eight initial file groups remain pending.

## Phase 26: remove retired coarse-cost bookkeeping

`coarse_binning_costs`, `prefer_dense_binning` and `CoarseBinningStats` only
served tests for a retired kernel-selection policy. Production tile indexing
still maintained per-bin candidate maxima, rescanned every bin on rebuild and
tracked/rescanned affected bins on sparse updates. Removing the retired policy,
three statistics fields and their update traversals permanently removes that
unconsumed work. The current dense-spatial-change policy, compact/scalar/chunked
coarse scheduling, painter order and GPU kernels remain unchanged. The live
partial-edge bin-count helper and its test now belong to the coarse scheduler.
Four tests specific to the retired statistics/policy are removed.

The almost 3000-line GPU planning file is split into buffer lengths, capacity
contracts, stable path allocation and tile membership/storage/update modules.
Active-batch queries live with the tile index whose private state they inspect;
tests live with their owners. Live algorithm bodies are preserved, apart from
removing the unused statistics and using the existing page-count accessor.
The hot update methods remain available for cross-module inlining: exploratory
measurements and emitted symbols showed that simply moving them changed their
optimization boundary. No compatibility layer or alternate implementation is
added.

A new semantic preservation test passes before and after cleanup. It compares
every tile against a scalar painter-order oracle across partial edge bins,
more than 256 overlapping draws, sparse motion, off-canvas movement, overlapping
change ranges and rank changes. The page-compaction test now checks the actual
compacted allocation, empty free list and exact membership instead of retaining
an internal counter only for tests. Two public statistics fields that were never
populated, `tile_pages_rewritten` and `tile_page_compactions`, are removed; the
English profiling guide no longer advertises the unavailable page count.

All 40 focused planning/coarse tests pass. The final full serial release suite,
including ignored tests, reports 1047 passes and the same three known failures
described in phase 12. All twelve example tests, formatting, backend-free
checking and release Clippy pass with no new warnings. All 1730 SVG images and
seven example PNGs match the baseline exactly. The validated Metal window
completes eight frames and its 640x360 to 480x270 resize. The Metal shader manifest
is unchanged. Actual DXC and DX12/Vulkan GPU runtimes remain unavailable.

The [real-renderer measurements](benchmarks/cleanup-phase26-metal-2026-10-11.json)
use retained-scene transactions and actual Metal textures, command submission
and completion for sparse controls, a scrolling grid and a large moving panel.
Two sets of eight alternating pairs retain 2304 timed frames per variant and
workload. Their combined complete-frame medians change by -2.73%, +0.07% and
-0.51%; host submission medians change by -1.17%, +0.21% and -1.08%.
Frame timing excludes initialization, final readback and PNG writing. All 150
final readbacks across exploratory, warm, timed and control processes match.
The existing `native_present metal --smoke` application's whole-process median
changes from 235.87 to 234.14 ms (-0.73%) across eight alternating pairs.
No rendering speedup is claimed.

The first final series had a worse large-panel p95, while its prespecified repeat
reversed that result. Same-executable controls also vary substantially at p95,
including 15.94 versus 6.63 ms for scrolling. All samples and series are retained;
latency tails are too unstable to attribute to this cleanup. These measurements
do not establish universal workload or other-backend performance. Temporary
measurement source, executable copies and PNG outputs are removed.

The audit inventory now distinguishes removed historical paths, current
implementation files and generated benchmark/lockfile data. Newly split files
and previously omitted retained/shared tests are included in the current
inventory. Nine current file groups remain pending; the repository-wide audit
is still in progress. Newly confirmed ignored incremental configuration and
test-only retired planner implementations are recorded for the next cleanup.

## Phase 27: remove unused incremental controls and retired test implementations

`CoarseBinningMode` and `coarse_binning` had no production consumer. The
direct-render exit ratio/frame controls were consumed only by a test-only
transient-output planner; production never emitted `DirectTransient` or
`RebuildHistory`. Those controls, variants and the retired planner are removed,
along with the test-only `ActiveScanPlan`/`append_cumsum_path` implementation and
their three self-contained tests. Seven public statistics fields with no
producers are also removed: `dense_coarse_batches`, `scanned_paths`,
`scanned_lines`, `scan_chunks`, `filter_dispatches`, `compact_filter_dispatches`
and `reused_compiled_plan`. Current native history routes, path allocation and
scan execution retain their algorithms. No compatibility aliases are added.

The unused direct-render policy also caused a real configuration bug: its
default exit ratio of 0.4 rejected otherwise valid full-redraw thresholds at or
below 0.4. A release regression reproduced that rejection before removal. The
remaining validation now accepts every finite threshold in `(0, 1]` and still
rejects invalid values. This fixes the cause, rather than adding another default
or special case. The obsolete Wasm-specific texture-budget branch is removed;
the native budget remains 256 MiB.

The former 1038-line incremental module now owns current damage planning, with
separate configuration/diagnostics and snapshot-comparison modules. Tests are
split by owner. Structural review confirms that all preserved algorithm and test
bodies are unchanged, apart from the intended configuration default/validation
cleanup. The snapshot-comparison route still has live dependency-damage
responsibilities; its version and backdrop-cache contracts need further audit
before determining whether it can be removed.

Two new configuration tests cover accepted thresholds and invalid-domain edges.
Two new actual Metal tests pass both before and after cleanup: they check owned,
tracked, persistent, transient and nonzero-origin output routes, repeat-frame
statistics and pixels, plus persistent path holes, growing allocations and
subsequent motion against fresh rendering. All 28 focused release tests pass,
including the five current native scan tests. The final full serial release suite
including ignored tests reports 1048 passes and the same three known failures
described in phase 12. All twelve Metal example tests, formatting, backend-free
checking and release Clippy pass with no new warnings.

All 1730 SVG images and seven example PNGs match the baseline exactly. The
validated Metal window completes eight frames and its 640x360 to 480x270 resize;
both active Metal manifests remain byte-identical. The
[existing window application's whole-process measurement](benchmarks/cleanup-phase27-metal-2026-10-11.json)
uses two warmup processes per variant and ten alternating timed pairs. Its
median changes from 227.88 to 223.16 ms (-2.07%); nearest-rank p95 changes from
231.80 to 227.36 ms. Every measured process completes all eight frames and the
resize. These timings include initialization, cached shader loading, rendering,
presentation and teardown; no GPU or frame-time speedup is claimed. Temporary
baseline executable and source backups are removed. Actual DXC and DX12/Vulkan
GPU execution remain unavailable.

Complete reads of more scheduling/resource files also identified three repeated
filter-resource traversals and upload repacking. That independent cleanup is
recorded for the next stage. Eight current implementation groups still contain
unread files; the repository-wide audit remains in progress.

## Phase 28: one filter-resource collector and direct upload payloads

Transfer, convolution, turbulence and filter-brush preparation each recursively
walked the same execution/filter tree. The new `FilterResourceUpload` collects
all four payload kinds in one traversal, preserving child-before-filter,
backdrop-before-children and content-before-mask order. An enclosing localized
filter is appended after its child plan. The three old upload types and the
separate brush collection constructors/walkers are deleted, with no forwarding
aliases or alternative preparation path.

Transfer tables now borrow their original fixed-size arrays through native
upload, removing both flattening and array reconstruction. Generated turbulence
lattices move directly into upload instead of being flattened and reconstructed
with new gradient allocations. Brush words move into the native owner after
byte upload, removing its full-vector clone. Variable convolution offsets and
the GPU byte layout remain unchanged. Immutable image associations, public
value/address validation and batch ownership remain enforced. This permanently
removes redundant preparation at its source.

A mixed-payload preservation test passed on the old implementation before
cleanup. It independently checks transfers, variable kernel offsets, seeded
lattices, encoded brush words and cursor positions across nested filters,
backdrops, masks, sibling operations and an optional enclosing filter. The same
assertions pass after the collector change. Another test covers empty plans,
empty chains/graphs and filters with no resource payloads. Existing order,
skipped-work, localized-index and native brush boundary tests remain in use.

All 54 focused serial release tests pass. The broader `--all-targets` release
suite, including ignored tests, reports 1053 passes and the same three known
failures described in phase 12. This includes all twelve example tests, including
the automatically discovered SVG/mesh examples. Formatting, backend-free
checking and release Clippy pass; Clippy warnings decrease from 4/12 to 1/8 for
the library/library tests. All 1730 SVG images and seven example PNGs match the
baseline exactly. The validated Metal window completes eight frames and its
640x360 to 480x270 resize. Both active Metal manifests remain byte-identical.
Actual DXC, ShaderTools and DX12/Vulkan GPU execution remain unavailable.

The [application measurements](benchmarks/cleanup-phase28-metal-2026-10-12.json)
render four existing 300x300 SVG inputs through `NativeRenderer`: transfer,
convolution, turbulence and image brushes. Initial separate-process comparisons
sometimes show slower results; repeating them with the identical executable
also produces changes of tens of percent. Longer sampling and interactive
thread QoS alone do not remove this variation. Those earlier results and their
controls are retained, rather than discarded.

The final comparison links the committed baseline and current implementation
into one measurement process and alternates versions every frame, waiting for
each completion. Two current renderers provide a same-implementation control.
Each mode has two warmup processes and eight timed processes; every workload
warms up 64 cycles and measures 512 cycles per process. Each variant/workload
therefore has 4096 timed frames. Complete-frame medians change by -0.64%, +0.51%,
-1.10% and +0.12%; submission medians change by -0.38%, -0.13%, -0.56% and -1.10%.
Same-implementation complete-frame medians vary by up to +1.36%. These measured
applications preserve performance within observed control variation. No
universal GPU or frame-throughput speedup is claimed. Parsing, initialization,
final readback and PNG output are excluded from frame timings.
Latency tails remain variable: the image-brush same-implementation control's
complete-frame p95 changes by +31.58%, so tail changes are not attributed to the
cleanup.

All 720 measurement readbacks match the immutable SVG baseline exactly. Every
earlier series, control and individual sample is preserved in the report's
compressed raw-data archive. The two linked versions have identical shader
recipes; their artifact hashes differ between compilation roots, so linked
artifacts are not described as byte-identical. Temporary Rust measurement
programs, isolated baseline source, copied executables, QoS helper and readback
PNGs are removed after validation.

Additional complete reads cover the filter scheduler/cursor tests, shared brush
encoder and scene/glyph upload staging. The whole-repository audit remains in
progress, including the retained version/journal-loss/backdrop contract and
eight groups with unread implementation files.

## Phase 29: versioned retained frames and rejected-target damage

The only production retained-frame constructor is the persistent materializer.
Its frames always have a scene version and complete retained content. The
optional version and incomplete-content redraw branch only served manual test
fixtures. `RetainedFrame.version` is now mandatory; `incremental_complete` and
`FullRedrawReason::UntrackedContent` are deleted, together with the test that
fabricated that impossible production state. Immediate canvases still select
the explicit non-retained full-render route.

Three other fields are removed: node `placement_bits` was always `None`, node
`order` only copied an unused number, and frame `dependency_free` had no readers.
Painter ordering continues to use actual node indexes. The materializer's live
`dependency_free` classification remains in use. Node comparison now uses the
complete meaningful state directly. Identical materialized-frame selection
branches collapse into one version-aware cache update; no aliases or compatibility
states remain for the deleted representations.

Tracing the snapshot route found a real rendering bug. Two successive public
retained render requests can materialize new scene versions and then reject an
incorrect output size before rendering begins. State-delta pruning can leave
those versions sharing the same base-node array. The old pointer-only static
shortcut then reports no damage and displays old pixels. A permanent native
regression failed on unchanged production code with this exact sequence. The
shortcut now also requires equal versions, allowing logical overlay comparison
when the rendered cursor is behind. This fixes the incorrect assumption at its
source; it does not add a recovery implementation or force a full redraw.

The regression covers plain content, a root backdrop and a backdrop inside an
ordinary filter, with overlapping later foreground. It compares the recovery
and next partial frame against fresh native rendering. A separate preservation
test expires the actual 256-entry scene journal and verifies both root/nested
backdrop histories, the following partial frame and an unchanged frame. That
test also passed before the fix. Snapshot comparison and journal-loss cache
reconstruction are current submission semantics, as demonstrated through the
public renderer, so they are retained.

The 1342-line materializer damage file is split into dependency propagation,
frame construction, content/local/surface/topology patches and spatial-index
ownership. Each resulting file has fewer than 300 lines. A token-level review
confirms every moved method body is identical after the documented inert-field
and version changes, apart from formatting and the moved module path.

Focused serial release selections pass: 163 retained tests, 20 final incremental
tests and both native regressions. These selections overlap and are not added
as a distinct-test total. The full `--all-targets` release suite, including ignored
tests, reports 1054 passes and the same three known failures documented in phase
12; all twelve example tests pass. Formatting, backend-free checking and release
Clippy pass, with the existing 1/8 library/library-test warnings and no new ones.
All 1730 SVG images and seven example PNGs exactly match their immutable baseline.
Both active Metal manifests remain byte-identical. The validated native window
completes eight frames and its 640x360 to 480x270 resize.

The [application measurements](benchmarks/cleanup-phase29-metal-2026-10-12.json)
retain every observation from 48 real window processes: baseline/current and
identical-current controls, each with two warmups per label and ten timed
alternating pairs. Whole-process medians change from 233.845 ms to 233.126 ms
(-0.31%); the identical-current control changes by -0.003%. The measured application
preserves performance. Startup, presentation, resize and teardown are included;
this does not establish a GPU or isolated frame-time improvement. No temporary
Rust measurement program was added, and the copied baseline executable is
removed after validation. Actual DXC/ShaderTools and DX12/Vulkan GPU execution
remain unavailable. Whole-repository auditing continues.

## Phase 30: remove duplicate fused-layer operation markers

`ExecOp` now contains only draw batches and the two offscreen operations. The six
CPU Begin/End clip, opacity and blend variants were emitted by compilation and
copied during offscreen localization, but the executor ignored them. Their state
already lives in each batch's ordered `LayerStackEntry` snapshot. The obsolete
variants, emission/copying/no-op branches and fifteen newly unreachable wildcard
arms are removed. This deletes the duplicate representation permanently.
GPU coarse/fine begin/end particles still implement the actual raster semantics.

Clip, opacity and blend compilation now share one push/recurse/pop traversal.
Empty fused groups no longer obstruct coalescing adjacent batches with the same
stack. Painter order, hidden draw references and retained branch membership
remain in the existing batch metadata. Tests check nested group stacks and empty
groups through the meaningful state instead of asserting obsolete marker tokens.

Fixed clip slots previously used marker kinds to reject opacity/blend schedules.
That decision now examines the actual stack entries and requires a draw-only,
pure-clip plan. Group compositing needs particles beyond the pure-clip slot
reservation. New native tests cover both nested and sequential opacity/blend
stacks; all four combinations remain excluded. Both new semantic preservation
tests passed before the removal, then passed with the simplified representation.

The 1138-line execution file is split into types, batching/indexes, fragment
relocation, layer-stack storage, retained offscreen identity, layer patches and
surface patches. Each resulting file is below 300 lines. A token-level review
checks all 42 moved function bodies against the previous commit; they are
identical after removing comments, formatting and obsolete exhaustive-match
wildcard arms. A contradictory append/insert comment is simplified as well.

Focused serial release selections pass: 17 Canvas compilation tests, 13 clip
scheduling tests, 163 retained tests and six operation traversal tests. These
selections can overlap and are not reported as a distinct-test total. The full
`--all-targets` release suite, including ignored tests, reports 1056 passes and
the same three known failures documented in phase 12; all twelve example tests
pass. Formatting, backend-free checking, release Clippy and example builds pass,
with the existing 1/8 library/library-test warnings and no new ones. All 1730 SVG
images and seven example PNGs exactly match their immutable baseline. Both active
Metal manifests are byte-identical. With Metal validation enabled, the native
window completes eight frames and its 640x360 to 480x270 resize.

The [application measurements](benchmarks/cleanup-phase30-metal-2026-10-12.json)
retain every observation from 48 real SVG rendering processes: baseline/current
and identical-current controls, each with two warmups per label and ten timed
alternating pairs. Every process renders, reads back and saves all 1730 fixtures
at 300px, and every resulting PNG matches the immutable baseline. Whole-process
medians change from 4285.493 ms to 4263.504 ms (-0.51%); the identical-current
control changes by +0.48%. This preserves the measured application workload.
Initialization, SVG loading, planning, GPU completion, readback, PNG writes and
teardown are included, so these numbers do not establish isolated GPU or frame
time improvements. Validation is disabled for timing and enabled separately for
the full tests/renders. No Rust measurement program was added; the copied
baseline executable and temporary measurement scripts are removed afterward.

Additional complete reads cover Canvas draw/layer/path recording and related
tests. Repeated command-root repair, an empty-stack substitute and redundant
forwarders/extrema scans are traced for the next cleanup. The audit remains in
progress with 438 current implementation files unread. Actual DXC/ShaderTools
and DX12/Vulkan GPU execution remain unavailable on this host.

## Phase 31: own Canvas recording state directly

Canvas now owns one current command-list ID and a stack of parent-list/kind pairs.
Constructor, reset and retained reconstruction establish root list zero; closing
an actual layer restores its saved parent directly. The duplicate root field,
parallel command/kind stacks, per-draw missing-root repair, empty-stack root
substitute and guarded second pop are removed. The never-constructed public
`LayerKind::Root` variant is deleted; popping at the root still returns `None`.
This is a permanent representation cleanup, with no replacement recovery path.

Layer kind is derived from its actual command, removing a second caller-supplied
kind. Ten private forwarding entry points now live directly in their public
methods. Append uses the established root and current list, and draw-brush lookup
uses the index already checked by `draw_index`. Retained chunk installation
returns one command-list range instead of separately returning its identical
root/start indices. Image/geometry validation, stale draw-handle rejection,
append closure rules and filter-region semantics remain necessary current
behavior. Both website Canvas API pages document root/pop/reset behavior.

Rectangle recognition computes its four-corner bounds once. An approximately
duplicate closing point participates in recognition tolerance without expanding
the final rectangle. Review caught the first cleanup incorrectly reusing bounds
that included that point: a focused test failed before the correction and now
passes. The final algorithm preserves the original corner geometry while
removing repeated extrema scans.

Three lifecycle preservation tests passed before the production changes and
remain permanent. They exercise all eight nested layer kinds, cloning while open,
mask/append imports into a parent and reset with 64 open groups, including stale
draw handles. Three existing text tests previously returned without assertions
when host font lookup produced no layout. They now load the checked-in font and
require nonempty shaping before checking append and DPI behavior.

The 609-line primitive implementation, 1322-line primitive tests and 1154-line
retained plan module are split by responsibility. Every resulting child file is
below 300 lines. Token-level review confirms all 32 primitive functions, 48
original test/helper functions and 24 retained plan methods are preserved after
the documented root-call/text-test edits; the plan split is compared against its
edited pre-split snapshot. The checked-in-font helper is the one new primitive
test helper. Painter initialization allocations and plan eligibility contracts
are recorded for a subsequent audit that measures an affected retained
application workload.

Focused serial release selections pass: 116 Canvas-related tests, 163 retained
tests and nine filter-resource tests. Selections overlap and are not added as a
distinct-test total. The full `--all-targets` release suite, including ignored
tests, reports 1060 passes and the same three known failures documented in phase
12; all twelve example tests pass. Formatting, backend-free checking, release
Clippy and example builds pass, with the existing 1/8 library/library-test
warnings and no new ones. All 1730 SVG images and seven example PNGs exactly match
the immutable baseline. Both active Metal manifests are byte-identical, and the
validated native window completes eight frames and its 640x360 to 480x270 resize.

The [application measurements](benchmarks/cleanup-phase31-metal-2026-10-12.json)
retain all 48 real SVG rendering process observations: baseline/current and
identical-current controls, each with two warmups per label and ten timed
alternating pairs. Every process renders, reads back and saves all 1730 fixtures
at 300px; every PNG matches the immutable baseline. Whole-process medians change
from 4373.299 ms to 4356.943 ms (-0.37%), compared with -0.34% in the identical-current
control. Nearest-rank p95 changes by +3.00%, compared with +2.65% in the control;
with ten timed observations per label, this is the maximum sample. These results
preserve the measured application workload within process variation. They include
initialization, loading, CPU planning, GPU completion, readback, PNG writes and
teardown, and do not establish isolated GPU or frame-time improvements. Validation
is disabled for timing and enabled separately for tests/renders. The copied
baseline executable and temporary measurement/edit scripts are removed afterward;
no Rust measurement program or instrumentation was added.

The Canvas group now has no unread implementation files. The wider audit remains
in progress with 434 current implementation files unread across six groups.
Actual DXC/ShaderTools and DX12/Vulkan GPU execution remain unavailable here.

## Phase 32: give retained painter metadata one owner

Canvas directly owns one `PainterMetadata` containing painter keys, stable batch
IDs and live batch counts. Ordinary canvases keep empty vectors and use their
execution plan's order; the existing persistent-root identity selects retained
metadata. The three independently optional arrays and their missing-array
allocation/recount substitutes are deleted. Constructor/reset establish empty
owned data, and materialization initializes all arrays together. This permanently
removes invalid partial initialization states without adding a compatibility path.

A full painter rebuild now moves the previous metadata for dirty-range comparison
instead of cloning both old arrays. It creates batch membership once from the
compiled plan, deleting the inactive batch allocation that was immediately
replaced. Incremental updates move the complete metadata once per update, then
write keys and batch membership in one per-draw traversal. Per-node take/unwrap/
restore cycles, a shared writer with a meaningless `batch=None` rebuilding mode
and a now-redundant immediate-mode condition are removed. Batch synchronization
has one caller immediately after `refresh_compiled_plan`, so its silent absent-plan
return is replaced with the producer's required invariant.

Immediate-plan ordering, historical dead-batch counts, sparse active queries,
abandoned-upload recovery and pending-plan rebuilding remain current semantics.
Out-of-range historical batch counts represent dead batches; they cannot be
replaced by unchecked indexing. The remaining per-draw compiled-membership and
plan-fragment eligibility contracts are still tracked for further audit.

Two permanent lifecycle tests passed before the production change. They cover
empty initialization, empty/nonempty insertion and removal, DPI rebuilding,
multi-leaf offscreen content patches, coherent counts and an immutable earlier
Canvas snapshot. Tile-bin and upload-journal tests now consume actual retained
materialization rather than constructing root-marked canvases without metadata.
The GPU journal test still exercises all 256 accepted/abandoned batch transitions.
Painter types are isolated in a small Canvas module, and active-batch query tests
are separated from their implementation.

Focused serial release selections pass: 165 retained, 116 Canvas-related, 30 GPU
planning, six draw-batch and six operation tests. Selections overlap. The full
`--all-targets` release suite including ignored tests reports 1062 passes and the
same three known failures documented in phase 12; all twelve example tests pass.
Formatting, backend-free checking, release Clippy and example builds pass, with
the existing 1/8 library/library-test warnings and no new ones. All 1730 SVG images,
seven example PNGs and both active Metal manifests match their immutable baseline.
The validated native window completes eight frames and its 640x360 to 480x270 resize.

The [retained application measurements](benchmarks/cleanup-phase32-metal-2026-10-12.json)
record 48 real rendering processes and all 1152 completed frame times. Each process
uses public `RetainedScene`/`NativeRenderer` APIs for three independently initialized
scenes, initially 256 nodes and 768 draws with 32 opacity-filter leaves. Eight
stages per scene cover initialization, replacing all content, removing/reinserting
64 nodes, growing 16 leaves to 12 draws, reordering, shrinking to 384x224 and
returning to 512x256 with another content edit. Every frame completes GPU readback
and saves a PNG; all runs reproduce the 24 validated baseline images exactly.

Baseline/current and identical-current modes each have two warmups per label and
ten timed alternating pairs. Whole-process medians change from 175.971 ms to
170.546 ms (-3.08%), compared with -1.79% in the identical-current control.
Nearest-rank p95 changes by -10.72%, compared with -5.24% in the control; with ten
observations per label, p95 is the maximum sample. This preserves the complete
measured application workload. Stage-level medians are recorded in the data,
including small apparent increases, but the same-version stage timings vary more
(for example, initialization changes by -23.42%). They do not establish isolated
stage or GPU improvements or regressions. Whole-process time includes construction,
mutation, materialization, GPU completion, readback, PNG writes and teardown;
per-frame times exclude PNG saves. Metal validation is disabled for timing and
enabled for separate probes/tests/renders.

The temporary Rust application source, edit/measurement scripts and copied
executables are removed after validation; no production measurement hooks remain.
Whole-repository auditing continues. Native scene assembly now has a complete
read, including upload acceptance, snapshots and program-image lifetime; its large
recording module and remaining initialization/cache contracts are queued for the
next cleanup. The audit has now read 526 of 953 current implementation files;
427 remain across six groups. Actual DXC/ShaderTools and DX12/Vulkan GPU execution
remain unavailable.

## Phase 33: unify initialized scene metadata and shader detection

Native scene preparation now owns one optional, complete `PlanMetadata` containing
its fingerprint, stack depths and program-brush detection. `PreparedPlan` carries
those values into recording. The separate `SceneCache::has_programs` and
`programs_initialized` flags are deleted. An installed retained plan no longer
implies that this renderer has already initialized its metadata; reuse requires
an actual prior metadata snapshot. Explicit localized plans compute the same
complete metadata from their associated Canvas.

This fixes two real bugs rather than adding a temporary recovery path. Execution
fingerprints deliberately omit brush data, but the old program flag was retained
when immediate frames changed from solid paint to a native shader with the same
commands. The renderer skipped that shader and produced different pixels from a
fresh renderer. The permanent public-renderer regression failed on frame 1 before
the fix. Program detection now refreshes on immediate frames and on retained
draw/brush changes; only an explicit unchanged retained journal can preserve it.
A separate new regression failed because a first installed plan with retained
structure/value hints incorrectly reported reused metadata, leaving stack depths
at their uninitialized defaults. Initialized metadata ownership fixes that contract
at its source. Tests that intentionally force a fingerprint mismatch now mutate
an initialized fingerprint rather than remove its initialization evidence.

The 697-line native scene module is divided into shared ownership types, cache
lifecycle, scene input upload, CPU work-arena upload and raster dispatch. Each
resulting module is small and keeps its implementation dependencies local.
Thirteen moved function/work-block comparisons confirm unchanged bodies, apart
from the work block's `active` parameter and its extraction boundary. Frame-local
shader images, accepted GPU snapshots, abandoned-upload recovery, empty GPU buffer
allocations and physical extent validation retain their required semantics.

Four permanent tests cover first installed metadata under both retained hints,
draw membership edits without re-encoding brushes, same-topology solid/shader/
uniform switching with and without an SDF clip, and a shader compilation failure
followed by successful shader and solid frames on the same renderer. Focused serial
release selections pass: eight preparation, two shader-cache, 43 native scene and
165 retained tests; selections overlap. Full release tests including ignored
cases pass 1066 with the same three known failures documented in phase 12. All
12 example tests, release Clippy, formatting, backend-free checking and example
builds pass; Clippy retains the existing 1/8 library/library-test warnings.

All 1730 SVG native PNGs, seven example images and both active Metal artifact
manifests match their immutable baseline. The validated native window completes
eight frames and its 640x360 to 480x270 resize. The separate 32-frame actual
application probe also matches old rendering exactly.

The [actual application measurements](benchmarks/cleanup-phase33-metal-2026-10-12.json)
retain 96 process observations and all 1536 mixed-scene frame samples. Each of two
workloads has a baseline/current series and an identical-current control, with
two warmups per label and ten timed alternating pairs. The mixed application uses
public renderer/retained APIs for 768 draws, 32 opacity-filter leaves and 16 native
shader-painted draws. Two rounds cover immediate paint replacement and retained
content edits, transforms, removal/reinsertion, reordering and resize. Paint kinds
stay fixed so baseline/current images are comparable; separate regressions cover
the previously broken kind switches. The other workload runs the complete
1730-fixture SVG application, including ordinary paint with no shader brushes.

Mixed whole-process medians change 366.204 to 375.645 ms (+2.58%), compared with
+4.00% in the same-version control. SVG medians change 4418.261 to 4396.405 ms
(-0.49%), compared with +0.43% in its control. These workloads show no detectable
material regression; they do not establish an isolated GPU/stage improvement or
guarantee every workload. Nearest-rank p95 is the maximum of ten observations.
Whole-process time includes initialization, preparation, GPU completion, readback,
PNG writes and teardown; mixed per-frame samples exclude PNG saving. All 84576
measured PNGs match their validated baseline manifests. Timing disables Metal
validation; separate tests and probes enable it. The mixed workload explicitly
pins registry ID `0000000100000575`; the SVG application selects the system-default
Metal device on this verified single-GPU Apple M5 host.

Temporary Rust source, timing script and copied/compiled measurement executables
are removed; no production instrumentation remains. The audit has read 545 of
958 current implementation files, leaving 413 across six groups. Newly traced
native execution state and obsolete filter-work callbacks are the next concrete
cleanup targets. Actual DXC/ShaderTools and DX12/Vulkan GPU execution remain
unavailable here; independent repository auditing continues.

## Phase 34: remove empty filter hooks and transient execution slots

The shared filter scheduler still exposed source/output work preparation hooks,
but its sole production adapter returned success without doing any work. Test
adapters then simulated failures in those nonexistent stages. Remove both hooks,
their calls, trace events and fake failure cases. Keep halo damage selection before
scan, unconditional output damage restoration, and the single scratch/context
ownership boundary for actual copy, filter, scan and allocation failures.

Shared draw scheduling always recorded coarse immediately before fine. The
adapter now records one complete draw and keeps its FinePlan local to that call.
Delete the optional FinePlan cache and its missing-coarse error, inline the
single-caller preparation wrapper, and remove intermediate batch IDs from test
adapters. Painter order, live membership, partial root updates, attempted-batch
statistics and stop-on-error behavior retain their existing semantics.

Execution and saved parent contexts now store one SceneState: a pending matched
plan or a scanned Scene. This replaces two independent optional fields and their
repeated missing-scene branches. Filter scan still occurs after selecting local
damage; a successful scan installs the complete Scene. Ready scenes stay inline
to avoid a heap allocation for each context. Each scan clones one Rc plan handle
while recording, then releases the pending handle. Failed recording still
invalidates the command batch; state restoration is not permission to submit it.
This is permanent removal of obsolete scheduling abstractions at their owners.

Before production edits, the new nested-context semantic test and strengthened
partial-failure damage test pass on the old implementation. Final tests verify
nested success/failure restores each parent's plan identity, origin and target,
and subsequent fine passes bind that target. Context tests move into their own
181-line module with shared setup. Draw tests cover failure before and after a
write without retaining coarse/fine scratch state.

Release tests run serially with Rayon limited to one thread. Focused shared
render tests pass 231 cases, native execution passes seven, and retained-scene
tests pass 109. The full all-target run passes 1067 tests with the same three
existing failures: the two unavailable pinned-DXC compiler checks and the
1600-pixel turbulence canonical comparison. Formatting, release clippy,
backend-free compilation and all example builds pass; clippy retains its existing
one library/eight test warnings. Metal validation is enabled for correctness.
All 1730 SVG fixture images and seven example outputs match their immutable
baseline pixels exactly. Both Metal artifact manifests match. Native presentation
completes eight frames and its 640x360 to 480x270 resize.

The [actual application measurements](benchmarks/cleanup-phase34-metal-2026-10-12.json)
retain 96 whole-process observations across the SVG fixture application and the
native retained-window example. Each has baseline/current and identical-current
control series, two warmups per label and ten alternating timed pairs. Window
medians change 232.742 to 233.304 ms (+0.24%), compared with +0.30% in its
same-version control. SVG medians change 4276.020 to 4269.695 ms (-0.15%), compared
with +0.06% in its control. These measured applications show no
detectable material regression; whole-process timing does not establish an
isolated GPU/stage improvement or cover all partial-filter workloads. Nearest-rank
p95 is the maximum of ten observations; a current-SVG outlier raises it from
4311.691 to 4485.497 ms, so this small sample cannot establish tail behavior.
Timing includes setup, rendering, completion and teardown; SVG additionally includes parsing, readback and PNG
saving. All 83040 measured PNG hashes match the immutable baseline, and all 384
window frames complete with the expected resize. Timing disables Metal validation.
The window explicitly pins registry ID `0000000100000575`; the SVG application
uses the system default on this verified single-GPU Apple M5 host.

Temporary measurement/validation scripts and copied executables are removed.
The audit has read 558 of 959 current implementation files, leaving 401 across
six groups. Native CPU layer/stack upload helpers have only validation-test callers; their removal and
an audit of active mask reference semantics are next. Actual DXC/ShaderTools and
DX12/Vulkan execution remain unavailable; independent auditing continues.

## Phase 35: delete dormant native layer upload frontends

Geometry::upload, its CPU Scene slices and Stack::upload had no production
callers. Their only caller was a validation test exercising that second frontend.
Production already builds Geometry/Stack views from the owning Scene's uploaded
draws, scan outputs, paint and layer records. Delete the dormant upload path,
its duplicate validation/padding/copy logic and the test that kept it alive.
Actual shared LayerStackRecord conversion tests continue checking draw identity,
byte opacity and blend payloads. This permanently removes an unused alternative
implementation; no compatibility adapter or replacement uploader is added.

The active mask path also converted every missing logical draw into INVALID_INDEX
and recorded transparent coverage. Replace that silent substitute with an error
before allocating uniforms or recording a pass. The regression fails before the
fix against actual SceneCache geometry, then passes for empty, one-draw and
three-draw scenes. It rejects the first missing draw, the next index and u32::MAX,
checks resource/pass counts stay unchanged, and accepts the last valid draw.
Required physical ownership, alias, allocation and coordinate validation remains
at the shared native boundary.

Focused release tests pass the new regression, 39 scene tests and four current
filter validation tests. The serial full all-target run passes 1067 tests, with
the same three known failures: two unavailable pinned-DXC checks and the
1600-pixel turbulence canonical comparison. One old-frontend test is deleted and
one actual-path regression added. Formatting, release clippy, backend-free
compilation and all example builds pass. Existing clippy warnings remain.
Metal-validated rendering preserves all 1730 SVG fixture pixels and seven example
images exactly; both artifact manifests match. Native presentation completes
eight frames and the 640x360 to 480x270 resize.

The [affected actual SVG application measurements](benchmarks/cleanup-phase35-metal-2026-10-12.json)
retain 48 process observations: baseline/current and identical-current control,
two warmups per label and ten alternating timed pairs per series. Every process
loads and renders all 1730 fixtures. Medians change 4263.686 to 4264.850 ms
(+0.03%), compared with +0.22% in the same-version control. This workload's
central timing shows no material regression; the sample does not establish GPU
stage cost, tail behavior or every application workload. Nearest-rank p95 is the
maximum of ten observations; raw values and both tails remain in the report.
Timing includes initialization, SVG parsing, rendering/completion, readback, PNG
saving and teardown. All 83040 measured PNG hashes match the immutable baseline.
Timing disables Metal validation; correctness runs enable it. The SVG application
uses the system-default device on the freshly verified single-GPU Apple M5 host.

Temporary scripts and copied measurement executables are removed. The audit has
read 569 of 960 current implementation files, leaving 391 across six groups. Ten newly read layer/composite shader files reveal a
fixed 64-group evaluator whose reachability needs a complete producer proof:
normal compilation refuses to fuse opacity/blend around offscreen descendants.
Retained edits, fragment replacement and filter localization still need semantic
invariant tests before simplifying that shader. Actual DXC/ShaderTools and
DX12/Vulkan execution remain unavailable; independent auditing continues.

## Phase 36: offscreen clip composition

Complete producer tracing confirms offscreen `outer_stack` ranges contain only
clips. `Canvas::can_fuse` places opacity/blend ancestors with offscreen descendants
on their own surfaces, and retained patches preserve entry kinds. Fragment
relocation and local filter remapping also preserve this contract. The new semantic
tests cover depths 0, 1, 63, 64, 65 and 128 across filters, backdrops, masks,
isolates and mixed fused groups; producer tests pass before production changes.

Delete both shaders' dormant CompositeGroup evaluator, fixed 64-slot arrays,
group unwind and capacity-skipping branch, and remove the unused HLSL constant
header. Native ownership views are named ClipStack/filter_clip_stack and document
their offscreen range contract. Each composite reads the clip's draw index directly.
Actual offscreen blend, masks and fused fine-raster groups retain their semantics.
This is permanent removal of a second implementation with no production producer.

Tracing also exposes a real retained clipping bug: independent subtree compilation
started with an empty stack. The focused regression fails before the fix with
incremental depth zero versus fresh depth one. Recompilation now receives the
original ancestor stack at the compiler input; backdrop children inherit it during
the same traversal. This fixes the missing compiler context rather than applying
a post-render mask or a full-scene recompile substitute. Pinned GPU tests compare
nested insert/remove results for filter/backdrop/mask layers against fresh renders
and independently check outside transparency. Deep filter/mask/group pixels are
also verified through 128 clips.

Five focused release tests pass. The serial full all-target run passes 1072 tests,
with the same three known failures: two unavailable pinned-DXC checks and the
1600-pixel turbulence canonical mismatch. Formatting, release clippy, backend-free
compilation and all example builds pass; existing one library/eight test clippy
warnings remain. Metal validation is enabled for correctness. All 1730 SVG fixture
images and seven example outputs match their immutable baseline pixels exactly;
native presentation completes eight frames and its 640x360 to 480x270 resize.
Both Metal manifests change only the filter-stack artifact's two source files,
with ABI, bindings, compiler settings and all other artifacts unchanged. HLSL
execution remains unverified without the pinned DXC/Windows GPU environment.

The [actual application measurements](benchmarks/cleanup-phase36-metal-2026-10-12.json)
retain 48 whole-process observations across baseline/current and identical-current
control series, with two warmups per label and ten alternating timed pairs.
SVG medians change 4270.954 to 4280.525 ms (+0.22%), compared with +0.11% in the
same-version control. Central timings show no material regression in this
1730-fixture application, which measures initialization, parsing, rendering/completion,
readback, PNG saving and teardown; it does not isolate shader stage cost or establish
every retained-edit workload's performance. Nearest-rank p95 is the maximum of ten
observations; raw values and both tails remain in the report. All 83040 measured
PNG hashes match the immutable baseline. Timing disables Metal validation, and
the example uses the default device on the freshly verified single-GPU Apple M5
host. The fixture source manifest is also unchanged.

Temporary validation scripts and copied executables are removed. The audit has
read 573 of 962 current implementation files, leaving 389 across six groups.
Actual DXC/ShaderTools and
DX12/Vulkan execution remain unavailable; independent auditing continues.

## Phase 37: current retained fragment membership

Retained edits intentionally reuse execution structure while Canvas painter
metadata owns current draw order and batch membership. The painter updater
previously cleared every dirty slot, then tried to recover multi-batch leaves
from the old compilation table. Appended fragment draws were absent from that
table and became inactive through an out-of-bounds-to-MAX substitute. A focused
regression first reproduces a visible draw silently becoming inactive.

Install each fragment's actual per-draw assignments at both root insertion and
offscreen replacement. Metadata updates preserve live assignments and clear
retired/hidden slots after restoring current keys. Delete the old plan-table
clone and recovery branch. Fragment classification indexes its complete compiler
output directly instead of silently skipping missing assignments. This fixes
the ownership error permanently without adding a parallel cache or recompiling
the whole retained scene.

Pinned GPU tests then expose two consumers of the same outdated compilation
tables: filter candidate ordering drops newly inserted draws, and filter
localization indexes a table shorter than the current draw arena. Pass the
current Canvas to filter candidate queries; retained queries use current painter
keys, while immediate queries use their current compiled sequence. Persistent
bins own complete ranks, so remove the per-candidate rank-availability probe.
One Canvas membership accessor now serves localization, active batches and GPU
uploads. Exterior queries still include all possible filter sources; this is
required for filters that move off-canvas pixels into the visible output.

Four new semantic tests cover multi-batch insertion, reparenting a leaf whose
content becomes layered, persistent/transient candidate ordering and actual
filter pixels. GPU checks exercise both local and exterior sources, independent
color samples, and repeated removal/insertion to reuse physical slots. Nine
focused fragment tests and 21 spatial-bin tests pass. The serial full all-target
release run passes 1076 tests with the same three known failures: two unavailable
pinned-DXC checks and the 1600-pixel turbulence canonical mismatch. Formatting,
release clippy, backend-free compilation and all example builds pass; existing
one library/eight test clippy warnings remain.

Metal validation is enabled for correctness. All 1730 SVG fixture images and
seven example outputs match their immutable baseline pixels exactly. Native
presentation completes eight frames and its 640x360 to 480x270 resize. Both Metal
artifact manifests match exactly; this phase changes no shader source or ABI.

The [actual application measurements](benchmarks/cleanup-phase37-metal-2026-10-12.json)
retain 96 whole-process observations across the SVG fixture application and the
native retained-window example. Both include baseline/current and identical-current
control series, two warmups per label and ten alternating timed pairs. SVG
medians change 4312.680 to 4311.113 ms (-0.04%), compared with -0.33% in its
same-version control. Window medians change 233.454 to 233.206 ms (-0.11%),
compared with -0.36% in its control. These measured applications show no material
central timing regression. Whole-process timing does not establish an isolated
stage improvement or every retained-edit workload's performance. Nearest-rank
p95 is the maximum of ten samples; the SVG baseline/current maxima are 4520.416
and 4541.177 ms, so this sample cannot establish tail behavior.

Timing includes initialization, rendering/completion and teardown; SVG also
includes parsing, readback and PNG saving. Verification runs after each stopped
timer: all 83040 measured PNG hashes match the immutable baseline and all 384
window frames complete with the expected resize. The SVG source manifest is
unchanged. Timing disables Metal validation. The window explicitly pins registry
ID `0000000100000575`; SVG uses the system default on this freshly verified
single-GPU Apple M5 host.

Temporary validation/measurement scripts and copied executables are removed.
The audit has read 588 of 965 current implementation files, leaving 377 across
five groups. All retained-scene implementation/test files have now been read;
clip-bound updates inside filters still warrant an exact metadata-patching
investigation before replacing their full metadata reconstruction. Current
transaction validation, journal-gap reconciliation, deferred resize metadata and
input-domain damage rules preserve required semantics. Actual DXC/ShaderTools
and DX12/Vulkan execution remain unavailable; independent auditing continues.

## Phase 38: remove impossible query domains and duplicate geometry work

Active batch queries receive damage, bins and complete draw membership for the
same current scene. Scene upload validates damage dimensions before querying.
DamageTiles clips and deduplicates its tile list; persistent/transient bin builders
and mutation updates generate draw references from the current scene domain.
Immediate compilation sizes membership to the whole draw arena, while retained
painter metadata owns the same complete domain after fragment updates.

The query nevertheless skipped missing tile and batch indices. Only a test-only
query frontend supplied arbitrary tile 99, truncated membership and duplicate
raw tile lists. Delete that frontend and the production skip branches. Existing
tests now use the actual collect entry with DamageTiles and the owning Canvas
and plan. Exact expected sets replace comparing the query against its own tile
subroutine. Overlapping damage, multi-page and flat bins, sparse/dense selection,
inactive/degenerate draws, repeated queries and generation wrap remain covered;
one redundant deduplication test is removed. A new producer contract test passes
before production edits and covers actual immediate/retained compilation,
hidden layer records, three raster batches, partial edge tiles, clipped oversized
damage and both bin storage modes. This permanently removes an unnecessary
frontend and impossible-domain recovery; no substitute query path is added.

Shared geometry reading also finds two consecutive identical degenerate-edge
checks, with a wrapper whose only action was calling the next emitter. Merge
them into one emission boundary. The new semantic test passes before the edit
and verifies duplicate points, repeated ClosePath, real closing edges and path
identity. Scan tile counting already receives its clipped bbox, so remove its
unused canvas-dimensions argument and the parameters/calculations forwarding
it through normal append, retained storage, offscreen translation and tests.
ScanLinePlan and its epsilon are private implementation details. Actual
zero-area geometry, clipping, numerical boundary rules and GPU guards retain
their meaning; the scan algorithm and emitted path data are unchanged.

Focused shared release tests pass 170 cases. The serial full all-target release
run passes 1077 tests with the same three known failures: two unavailable
pinned-DXC checks and the 1600-pixel turbulence canonical mismatch. Formatting,
release clippy, backend-free compilation and all example builds pass; existing
one library/eight test clippy warnings remain. Metal validation is enabled for
correctness. All 1730 SVG fixture images and seven example outputs match their
immutable baseline pixels exactly. Native presentation completes eight frames
and its 640x360 to 480x270 resize. Both Metal artifact manifests match exactly;
this phase changes no shader source or ABI.

The [actual application measurements](benchmarks/cleanup-phase38-metal-2026-10-12.json)
retain 96 whole-process observations across the SVG fixture application and
native retained-window example. Each workload includes baseline/current and
identical-current control series, two warmups per label and ten alternating
timed pairs. SVG medians change 4285.083 to 4306.222 ms (+0.49%), compared with
+0.15% in its same-version control. Window medians change 233.174 to 232.836 ms
(-0.15%), compared with -0.03% in its control. These applications show no material
central timing regression; the observations do not establish an isolated query
or scan speedup. Nearest-rank p95 is the maximum of ten samples; raw timings and
both tails remain in the report.

Timing includes initialization, rendering/completion and teardown; SVG also
includes parsing, readback and PNG saving. Verification follows each stopped
timer: all 83040 measured PNG hashes match the immutable baseline, and all 384
window frames complete with the expected resize. The SVG source manifest is
unchanged. Timing disables Metal validation. The window explicitly pins registry
ID `0000000100000575`; SVG uses the default on the freshly verified single-GPU
Apple M5 host. Temporary validation scripts and copied executables are removed.

The audit has read 620 of 966 current implementation files, leaving 346 across
four groups. Shared implementation/test files are now fully read. Historical
inspection confirms that RectShadowOptions is only a compatibility alias for
ShadowOptions; migration to the canonical type is next. Image upload also models
an optional scene store although its only caller always supplies one. Offscreen
clip exclusion still needs producer proof before removing its error wrapper.
Actual DXC/ShaderTools and DX12/Vulkan execution remain unavailable; independent
auditing continues.

## Phase 39: canonical shadows and complete image-upload snapshots

Remove RectShadowOptions, the compatibility alias retained when commit 74282fc9
introduced ShadowOptions. Rectangle, circle, arc, line and callout shadow APIs,
examples and tests now use the same canonical type. Both website languages
explain the shared options. The shadow data and calculations are unchanged.

Image upload has one constructor whose only caller always supplies both resource
stores. Remove the optional scene branch, duplicate selected-texture limit checks
and impossible first-page default. Every upload owns either packed atlas pages
or its initialized transparent page. Remove derived Default, which created an
invalid zero-page upload, and the native consumer's second empty-atlas creation
path. Recording now stores the upload and its signature together in one optional
cache entry; absence before the first frame no longer needs a fake upload.
Tests construct empty uploads through the real store producer.

Shelf packing receives selected images that fit a fresh page including their
one-pixel guard. Seed each page with its first item and remove repeated fit
filtering, empty-packer retries and impossible silent skips. Device budgets,
missing-image transparency, renderer/scene namespaces, weak identity guards,
unchanged-pixel reuse and vector ownership retain their required semantics.
Split the 1049-line resource file into stores/identity (222 lines), uploads
(363), atlas packing/copying (213), and separate semantic tests. This permanently
removes redundant states and branches; no substitute compatibility path is added.

Three new semantic tests cover overlapping namespace keys, resource removal to
empty, page dimensions 0/1/2/3/7/8/9/31, zero/one/multiple page budgets, exact
standalone texture limits and contiguous placement indices. All 21 image tests
pass before and after the production edit. Native image tests pass 11 cases,
recording tests 20 and shadow tests eight. The serial full all-target release run
passes 1080 tests with the same three known failures: two unavailable pinned-DXC
checks and the 1600-pixel turbulence canonical mismatch. Formatting, release
clippy, backend-free compilation and example builds pass; existing one library/
eight test clippy warnings remain. Website build dependencies remain unavailable.

With Metal validation enabled, all 1730 SVG images and seven example outputs
match the immutable baseline exactly. Presentation completes eight frames and
the 640x360 to 480x270 resize. The two Metal artifact manifests and backend-free
empty manifest are unchanged; this phase changes no shader source or ABI.

The [application measurements](benchmarks/cleanup-phase39-metal-2026-10-12.json)
retain 96 whole-process observations, including baseline/current and identical-
current controls, two warmups per label and ten alternating timed pairs. SVG
medians change 4277.914 to 4297.932 ms (+0.47%), with -0.02% in the same-version
control. Window medians change 232.569 to 232.700 ms (+0.06%), with -0.03% in its
control. These applications show no material central timing regression; the
measurements do not establish an isolated packing speedup. Nearest-rank p95 is
the maximum of ten samples; raw timings and tails remain in the report.

Timing includes initialization, rendering/completion and teardown; SVG also
includes parsing, readback and PNG saving. Verification follows each stopped
timer: 83040 PNG hashes match, 384 window frames complete, and SVG inputs remain
unchanged. Timing disables Metal validation. Window explicitly pins registry ID
0000000100000575; SVG uses the default on the freshly verified single-GPU Apple
M5 host. Temporary scripts and copied executables are removed.

The audit has read 684 of 970 current implementation files, leaving 286 in native
runtime, shaders and the SVG entry. All render scheduling and DX12 implementation
files have been read. DX12 lock-poison recovery and packed uniform handle absence
need focused policy/producer tests. Sweep shaders also repeatedly add/subtract
TAU toward unrestricted public start angles, risking excessive work or a stalled
float increment; a bounded arithmetic fix with semantic regressions is next.
Actual DXC/ShaderTools and DX12/Vulkan execution remain unavailable.

## Phase 40: bounded sweep gradient evaluation

Public peniko sweep constructors store unrestricted start/end angles, and the
brush previously copied them directly. Both native shaders repeatedly added or
subtracted TAU toward the start. Large starts could take excessive work or stall
when f32 increments stopped changing. Negative starts also failed periodicity:
a sweep from -7 to -3 radians sampled white where the independent angular
reference expects gray 46. That GPU regression and a host upload contract test
both fail before the fix; dangerous huge inputs are not submitted to the old loop.

Normalize the start once on the CPU in f64, into [-PI, PI], and store the original
signed span separately. The private brush parameter formerly containing the end
now contains that span; encoding and decoding change together, without a legacy
format path. Both Metal and HLSL replace their loops with at most one turn
correction. Wide subtraction also handles opposing finite f32 extrema without
losing the span before conversion. A span overflowing f32 becomes signed infinity,
whose bounded angular numerator evaluates to the ramp start at pixel precision.
Nonfinite public angles are explicitly rejected by a documented panic; decoded
records reject noncanonical orientation and NaN spans. This fixes the algorithm
and its data contract permanently, without an iteration cap or recovery shader.

Four focused tests pass after the edit, plus 19 broader brush tests. GPU tests
check 420 samples against an independent wide angular/color reference across
positive, negative, reversed, narrow and zero spans, centers, ten directions and
Pad/Repeat/Reflect. Inputs around 1e30 and opposing f32 extrema now complete.
Host tests retain signed spans and exact encoded round trips. After removing the
temporary measurement target, final serial all-target release tests pass 1084
cases with the same three known failures: two unavailable pinned-DXC checks and
the 1600-pixel turbulence canonical mismatch. Formatting, release clippy,
backend-free compilation and example builds pass; the existing one library/
eight test clippy warnings remain. HLSL source is updated, but actual DXC and
DX12/Vulkan execution remain unavailable. Both website languages document the
sweep angle contract; website build dependencies remain unavailable.

With Metal validation enabled, 1730 SVG images and seven example images match
the immutable baseline exactly. The existing 720x520 gradient scene also matches
baseline pixels after 60 immediate frames and readbacks. Presentation completes
eight frames and the expected resize. Both Metal artifact manifests change
consistently with the new shared sweep source; the backend-free manifest remains
unchanged. No Metal ABI binding layout changes.

The [application report](benchmarks/cleanup-phase40-metal-2026-10-12.json)
retains 144 whole-process observations: baseline/current and identical-current
controls for the existing gradient scene, SVG fixture application and native
window. Each series uses two warmups per label and ten alternating timed pairs.
Gradient medians change 62.284 to 61.730 ms (-0.89%), while its same-version
control changes -1.95%. SVG changes 4298.391 to 4267.375 ms (-0.72%), control
+0.04%. Window changes 236.718 to 233.573 ms (-1.33%), control -0.49%.
The measured applications show no material central timing regression; short
process timing varies substantially, so these observations do not establish an
isolated shader speedup or every sweep workload's performance. Nearest-rank p95
is the maximum of ten samples; all raw observations and tails remain available.

Timing includes setup, rendering/completion and teardown. Gradient repeats the
unmodified examples/common/gradient_scene.rs through NativeRenderer with 60
completed immediate readbacks and saves the final PNG; SVG includes parsing,
readback and PNG saving. Verification follows each stopped timer: 83040 SVG PNG
hashes, 48 gradient outputs, 2880 gradient frames and 384 window frames pass.
SVG source hashes remain unchanged. Timing disables Metal validation. Gradient
and window pin registry ID 0000000100000575; SVG uses the default on the freshly
verified single-GPU Apple M5 host. Temporary wrapper/source copies, scripts and
measurement executables are removed.

The audit has read 762 of 971 current implementation files, leaving 209 across
native runtime, shaders and the SVG entry. All coarse and fine shader files have
now been read. Repeated coarse stack validity skips, fine specialization's
unexpected-tag reinterpretation and constant-zero HLSL text math are recorded
for producer/caller proof and focused tests. Current fine particle interpreters
perform clip/group/path/text compositing; they are unrelated to the removed
custom-shader expression interpreter. The SVG entry has only been read through
line 220 and is not counted as complete. Independent auditing continues.

## Remaining audit work

- Continue the 209 pending native runtime, shader and SVG-entry reads. Prove coarse stack emitter inputs, fine specialization membership across reused slots and constant-zero HLSL text callers before removing their redundant paths. Test DX12 lock-poison policy and prove packed uniform handle presence. Trace exact retained metadata patching for clip-bound updates inside filters and prove offscreen clip exclusion before removing its error wrapper. Necessary SVG paint/font behavior, degenerate geometry semantics and unsafe GPU ownership/bounds validation must remain correct; a keyword match alone does not establish an obsolete implementation.
- Validate Vulkan cache construction with an actual driver; controlled native-call tests and Windows type checks do not replace GPU execution.
- Execute the HLSL editor test against the actual ShaderTools language server; local protocol regressions pass, but the server is unavailable locally.
- Execute the three Win32 geometry regressions and DX12/Vulkan shutdown smoke on Windows; the current Rust type checks do not verify those runtime paths.
