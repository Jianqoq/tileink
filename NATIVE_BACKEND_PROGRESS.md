# Native backend implementation record

Status: **M0/M1 complete. Windows M2 toolchain and M3 minimum native adapters complete.
The full NativeRenderer/Canvas shader inventory remains M4; Mac hardware validation is deferred.**

Current Windows M3 record: [implementation and exact acceptance](docs/native/m3-completion.md),
[verification receipt](docs/native/m3-completion-verification.json). Shared batches, owning receipts,
native textures, pre-recording validation and failure containment execute on both APIs.
All 306 minimum cases repeat three times across four real APIs with exact bytes.
Older continuation entries below are historical snapshots, not the current milestone status.

On 2026-09-13 the user instructed: **“不用比较性能了”** (stop performance comparisons). For this M0/M1 closeout, no further Criterion comparisons, resize timing or telemetry are required. Performance is not an acceptance gate for this delivery, and `performance_accepted` remains false. Historical regressions, rejected experiments and incomplete timing runs remain preserved. This does not waive functionality, exact pixels, feature/dependency checks, release tests or code review.

Selected source: `target/backend-parity/m1-new-batch-walk-1/current/source`.
The [M1 closeout](docs/native/m1-closeout.md) is the authoritative current record;
older pending/candidate statements below describe archived snapshots.

| Current M1 checkpoint | State |
| --- | --- |
| Optional default wgpu feature and CPU-only build | Implemented; 938 default and 620 CPU-only release tests pass |
| Shared preparation, execution, filters, damage and history | Implemented, with focused semantic regressions |
| Feature/platform/native dependency checks | 21 feature builds across Windows, Linux x86_64 and macOS x86_64; bench-internals all-target check, 3 dependency audits and aggregate contracts pass; cross checks do not certify hardware execution |
| Native constructor contracts | Single-feature and aggregate unavailable-backend tests pass; no native renderer is claimed |
| Latest exact pixel reference | 15,448 outputs, 6 fresh processes, zero different pixels; runtime/precompiled SVG/example/retained paths |
| Earlier complete renderer/supplemental regressions and reviewed PNGs | Preserved with their source ancestry and human review receipt |
| Performance comparisons | Stopped by user; not accepted and not a delivery gate |
| Final integration and release/fmt/Clippy | Complete; extra CPU-only `-D warnings` limitation disclosed in closeout |
| Final review and commit/push | Review complete; delivery commit is the commit containing this record |

## Windows M2 continuation — 2026-09-13

The user-confirmed M0/M1 push resolves to `90151cbd`. Windows HLSL→DXIL/SPIR-V
builds now use persistent content-addressed shader artifacts, ABI reflection and
include/toolchain invalidation. Four minimum probes (42 cases) execute on all four
real APIs with exact bytes; native pipeline caches also hit across processes.
See [build/cache contract and current scope](docs/native/m2-windows-shaders.md).

This does not complete the full native renderer. M2 Metal compile/GPU validation
is deferred because the user has no Mac; Windows comes first. The 179 full renderer
program/variant inventory remains explicitly unported. M3 production adapters and
texture/numerical coverage, M4–M6 still require implementation and acceptance.
No performance comparison was resumed.

## Windows M3 queued execution continuation — 2026-09-13

M2 commit `e8631bf5` is pushed and the remote was confirmed. The executing native
verification adapters now separate submit/readback and keep each frame's command,
descriptor and buffer resources until completion. Both native APIs pass 42 queued
cases with reverse-order readback, ticket/device rejection and unread-frame
teardown; the four-API minimum output contract remains exact. Details and current
limitations are in [M3 queued submissions](docs/native/m3-queued-submissions.md).

This is a tested M3 execution/lifetime slice, not production NativeRenderer or full
M3 completion. Production shared-adapter integration, texture/numerical probes and
error recovery remain open. Mac verification is still deferred; no timing work
was resumed.

## Historical evidence

The current boot's recorded RTX 4090 LUID is `9f3f010000000000`; historical
`bf3f010000000000` records below retain their original session identity.
Full GPU completion is not inferred from partial passing counts, and neither
WGPU texture mode is described as a native API implementation.

The following M0 record describes the main working tree, which remains the
performance baseline while the M1 candidate is validated.
The production source is back to the **V13 scalar-blur implementation**.
The V14 pixel-interval candidate was rejected after forward/reverse Criterion
runs showed no consistent benefit (native diagonal +3.23% in the forward round).
The temporary boundary probe checked 5618 line configurations and 1,438,208
pixels per API; all four focused coverage tests passed. The candidate-only
probe is archived with the experiment, since the production shader again uses
the same integer rule as its reference. Existing analytic coverage regressions
remain in the suite. Experiment results are retained in
`target/backend-parity/m0-v14-interval-criterion-1/summary.json`.

## Reference and scope

- Original implementation reference: `eabbe0b97b392582d663206c1f2aad51f76695aa`;
  plan commit: `29396c32`; this numerical/retained batch starts at `da70e7fe`.
- GPU: NVIDIA GeForce RTX 4090, vendor 4318, device 9860,
  DX12/Vulkan LUID `bf3f010000000000`.
- Drivers: DX12 `32.0.16.1062`, Vulkan NVIDIA `610.62`.
- DXC: Windows SDK 10.0.26100.0 x64, `dxcompiler.dll` version `1.8.2502.11`.
- WGPU 30.0.1, maintained HAL 30.0.0 at `vendor/wgpu-hal`.
  [Shared HAL maintenance](WGPU_PATCHES.md) describes provenance and consumers.

In M0, four routes mean **WGPU DX12/Vulkan × native/portable texture execution**.
They do not mean the future four native/WGPU API implementations. Native feature
splitting and shared execution have an isolated M1 development draft, described
below; they are not integrated or accepted. Maintained HLSL and direct adapters
(M2–M6) remain unimplemented. Only the NVIDIA device above has been tested; required
AMD/Intel and applicable Linux/macOS hardware certification remain outstanding.

Every runner invocation creates a fresh directory. Its manifest records expected
cases/frames/routes, source and binary hashes, compiler files, actual GPU identity,
capabilities, pipeline variants and resources used while parsing. Inputs, decoded
images and fonts are fixed; runtime resource content/membership is revalidated.
Raw RGBA comparison includes alpha and RGB under alpha zero, with row padding
removed. Missing routes, frames, resources or compiler provenance fail certification.
`complete` alone is never a passing result.

## Historical M0 correctness evidence

The following are completed runs of the V13 scalar-blur candidate plus the latest
retained history fixes. Artifacts are under `target/backend-parity/`.

| Check | Result | Evidence |
| --- | --- | --- |
| Complete runtime SVG reference | 1712/1712 frames, four WGPU routes, zero differing pixels/channel delta | `m0-v13-svg-runtime-1/report.json` |
| Complete runtime example reference | 45/45 frames, four WGPU routes, zero differing pixels/channel delta | `m0-v13-examples-runtime-1/report.json` |
| Retained runtime reference | 29/29 frames × 24 route/target/Auto–ForceFull variants, zero differences | `m0-retained-halo-runtime-1/report.json` |
| Retained precompiled-fine reference | 29/29 frames × 24 variants, zero differences; actual embedded DXIL required | `m0-retained-halo-precompiled-1/report.json` |
| Focused cross-API GPU regressions | All 16 explicitly enabled tests passed (389.94 s) | `m0-v13-all-parity-gpu-tests.log` |
| Incremental damage CPU regressions | 21 passed, including root/filter/isolate/mask deletion domains | `m0-journal-nested-removal-cpu-green.log` |
| Partial blur halo GPU regressions | Two tests passed in each WGPU texture mode, including poisoned scratch | `m0-blur-halo-gpu-native-green.log`, `m0-blur-halo-gpu-portable-green.log` |
| Vertical halo tile coverage | Zero-size, partial tiles, sparse columns and extreme radius passed | `m0-blur-halo-tiles.log` |
| External output/recovery regressions | 16 native target tests and both portable owned-switch tests passed | `m0-owned-recovery-v4-native.log`, `m0-owned-recovery-v4-portable.log` |
| Ordinary SVG / examples | 1712 SVGs and 45 examples completed; native/portable pixel comparisons passed | `m0-v13-ordinary-svg.log`, `m0-v13-ordinary-examples.log` |
| Current PNG review | 3488 PNGs: 3307 byte-identical, 181 pixel changes / 4695 pixels; human acceptance pending | `m0-v13-png-review/review.md`, `summary.json` |
| Serial release suite | All 12 groups passed with GPU tests enabled in both texture modes | `m0-v13-release-groups.log` |
| Formatting / all-target release Clippy | Renderer, measurement tools and final state-module split passed | `m0-v13-state-split-clippy.log` |
| State-module split | Existing 29-frame transaction regression passed; Standards/Spec reviews found no remaining issue | `m0-v13-state-split-cpu.log` |
| Strict repository PNG comparison | Expected failure: exactly 181 changed images; no additional paths or encoding-only differences | `m0-v13-strict-png-comparison-2.log` |
| Default Windows without DXIL precompilation | Library, parity runner, resize profiler and retained benchmarks compile | `m0-v13-precompile-disabled-check.log` |
| Linux / macOS compilation | Library, tests, parity runner, resize profiler and retained benchmarks compile without warnings | `m0-v13-reviewed-x86_64-{unknown-linux-gnu,apple-darwin}-check-3.log` |
| Package verification | 182-file package created and Cargo's package build passed; no publication | `m0-v13-package.log` |

The 16 focused tests precede the final retained halo fix; the complete runtime SVG,
example and both retained runs above include that fix. They cover numeric raster
inputs, blur radii 0/1/15/16/17/255/256/257, tiny sigma, mixed backdrop, coverage,
gradients, pattern transforms, lighting and repeated filters. The ordinary grouped
release suite and reviewed measurement-tool checks passed. Runtime/precompiled and independent repeat coverage is complete: all 18 reports
passed. Across three runtime plus three precompiled runs per corpus, 46,344 PNGs
were verified against recorded digests and compared in full, with zero differing
files. Resource and frozen-font inputs also match. Evidence:
`m0-v13-final-reference-repeats-1/{runs,cross-run-summary}.json`. These frozen runs
precede only the extra external-subresource validation guard described below;
the shader/rendering algorithms are unchanged. Fresh ordinary
PNGs are now captured in the current 181-image review. The original 177 images
remain byte-identical to the previous review; four additional blur-related examples
add only 25 RGB differences of 1, without alpha changes.

## Numerical changes and their causes

These changes correct production semantics. None uses fixture detection, output
readback repair, per-backend expected pixels or a tolerance.

- **Pattern coordinates:** compensated products prevent cancellation residue from
  selecting a texel across a repeat seam. The original 115-pixel cross-API failure
  became zero. This earlier batch was committed, its two changed PNGs accepted,
  and its four Vulkan Criterion cases classified as unchanged/noise.
- **Coverage:** explicit line-intersection evaluation and a stable clipped-trapezoid
  integral avoid narrow-span cancellation. The old formula changed analytic alpha
  124.4966 into 125; the corrected CPU/GPU regressions produce 124. Ordinary fine
  coverage and filter clips share `shared/coverage.wgsl`.
- **Gradients:** interpolate stored byte channels directly and fix transform
  evaluation order. This preserves half-channel rounding and passes the 393216-case
  integer oracle on both APIs.
- **Turbulence:** explicit gradient-dot and scalar interpolation fusion gives the
  same result for the reproduced seed-zero lattice on both APIs.
- **Lighting:** alpha samples accumulate in byte units; dot products use explicit
  evaluation order. Specular lighting computes the unnormalized dot divided by
  both lengths once, avoiding differing component-normalization roundoff. Both
  runtime-parameter probes and original SVG fixtures guard the result.
- **Blur:** scalar Gaussian recurrence uses explicit product-rounding boundaries.
  V13 hoists complete-support bounds checks to the pixel entry, preserving scalar
  tap/weight order. Boundary pixels retain the general path and small radii retain
  the existing shared-tile kernel. Performance is measured separately below.

Workgroup weight-cache candidates V8–V12 were rejected. Although one experiment
reduced stage time, the 1600-pixel blur still differed at (1105,419): alpha 100/99.
Writing intermediate taps changed that result, implicating compiler evaluation
rather than establishing a portable cached algorithm. V13 contains no such cache.
Temporary probes are saved under ignored `target/backend-parity/`, not registered
as permanent tests. Their timings are not accepted performance results.

## Resource identity and retained history corrections

| Root cause | Production correction and regression |
| --- | --- |
| Pointer-derived image signatures survived source-store destruction/address reuse or `Rc::make_mut` | Weak references retain allocation identity without retaining image contents; deterministic copy-on-write/identity regressions protect cache invalidation. No per-frame pixel hashing. |
| External targets reached GPU validation without required effect-read usages | Prepared metadata caches actual root read requirements; direct-output validation returns a typed error. Copy-only history output remains valid. A rejected output does not touch destination pixels or commit history, although preparation may upload renderer-owned buffers. |
| Switching from successful external output to owned output reused the wrong history strategy/allocation | Owned entries select internal history before damage planning and independently ensure owned target dimensions. Tests cover success/rejection recovery, size changes and cached preparation. |
| Portable partial offscreen execution copied unwritten scratch pixels over preserved history | Copy back only coalesced, clipped active regions; retain the full copy for full frames. A poisoned-scratch 129×65 regression covers interior and edge tiles. |
| Journal deletion damage referenced an ID with no current painter position | Deleted patches add unattributed old bounds, matching full scene-diff semantics. CPU and GPU regressions remove/reinsert a clipped leaf before backdrop blur. |
| Nested filter/isolate/mask traversal discarded unattributed old pixels | Seed each local dependency domain with pending unattributed damage, while keeping ordinary outer painter damage isolated. The deletion regression runs in all four domains. |
| Compact partial blur dispatched horizontal intermediates only for final dirty tiles | Vertically expand the horizontal worklist by the vertical kernel halo, then restore the original list for the vertical pass. Each encoded list has independent arena storage; sparse column gaps remain sparse. Poisoned unused scratch reproduces the former stale-row reads. |

The first full retained run exposed portable history corruption. After that correction,
delete/reinsert frames exposed journal damage loss and finally 159 stale blur pixels
at the next tile row. Each confirmed defect has a regression that failed before its
fix. The complete 29-frame runtime and precompiled references now pass, including
DPI changes, resize, target replacement, static reuse and actual partial updates.

## HAL ownership and synchronization

The HAL migration is already committed: Tileink owns the sole maintained copy;
gfx_ui and the trading application point their Cargo root patches to it. No HAL
source changes are part of this numerical/retained batch.

The earlier DX12 fix emits a UAV barrier when dependent texture accesses remain in
`UNORDERED_ACCESS`, including write-only accesses. Without it, repeated writes left
nonzero pixels after clear. The HAL GPU regression and 32-frame repeated-filter
reference pass with it. No extra CPU wait, submission or copy was added.

The existing one-pair Criterion result was 90.28 → 87.66 µs. Eight pairs measured
169.01 → 211.46 µs (+25%); the old binary also failed its final pixel check because
it omitted required ordering. Preserve this historical ordering cost explicitly:
it is already part of the correct `da70e7fe` baseline used for this batch, and is
not an unresolved regression of the current changes. The incorrect old output
cannot serve as a faster alternative. Further optimization must preserve the
required dependencies; this measurement is not an application-gain estimate.

The completed ownership batch also passed 15 HAL tests; Tileink's 12 serial test
groups; 1712 ordinary SVGs and 45 examples; all 3488 unchanged repository PNGs;
1720 gfx_ui and 1291 application test executions; and formatting/Clippy/package
checks. Those results belong to the committed ownership batch, not this dirty batch.

## Performance status

The complete V13 numeric Criterion matrix finished: 48 comparisons, with 8
improvements, 17 unchanged, 15 within the noise threshold and 8 regressions. Each
comparison used 20 samples, 2 s warmup and 4 s measurement; all timings include
GPU completion. Evidence: `m0-numeric-criterion-v13/` and
`m0-numeric-criterion-v13-summary.json`. The initially flagged cases and their controlled follow-ups are recorded below.

All four 1600-wide blur cases improved in complete-frame time:

| API / texture execution | Before → after (µs) | Mean change |
| --- | --- | --- |
| Vulkan / native | 2584.28 → 2377.45 | −8.00% |
| Vulkan / portable | 2481.25 → 2252.78 | −9.21% |
| DX12 / native | 3017.78 → 2519.27 | −16.52% |
| DX12 / portable | 2974.18 → 2508.25 | −15.67% |

The initial run flagged eight cases for controlled repeats and stage diagnosis: Vulkan native
`diagonal-300` (+4.71%), `lighting-1600` (+3.78%); Vulkan portable
`diagonal-300` (+3.00%), `star-clip-300` (+3.11%), `star-clip-1600` (+4.15%),
`blur-300` (+2.08%); DX12 native `diagonal-1600` (+1.92%),
`lighting-300` (+3.47%). These original observations remain recorded; the
controlled follow-ups below determine whether they reproduce.

Controlled repeats of the eight flagged cases are now complete. The ABBA order
used baseline/current followed by current/baseline, preserving each 20-sample
Criterion run. After inverting reverse comparisons, 16 results are: 1 improved,
7 unchanged, 6 within noise and 2 regressed. The significant cases at that stage
were Vulkan native `diagonal-300` (+2.88%, reverse round) and Vulkan portable
`star-clip-1600` (+2.20%, reverse round). Both were unchanged in the forward
round, prompting the longer protocol below. Neither result was discarded.
Evidence: `m0-v13-regression-abba-1/summary.json`.

The predeclared longer repeat/control protocol is complete: 60 samples, 5 s
warmup and 15 s measurement, with the same significance and noise thresholds.
The three Vulkan cases (native diagonal and both portable star-clip sizes)
produced **1 improved, 2 unchanged and 3 noise classifications**, with no
regressions across forward/reverse comparisons. The current and baseline
same-binary controls also show only unchanged/noise classifications. Together
with the preceding eight-case ABBA, the initially flagged regressions have no
unresolved significant result in these controlled follow-ups: all eight cases
received ABBA, and only the three named Vulkan cases received the longer
protocol. Earlier failed results remain recorded above. Evidence:
`m0-v13-long-numeric-controls-1/summary.json`; binary/source records are in
`m0-v13-cli-binaries.json`. CLI sampling parameters were checked in every log.
The restored V13 production source also passed release builds, formatting and
all-target Clippy (`m0-v13-cli-{fmt,clippy}.log`).

The separate 256-frame stage profiles are diagnostic means, not production PMax
samples. Vulkan portable `blur-300` showed a fine-stage mean increase but did
not reproduce a complete-frame regression in either ABBA direction. DX12
`diagonal-1600` likewise did not reproduce a significant complete-frame
regression despite its diagnostic scan mean increase. The profiles are retained
in `m0-v13-reviewed-regression-profiles-1/stage-comparison.json`; CPU/GPU stage
means must not be treated as a unique causal attribution without the frame data.

Fixed binaries use identical benchmark input sources and completed GPU frames:

- Baseline `numeric-raster-v7-baseline.exe`, SHA-256
  `2ef31fc16caa3934419b1e9f6de454691b0b43f86ff5ab1f4f38de882efa3dc3`.
- Current `numeric_raster-v13-halo-current.exe`, SHA-256
  `024c5c514af2f1fe62cdd764615823106a3697197f591dadffce4b7ef3b5240d`.
- `retained_portable_history` measures complete four-mutation cycles of partial
  offscreen history at 300×201 and 1601×1001. Two cycles are checked frame by
  frame against ForceFull before timing; the final frame is also checked.
- `retained_resize` measures a complete 64-frame grow/shrink cycle, with an
  independent ForceFull check of phase zero and all transitions before timing.
- Both maintained benchmarks require the physical GPU identity explicitly.
  `wgpu_resize_profile` measures production frames with profiling disabled,
  then collects GPU stages in a separate diagnostic pass. Its PMax partition
  comes only from the actual slowest production frame.

All GPU workloads run serially; Criterion excludes parsing, compilation and readback
and includes GPU completion. No numerical, HAL or native-API speedup is claimed
until the corresponding correct-output comparison passes. The completed resize
P50/P95/PMax baseline and stage attribution are reported below; these workloads
do not measure application swapchain configuration.

## Completed retained Criterion comparisons

The reviewed resize/history workloads completed both APIs in ABBA order:
16 comparisons, **8 unchanged, 6 within noise and 2 improved; no regressions**.
Every resize iteration contains the complete 64-frame cycle; every partial-history
iteration contains all four mutations. The original and current binaries passed
all pre-timing and final pixel checks. Results, confidence intervals, cycle costs
and derived per-frame means are in
`target/backend-parity/m0-v13-reviewed-retained-criterion-1/summary.json`.

The two Vulkan history improvements did not repeat in both directions, so this
is evidence against a measured regression in these workloads, not a stable
percentage-speedup claim. Numeric-raster's longer sampling/control protocol is
also complete, as recorded above; thresholds and full-frame completion were
unchanged. Moving its existing defaults to Criterion's
global config makes CLI overrides effective without changing default conditions.
Both benchmark source copies match, and focused Standards/Spec reviews passed.

## Completed V13 resize baseline

`m0-v13-resize-abba-1/summary.json` preserves eight independent process runs in
baseline/current/current/baseline order per API. Each process verifies every
resize transition, then measures 256 production frames per texture mode with
profiling disabled. There are 4096 production frames and a separate 4096-frame
diagnostic pass. The size sequence and exact wall-time partitions were checked.

Pooled results below use 512 production frames per version/API/texture route.
These target resizes do not include a window or swapchain.

| API / texture execution | Baseline P95 / PMax (ms) | V13 P95 / PMax (ms) | V13 PMax wait share |
| --- | --- | --- | --- |
| Vulkan / native | 1.087 / 1.505 | 1.138 / 2.105 | 87.0% |
| Vulkan / portable | 0.947 / 1.556 | 0.874 / 1.385 | 75.2% |
| DX12 / native | 1.491 / 4.238 | 1.538 / 4.778 | 82.8% |
| DX12 / portable | 1.162 / 1.603 | 1.132 / 1.606 | 67.6% |

Individual runs remain visible: for example, V13 Vulkan native PMax was
2.105 ms and 1.071 ms in its two runs. Its slowest-frame partition shifted from
87.0% completion wait to 70.0% render/record/submit. The wait interval is the
wall time of `device.poll`; it may include thread wakeup and completion-time
cleanup, and is not a GPU shader timestamp. These runs establish the baseline
and variability; they do not show that PMax has been reduced to P95 or prove
that one kernel caused every spike. V14 was rejected before resize measurement.

## Existing Criterion baseline completion

The five original benchmark entries now explicitly select and record the API,
physical GPU, compiler, feature set and original memory policy. Original profiled
series are preserved for diagnosis. New production measurements disable profiling
and include transaction/render/completion; paired mutations and fixed DeltaRotation
traces prevent sample-count-dependent workload weighting. The matrix contains 761
entries per API. Its definitions and units are in [BENCHMARKS.md](BENCHMARKS.md).

Three CPU inventory/sampling regressions each failed before correction and pass
in the real `benchmark_matrix` integration harness. Formatting, all-target release
Clippy and both review axes passed. The first full matrix stopped after 75/390
Vulkan scale cases: the unchanged production baseline panicked in `reorder/100`.
The frozen current binary reproduces the same crash during continuous sampling;
Criterion test mode's single iteration does not expose it. The original failed
runs and binary/source records remain in `m0-existing-baselines-1/` and
`m0-existing-baselines-binaries-1.json`. No performance pass is inferred from this
incomplete matrix. The predeclared protocol is `m0-existing-baseline-protocol.md`.

A permanent CPU regression reorders overlapping scene nodes beside empty groups,
both directly under the root and inside a group, over 600 consecutive frames. It
explicitly verifies that sibling order-key rebalancing changes the empty Group.
The same regression failed at the missing chunk lookup in both original baseline
and current code, then passed with the correction. Structural groups do not own
draw chunks; the topology frame patch now resolves chunks only inside Scene/Layer
branches, preserving missing-drawable invariant checks.

All 44 retained CPU tests and 49 retained GPU tests per texture mode passed; the
strengthened rebalance-specific CPU assertion also passed. The GPU regression
checks every pixel, including the overlap and transparent borders. Standards and
Spec reviews found no remaining issue; formatting, all-target release Clippy,
Linux/macOS selected-target compilation and package verification passed. The
final strengthened GPU check passes in both texture modes. Final 12/12 release
groups, full SVG and examples also pass. The strict comparator reports exactly
the existing 181 changes, all byte-identical to the pending review; no new PNG
changes (`m0-reorder-final-review-audit.json`). All final logs and copied full
process logs use the `m0-reorder-final-*-2` prefix. All six final retained
reference runs passed (runtime/precompiled × three): 4176 full PNG byte
comparisons against the preceding fixed reference have zero differences, with
GPU, compiler, resources, font inputs and recorded digest integrity checked.
Evidence: `m0-reorder-final-reference-repeats-1/cross-run-summary.json`.
The full Criterion timing matrix is running serially. Its Vulkan scale baseline passed all 390 single-iteration preflight cases; remaining duplicate preflight is consolidated into the unchanged complete timing run. The partial current preflight and stop record are retained, without claiming a full preflight pass.

New immutable binaries are recorded in `m0-existing-baselines-binaries-2.json`,
including full production-source manifests. The baseline is explicitly
`da70e7fe + Group correction`, with the exact patch and original failure logs
preserved. Its 390 Vulkan scale entries pass Criterion test mode for both revisions.
Preflight 2 was intentionally stopped after 6/78 baseline dirty-ratio cases, with
the busy thread repeatedly sampled inside NVIDIA shader compilation. The partial
outputs remain under `m0-existing-baseline-preflight-2/`. Test-mode success is not
a performance verdict. Timing will use a fresh output directory and the
unchanged complete matrix/protocol after the correctness preflight. One baseline
CPU rerun initially reused the old executable because Copy-Item retained an older
source timestamp; after a verified source hash and forced rebuild, the corrected
baseline passes (`m0-reorder-baseline-cpu-after-2.log`).

## Benchmark setup cache

Independent benchmark Renderers now share only an optional process-local pipeline
cache, preserving fresh scene/materializer, buffers, output and history per batch.
Both revisions use identical helpers and cache policy. Device cache capability and
actual cache use have separate metadata; APIs without support record no cache.
Default diagnostics request their device directly instead of discarding a full
Renderer. Strict tests use the same API/LUID/DXC selector as measured benchmarks.
A Vulkan Auto dirty-ratio probe completed all 13 cases in 127.65 seconds; the
previous uncached preflight stopped after six cases; the busy thread accumulated
about 619 CPU seconds and all eight instruction samples were in the driver compiler. This demonstrates a useful setup change, not a production frame speedup.
Original sampling and all 761 cases/API remain. Matrix 4 freezes reviewed helpers
and retains previous aborted runs; its full correctness/performance result remains
pending. The bench-internals missing-import check failed before correction.
All four strict context routes and the ordinary default route pass the independent
Renderer output/journal test. Vulkan records a shared pipeline cache; DX12 on this
setup does not expose that feature and records no cache. All-target release Clippy
including bench-internals, focused CPU tests, Linux/macOS cross-compilation and
both review axes pass for the finalized helpers. These are setup/semantic checks,
not an additional production performance claim.

## External target subresource validation

Two new GPU regressions reproduced validation panics when an array or multi-mip
texture was accepted as a D2 storage output. The output API addresses a complete
single-layer/single-mip image; it now rejects unsupported ranges before view
creation, history planning or destination commands, with actual counts in the typed
error. This closes an input-validation gap; it adds no GPU work or shader changes.
Both texture execution modes pass the new rejection and owned-output recovery
checks. Evidence: `m0-destination-subresources-{before,after-native,after-portable}.log`.
Broader target tests passed 18/18 in each texture mode, and the recovery tests
now draw a nontransparent shape and verify every interior/border pixel. Full
SVG/examples, formatting and all-target release Clippy passed. The strict PNG
comparison still reports exactly the existing 181 changes, and every candidate
SHA-256 matches the pending review (`m0-output-guard-review-audit.json`); there
are no new PNG changes. Both review axes found no remaining issue.

## Remaining M0 gates

1. Independent complete references and runtime/precompiled coverage passed (18
   reports, 46,344 byte-identical PNGs); all manifests and original runs are retained.
2. All 12 release groups, ordinary SVG/examples and the strict PNG comparison
   are complete. The comparator reports exactly the 181 images in the current
   human review; acceptance remains pending.
3. Numeric-raster, retained resize/history Criterion comparisons and resize
   distributions with the maximum frame's stage breakdown are complete. Preserve
   the original observations and known HAL ordering cost. Record the plan's
   existing `retained_scale`, `retained_dirty_ratio`, `retained_stress`,
   `root_batches` and range-scatter/upload baselines before M1.
4. The [program/ABI inventory](docs/native/wgpu-reference-inventory.md) is refreshed
   and Naga-validated for V13 (20 variants / 179 entrypoint instances). Windows
   precompilation-disabled, package and Linux/macOS compilation checks passed.
   Independent Standards/Spec reviews found no remaining actionable issue in
   the renderer, measurement tools and platform state-module split. Any further
   fixes require focused review and validation.
5. Obtain the required human review of the final concrete PNG changes before
   committing/pushing this rendering batch. Prior acceptance covers only the
   already committed two-PNG pattern batch.

Only after M0's exit condition is met will the plan advance to integrating the
native feature and shared-renderer work; the isolated draft does not advance it. Missing required hardware stays explicitly incomplete.


## Formal existing-benchmark evidence

The fixed matrix-4 timing run stopped with a sampling-contract failure. The first two fully audited Vulkan pairs
(retained_scale and retained_dirty_ratio) completed all 468 expected comparisons:
194 production cases classify as 130 unchanged, 14 within noise, 31 improved and
19 regressed; 274 diagnostic cases classify as 166 unchanged, 18 within noise,
36 improved and 54 regressed. All 19 production flags remain mandatory follow-ups. The separately audited
200-case stress prefix adds four production flags (25 unchanged, 7 noise, 4
improved, 4 regressed across its 40 production cases), for 23 total pending flags.
The original failed process remains failed; its complete numeric prefix is retained
in `m0-existing-baselines-4/retained-stress-preserved-prefix-audit.json`.
Several initial flags are substantial (including static, reorder, layer-update
and backdrop cases); the three dirty-ratio flags are preflat-immediate/80.0%
(+8.45%), persistent-force-full/1.0% (+4.35%) and persistent-auto/1.0% (+6.66%).
There is no no-regression claim. The retained_stress baseline collected 212 case
sequences, but Criterion rejected 12 insertion-only scopes in deletion frames as
zero-duration samples. It saved only 200 numeric cases. Current completed 200
numeric comparisons then exited on the first missing before baseline. Neither
process is a complete 212-case numeric pass. All original logs, saved cases and
regression flags remain. The presence-aware measurement repair is now validated:
NotExecuted retains actual profiled-frame/entry counts and an empty duration;
missing timing and executed zero-duration scopes are errors. Numeric production
results and declared reverse/long controls remain mandatory. Diagnostic results
do not waive production regressions.

The evidence auditor now binds each Criterion change/estimates.json to its audited
new/reference raw samples, checks the exact comparison mean and 95% confidence
interval, verifies the rounded log output and significance classification, and
records the change artifact hash. Focused change-interval and reverse-ratio tests pass. A read-only
smoke audit of 44 prior real comparisons passes; correcting the reverse-ratio
classification changes none of the 19 prior reverse classifications. Historical
artifacts were not rewritten.

## Current formal matrix and validation order

The independent presence probe completed Vulkan/DX12, baseline/current, with
2048 measured frames per size and phase. All 24 IDs per process pass: 12 positive
insertion controls and 12 absent removal scopes. Ten helper and five controller
acceptance tests pass. Actual environment, binary, compiler, input, raw sample
and acceptance-code hashes are required; absence is not a zero-cost measurement.

`m0-existing-continuation-7/plan.json` preserves 656 unchanged-unit numeric pairs
and adds 842 pairs. The complete inventory is 1498 numeric pairs plus 24 absence
pairs, including all 630 production cases and the original 23 regression flags.
Twelve combined root-component metrics now measure complete two-frame cycles;
older per-frame measurements remain historical, with their original units.

All 16 formal continuation processes finished and were individually audited.
The composed audit passed at 2026-09-08 23:34 UTC: 1498 numeric pairs and 24
presence pairs cover the complete inventory. Among 630 production comparisons,
Criterion classified 343 unchanged, 79 noise, 129 regressed and 79 improved.
The 129 flags include the original 23 and require follow-up; this is not a
no-regression result. The frozen controller, helper, protocol, inputs and binaries
remained unchanged throughout timing. Compilation resumed only after it finished.

The next checks are prepared in this order; preparation is not a passing run:

| Work | Current evidence / next gate |
| --- | --- |
| Composed initial matrix audit | `summarize-existing-continuation-7.py`: seven lightweight tests pass (0.016 s), static Spec review closed; the real complete composition passed, with all source-group binaries and units audited. It binds every comparison to exact source-group binaries and units. |
| Isolated M1 CPU / build checks | Attempt 1 captured an import compilation error. After correcting that import, group CPU RED/GREEN and all 452 CPU library tests passed. Attempt 2 passed observations, default all-target, four explicit-device selector tests, three native availability/all-target combinations and coexistence. The atlas benchmark import error was corrected; attempt 4 passed benchmark-internals and all fourteen Linux/macOS feature cross-checks. Strict default lint failed on three previously tracked unused submission-error/receipt items; that gate remains open. Attempts and failures remain separately saved. |
| Known GPU failures | `run-m1-known-red-1.py` requires the audited matrix and completed selector checks, then runs two focused modules across explicit DX12/Vulkan and both texture modes. Twenty-five lightweight acceptance tests pass (0.161 s); actual GPU RED completed all eight processes; the corrected GREEN run completed twelve processes and 44 test executions; the subsequent root-budget follow-up is still in progress. Each expected failure must have its own semantic assertion, passing controls, exact test count, executable hash and GPU/compiler provenance. |
| All production regression follow-ups | `continue-existing-reverse-7.py`: eight lightweight tests pass (0.026 s), static Spec review closed; the real prospective plan now contains nine jobs covering all 129 significant production regressions; no timing run yet. Derive every significant production flag from the full audited matrix, retain the original 23, use each source group's exact binary/units and original parameters. Remaining flags require the declared longer runs and same-binary controls. |

The GPU RED controller's independent reviews found and closed fail-open CIM
errors, prefix-number assertion matching, incomplete input freezing, unnamed
positive controls and acceptance of unrelated panics/errors. New regressions
actually failed before the corrections (20 tests, 11 failures), then all 24 passed.
The corrected process query also rejected the live formal controller in a real
read-only check. A subsequent resumption check also reproduced that only worker number 1 was detected; a new test failed for workers 2 and 17, then the generalized worker pattern passed all 25 checks (`test-m1-known-red-resume-guard-{red-1,green-1}.log`). The controller freezes all shader/HAL/resource/configuration and
acceptance inputs, detects added/deleted files, and requires the same executable
path and hash across all eight processes. These are root-cause fixes to evidence
validation; they do not change rendering or count as GPU RED/GREEN. Pre-fix code
is saved in `m1-red-auditor-before-guard-fix-1`; test logs are
`test-m1-known-red-guard-{red-2,green-2}.log`. Both static review axes are closed;
compatibility with actual GPU output still awaits the first run.

Only light source/document editing and static review run alongside formal timing.
The initial complete classifications are available above; reverse and no-regression conclusions remain pending.
The M0 Linux/macOS checks in the earlier evidence table are completed historical
checks; the completed M1 cross-checks above are separate compiler checks. Neither
cross-compilation nor a backend-unavailable test certifies actual platform GPUs.

## Isolated M1 implementation

M1 exists only in the ignored `target/native-m1-staging` source copy. It does not
replace the main M0 renderer and is not an accepted milestone. Its vendor junction
uses Tileink's sole maintained HAL. No native renderer, native adapter or maintained
HLSL implementation exists yet. The final native lifecycle contract is drafted in
`target/native-m1-staging/NATIVE_API_CONTRACT.md`; its additional native tests are
still unimplemented.

The draft's optional feature graph separates CPU/scene functionality from WGPU.
Default WGPU retains its public entry. Native-only requests currently return
FeatureDisabled, UnsupportedPlatform or AdapterNotImplemented explicitly; they
cannot construct a renderer or silently choose another API. This is M1's temporary
development state, not a completed native feature.

| Shared responsibility | Actual WGPU integration / validation state |
| --- | --- |
| CPU preparation, profiling, damage, history and geometry | CPU algorithms and retained image identity moved out of WGPU. Existing semantics include independent journal/history state, local damage, text/range preparation and CPU-only benchmark inventory. Earlier release checks passed; the latest draft requires a fresh full run. |
| Retained surface cache | Generic opaque allocation owner, instantiated with WgpuTarget. Metadata, budgets, eviction and failed-frame retry are shared; GPU allocation accounting and retirement remain adapter duties. Oversized replacement invalidation has real saved CPU RED/GREEN evidence. |
| Filters and resource tables | Shared cursors, transfer/convolution/turbulence data and path serialization preserve traversal order. They pack nested resources regardless of whether later execution has empty bounds. |
| Paint uploads | Shared PaintUploadState supplies borrowed immediate slices or retained packed words and dirty ranges; WGPU consumes it directly. Tests cover edits, reallocation, mode transitions and resource placement changes. Earlier CPU/default compile checks passed. |
| Deferred SVG resources | Nested SVG/pattern/feImage lower to immutable Canvas resources; WGPU renders children on its own device/queue in the parent command batch, copies GPU images, clears transparent output and publishes readiness after successful submission. Earlier focused GPU checks passed in both texture modes; they did not explicitly select both APIs. |
| Command batching | Generic CommandBatch uses a real WgpuCommands adapter, preserving uniform aggregation, early submission, cancellation and readiness. Confirmed prefixes and unconfirmed attempts have distinct retirement semantics. Eleven CPU cases and an actual GPU abort RED/GREEN per texture mode passed. |
| Uniform aggregation | UniformWrites keys actual GPU buffer identity instead of shared stage labels; it preserves padding, rollover and collision behavior. Nine CPU cases and a two-child pattern GPU case passed. The CPU Criterion workload for 1–512 renderers has not run. |
| Root execution and target planning | Shared root liveness, early-submission policy, 2D dispatch, dense/compact selection and target capacity algorithms are used by WGPU. The u32 dispatch-tail wrap fix has real saved CPU RED/GREEN evidence. |
| Direct draw batches | `render/draw_batches.rs` owns liveness, painter order, ping-pong history, root accounting, error propagation and early-submission eligibility; WGPU is its static adapter. Eight semantic cases plus two eligibility boundary cases passed in the complete 452-test CPU run; the actual WGPU adapter compiles in default all-target. |
| Scene allocation reuse | `render/scene_resources.rs` selects offscreen allocation bundles and manages frame-boundary reuse. WGPU uses the generic pool, retaining target metadata with the local lease. Five semantic cases passed in the complete 452-test CPU run; WGPU integration compiles. |
| Operation traversal | `render/operations.rs` owns plan order, active-root filtering, fused markers, cursors and first-error propagation, consumed by `wgpu/renderer/execution_adapter.rs`. Six semantic cases passed in the complete 452-test CPU run; WGPU integration compiles. Static Standards and Spec review closed findings; an unused old forwarding method and misplaced module documentation were corrected. Group cache/execution is now extracted below; filter/backdrop/mask execution still needs extraction. |

The group-layer extraction remains isolated, but now compiles with its real WGPU
adapter. Shared `render/groups.rs` owns retained cache lookup, clean reuse,
partial redraw, child execution, opacity/mask/composite order, counters and scratch
ownership. Filter/backdrop/mask algorithms remain in WGPU. The former WGPU bounds
and tile-count helpers moved without algorithm changes.

Both static reviews found fallible clear/opacity/mask callbacks leaving scratch
occupied. Actual release RED reproduced the defect: 13 tests, 11 passed and two
failed on the clear-error cleanup assertion. The root-cause fix records each
acquired source/mask and uses one cleanup boundary for every error return; cache
handoff removes local ownership before cleanup. Adapters still retain submitted
physical GPU-use leases independently. The old child helper was removed, and no
GPU call was added to the successful path. Fourteen focused tests then passed,
including failed recording followed by same-pool retry and new/dirty cache handoff
after composite failure. The complete no-WGPU CPU suite passed 452/452 in 1.33 s.
Both review axes closed their findings; the additional cache handoff test received
its requested Spec re-review. GPU output/performance gates remain outstanding.
Evidence: `m1-group-error-investigation/cpu-{red-2,green-1}`; original extraction
sources remain in `m1-validation-1/before-group-execution`.

### Completed earlier M1 checks

These passes precede the newest direct-draw, scene-pool, operation-traversal,
selector and image-fixture edits. They do not validate those edits retroactively.
Artifacts are under `target/backend-parity/m1-validation-1/` unless stated otherwise.

- No-WGPU release library: 417 tests, including 138 shared-render tests.
- CPU build helpers: three DXC discovery, three DXIL cache, one provenance,
  three script and six PNG-comparator tests.
- Independent benchmark inventory: 11 benchmark-matrix and six observation
  executions; observation cases occur in both harnesses.
- Release all-target compilation: default, no-default, native-dx12,
  native-vulkan, native and WGPU+native configurations. Four no-default dependency
  graphs contain no WGPU, wgpu-hal or Naga.
- Actual CPU RED/GREEN: oversized retained-cache replacement and dispatch-tail
  overflow (`cpu-boundary-red-green.json`).
- Actual GPU RED/GREEN: cancelled command batches in both texture modes
  (`command-abort-red-green.json`). Three focused deferred-SVG GPU cases per
  texture mode passed, including transparent child output, localized filter
  resources and retry after failed parent submission.

Strict no-WGPU Clippy exposed unused private rendering code and new-test lint
issues. That gate remains open; no blanket dead-code allowance or fake consumer
has been added. The fresh default lint run confirms three remaining unused submission-error/receipt items; no blanket allowance or fake consumer has been added. The first independent direct-draw review hit a reviewer usage limit and remains
recorded as incomplete. A fresh Standards/Spec review of the direct/root draw
extraction is now complete with no actionable finding: it checked real WGPU
consumption, painter order, early submission, partial-history initialization,
ping-pong switching and failure boundaries. This closes the static review gap
without claiming execution of the pending Rust tests.

### Atlas upload scope and empty-layer GPU corrections

The atlas fixture and Criterion workload now compile. The fixture uses a
2048-square atlas, four padded 1022-square images on page one and a fifth to create
page two. Standalone images are 2048 wide and 32 or 512 high: padding disqualifies
them from the atlas while dimensions stay valid. The 14-case upload workload has
an actual frozen before executable and source manifest in
`m1-atlas-benchmark-baseline-1`; its timing comparisons have not run.

`m1-known-red-1` completed and audited all eight GPU processes on the fixed RTX
4090: explicit DX12/Vulkan, both WGPU texture modes, each with the atlas and
empty-layer modules. All expected semantic assertions failed, while their named
controls passed. Sources, the shared executable, compiler, API and LUID were
verified across processes. This is actual GPU RED, not static inference.

The atlas correction now forces old page initialization only for the recreated
atlas. Independent raster textures retain explicit full-upload, dirty and their
own allocation-recreation conditions. Deferred vector preparation still runs
on atlas recreation so GPU-generated old pages are restored. The existing
standalone canary and explicit-full positive control cover the upload footprint;
no rendering tolerance or readback pixel correction was introduced.

Empty group/mask returns now advance descendant resource cursors. An empty
backdrop advances its own path/filter resources and executes children normally,
as required by Canvas. The five existing GPU regressions cover those three cursor
paths, a passing empty-filter control and actual visible backdrop-child pixels.
Both static reviewers verified the upload and cursor semantics, but found a
remaining integration gap: `root_draw_batch_count` still excludes children of an
empty backdrop, potentially suppressing or misplacing the first-quarter early
submission. After the frozen GREEN completed, the actual CPU regression reproduced both
errors: eleven tests, nine passed and two failed (mixed root count 3 versus 4;
sixteen live children yielding None versus Some(4)). Evidence is in
`m1-empty-backdrop-budget-investigation/cpu-red-1`. A new actual-GPU regression
checks sixteen Main batches, native two-submit versus portable one-submit policy,
and visible/clear pixel controls. Its corrected four-route RED reproduced the missing early submit in both native
texture paths, with both portable controls passing. The first fixture incorrectly
used a renderer status flag to identify texture mode; that failed attempt remains
recorded separately and the corrected fixture queries the actual fine pipeline.
The root-cause correction now counts every backdrop foreground, including empty
effect regions, while excluding group/filter/mask scratch work. All eleven focused
budget tests and 453 CPU library tests passed. All four actual GPU GREEN routes
then passed and were audited in `m1-empty-backdrop-budget-investigation/gpu-green-2`.
Both static review axes closed the finding. Frozen M0 reverse timing began only
after these correctness runs completed.

`run-m1-known-green-1.py` completed all twelve GPU processes and all 44 test
executions, including three deferred-vector controls per explicit API/texture mode.
Named outcomes, counts, GPU/compiler identity, source hashes and one executable
were audited throughout. This validates the atlas/empty-layer corrections at that
recorded source revision; it does not retroactively cover the new root-budget
regression or its future fix. Full M1 SVG/example parity and performance acceptance
remain outstanding.

The renderer unit-test harness now supports explicit DX12/Vulkan and reuses the
existing audited GPU/DXC selector. Six cached device slots distinguish API and
texture mode. Four CPU tests cover slot independence, forced choice, invalid
selection and the actual whole-canvas helper. The last case runs in a subprocess,
requires the precise invalid-API panic and verifies one test actually ran. It
regresses a selector bypass found during review; static re-review closed the issue.
These four CPU cases passed in `m1-validation-1/post-continuation-2/explicit-device-selection.log`. This does not substitute for actual two-API GPU execution.

`target/native-m1-staging/NATIVE_BACKEND_TESTING.md` records explicit API/mode
commands and required provenance. Other SVG/debug/coarse suites have separate
device constructors and are not automatically certified by this helper. Earlier
texture-mode-only tests are not retroactively claimed as explicit two-API coverage.

### Reverse results and prepared long controls

`m0-existing-reverse-7/summary.json` is **complete and audited**: all 18
processes and 129 original production flags were compared in reverse order.
Results oriented as current versus baseline: 88 unchanged, 6 noise, 17 improved
and **18 repeated regressions**. These 18 remain unresolved; the other 111 did
not repeat the regression in this reverse pass. No overall no-regression
claim is made.

`continue-existing-long-controls-8.py` selects all 18 remaining cases and keeps
four blocks per source group: current/current, baseline/current,
current/baseline and baseline/baseline. Each requests 60 samples, five seconds
warmup and fifteen seconds measurement. Significant drift in either same-version
control, or regression in either comparison direction, remains unresolved.
Plans and complete attempts are preserved; failed or partial evidence is never
replaced by a favorable subset. The actual plan is now frozen: **7 groups, 18
cases, 56 processes**. The former `run-long-after-m1-mask-1.py` queue was interrupted by a Windows
reboot before any long timing began. Plan 8 and its binary/input evidence remain
unchanged. The separate boot-specific controller 9 is now reviewed and its plan frozen;
it uses the same 18 cases, binaries and eight-process-per-group schedule with
the newly verified LUID. `run-long-after-m1-filter-1.py` waits for all 200 new
candidate GPU executions and active compilation to finish. Actual long timing
has not started yet.

Root benchmarks override CLI sampling settings at group scope. The pure
configuration transform preserves the 20/2/4 defaults while moving them before
CLI configuration. Isolation attempts 1 and 2 are retained as failed builds:
Cargo also requires declared unbuilt target entry files, then HAL compilation
revealed two GLSL include assets omitted by the old Rust/WGSL source inventory.
The third attempt supplies separately pinned metadata-only entries and genuine
production GLSL resources, with original Cargo artifacts and dep-info proving
their roles. Original raw asset line endings are preserved per revision.
Builder and effective compiler environment records are now hash-linked.
Nineteen root evidence tests passed after fixing the test entry-point ordering;
the older controller-check checkpoint is preserved as historical evidence.
The third attempt built **both root binaries successfully** and the complete
manifest passed its input/environment/artifact audit. Both review axes closed
their remaining findings. `m0-long-controller-checks-2` passed 16 runner tests,
19 root evidence tests and 4 transform tests; its logs and source hashes are
preserved. See `m0-root-cli-binaries-3/manifest.json` and the frozen
`m0-existing-long-controls-8/plan.json`.

### Mask extraction under validation

The isolated M1 draft shares opaque offscreen operations through
`render/surfaces.rs`; group-only opacity and layer-mask hooks remain separate.
`render/masks.rs` owns mask cache/damage decisions, content and coverage order,
resource cursors and scratch handoff. WGPU consumes it directly and the old
WGPU mask algorithm was removed. Cached mask coverage previously remained
occupied when mask-source allocation/recording failed. The shared executor
fixes that ownership path by tracking all four logical slots through one
cleanup boundary; submitted GPU resource lifetime remains the adapter's duty.

`m1-mask-extraction-1/cpu-check-1` completed: **11 mask tests, 14 group tests,
and all 464 no-default release CPU tests passed**, single-threaded; default
release all-target compilation passed. Tests cover same-pool retry, cached
coverage failures, generation changes, distinct content/mask streams, exact
operation targets/bounds and empty/cached/rendered path cursors. These tests
were written before implementation but no CPU RED run was performed for this
extraction while the frozen Criterion matrix was active. Both static review
axes closed their findings. Compiler-confirmed unused imports and duplicate
WGPU region-bound helpers were subsequently removed.

The first `run-m1-mask-gpu-1.py` attempt was interrupted by a Windows reboot.
Only Vulkan native `filters_masks` (6) and `filters_layers` (14) completed and
were audited; the next module's partial log is preserved. The planned 188
executions did not complete. The newer filter candidate contains this mask
extraction and is now undergoing a separate 200-execution GPU matrix. Full
SVG/examples, strict pixel parity, feature/lint and performance gates remain
required before M1 integration.
Before sources: `m1-mask-extraction-1/before`.

### Filter preparation candidate

`target/native-m1-filter-staging` is an independent candidate copied from the
fixed mask-validation source. It does not change the active GPU test inputs.
`render/filter_scene.rs` now owns existing CPU preparation: borrow the original
full physical canvas/plan and requested children; otherwise query spatial
candidates once, compact into local coordinates and translate the filter.
Scratch capacity includes source plus the two cached history slots, with the
maximum child/filter temporary requirement. The WGPU filter executor consumes
this shared object. The initial checkpoint below covered preparation only;
the subsequent execution extraction is described separately. No measured
speedup is claimed for the extraction.

`m1-filter-preparation-1/checks-1` passed **5 focused tests and all 469 release
no-default CPU tests**. The initial preimplementation compile had both a missing
new API and test-import mistakes; it is not claimed as a semantic RED. The first
all-target check exposed a missing copied PowerShell fixture, which was supplied
from the unchanged source. `checks-2` then passed format, default release
all-target compilation and release all-target Clippy. Clippy still reports the
three previously tracked private native-submission dead-code warnings; this is
not a warning-free lint result. Candidate source origin and both preparation
check attempts are preserved locally.

### Shared filter execution and resource fixes

The same candidate now shares filter history passes (`render/filter_pass.rs`)
and full filter scheduling/cache ownership (`render/filters.rs`). The WGPU
execution adapter consumes both; its old duplicate `execute_filter_layer` was
removed. Preparation, rendering policy, and actual GPU allocation/commands remain
separate responsibilities. Recording errors restore the previous context and
release logical scratch slots before returning; incomplete cache images are not
published. Source and filtered history remain separate, and compositing retains
surface size/origin independently of the visible clipped output rectangle.

Three rendering defects were reproduced during this extraction:

1. Full-plan resources copied for a filter targeting scratch retained the entire
   root table sequence, but the executor reset cursors to zero. Root indices are
   now retained independently of whether GPU allocations are reused.
2. Full-plan path tables contain the parent's filter sample path before child
   paths. Child execution now skips that parent slot; compacted local plans do
   not contain it and correctly start from zero.
3. A cached filter with a full-canvas surface and positive source halo overwrote
   root coarse/scan worklists used by other draws in the same submission.
   Restoring only CPU damage was insufficient, and another queue write would
   also rewrite earlier encoded work. Incremental scopes with positive dependency
   outset now use pooled local scene resources. Zero-outset scopes preserve the
   reuse path; the cursor layout remains independent of that storage choice.

These fix resource-layout and lifetime causes rather than fixture-specific
pixels. The retained worklist regression includes a zero-outset control, first
frame cache creation, exactly one dirty tile, preceding/following root draws,
and complete byte comparison against ForceFull. Both static review axes closed
their findings after the fixes; actual GPU GREEN is still required.

Evidence under `target/backend-parity`:

- `m1-filter-cursor-investigation/cpu-red-3.log`: 12 tests, 10 passed and the two
  intended cursor assertions failed (slot 0 versus slot 1). Earlier failed test
  setup attempts are retained and are not semantic RED evidence.
- `m1-filter-cursor-investigation/gpu-red-2`: all four explicit WGPU API/texture
  routes reproduced both expected pixel errors, 8 expected failures. Attempt 1
  correctly rejected the obsolete pre-reboot LUID and is not semantic evidence.
- `m1-filter-worklist-investigation/gpu-red-1`: all four routes reproduced the
  worklist pixel error; the two corrected cursor cases passed as controls
  (12 executions: 4 expected failures, 8 controls passed).
- `m1-filter-worklist-investigation/checks-1`: format, all 487 release no-default
  CPU tests, default release all-target compilation and all-target Clippy passed.
  The three previously documented private native-submission warnings remain.
- `run-m1-filter-gpu-1.py`: the fixed candidate is now running 24 processes,
  200 planned test executions across filter scheduling, masks/layers, retained
  structure, empty layers and deferred images. This is not yet a completed pass.

### New boot session

Windows rebooted while the old mask validation and performance queue were active.
`gpu-after-reboot-1/manifest.json` records an actual new DX12/Vulkan inventory:
RTX 4090 is LUID `9f3f010000000000` on both APIs, still NVIDIA driver 610.62
(DX12 version 32.0.16.1062). The prior campaign used `bf3f010000000000`; its partial
logs are preserved rather than combined with a new completed run. An AMD Radeon
integrated adapter is also available on both APIs, LUID `c551010000000000`; this
inventory is not AMD rendering certification.

Long controller 9 keeps the original auditors/environment unchanged and pins
its own device and boot manifest. Both review axes closed with no findings.
`m0-long-controller-checks-3` records all 19 passing controller tests, including
rejecting changed/missing boot information and keeping old/new LUID logs separate.
`m0-existing-long-controls-9/plan.json` is frozen: 7 groups, 18 cases, 56 processes.
It is queued after the new GPU matrix; no new timing result exists.

### Shared backdrop execution candidate

`target/native-m1-backdrop-staging` copies the frozen filter candidate; its
origin and the filter-validation source manifest are pinned in
`m1-backdrop-extraction-1/origin.json`. It does not modify the candidate currently
running the 200-execution GPU matrix or any M0 production/timing input.

`render/backdrops.rs` now owns backdrop painter order, cache decisions, separate
unfiltered input history, partial/full filter selection, resource cursors and
scratch cleanup. `wgpu/renderer/execution_adapter/backdrops.rs` owns actual GPU
filter/composite recording. The replaced `wgpu/renderer/layers.rs` and five unused
WGPU forwarding helpers were removed. First-error propagation releases occupied
scratch slots and restores filter work before returning; only completed images
are cached, and foreground executes after backdrop compositing and cleanup.

A pre-existing defect was statically confirmed by both implementation and review:
a root Path sample can have no outer clip. The old empty-stack test selected the
rect-only compositor, which rejected the Path and then skipped foreground. Both
fresh and cached compositing now require an actual Rect for the direct path;
Paths retain and reuse their coverage mask. The two GPU regression tests include
independent first-frame colors and a foreground-only retained update compared
byte-for-byte with ForceFull. Their **pre-fix executable is compiled and frozen**
in `m1-backdrop-extraction-1/path-gpu-red-build-1`; actual GPU RED/GREEN execution
is still pending the serial GPU/timing queue. This is not yet a GPU-certified fix.

Checks and review:

- `checks-1` failed imports; `checks-2` exposed the old scratch-only test adapter
  assumption about backdrop foreground. These are setup/fixture failures, not
  evidence of the Path bug. The fixture now records and checks root batching.
- `checks-3` passed 12 focused and 499 CPU tests, then found a WGPU wrapper import
  error; `checks-4` corrected it and passed default all-target check and Clippy.
- Review found the draft's active-region helper had changed exact dirty-tile
  intersection into a bounding-box intersection and added an unnecessary scan.
  `work-cpu-red-2` actually reproduced **two semantic failures plus one passing
  control**. The implementation now uses the original tile-intersection contract,
  leaving filter coordinates and adapter tile masking unchanged. RED attempt 1
  failed before compilation due to missing compiler environment metadata.
- Review also required keeping shared clip assertions exact. Test adapters now
  use an explicit expected stack, defaulting to the existing `2..4`; Path cases
  request the empty stack explicitly instead of weakening other suites.
- `checks-5` passed **17 focused and all 504 release CPU tests**, default
  all-target check and release all-target Clippy. Cases include Path/Rect cache
  hits, painter order, empty effects, full versus partial work, recording failure,
  scratch exhaustion/retry, root versus nested foreground, and direct-downsample
  eligibility. `checks-6` removed both unused imports and repeated default/Clippy successfully; three earlier native
  submission-contract dead-code warnings remain, so strict warning-free lint is
  still open. Both reported Standards findings are statically closed.

CPU fake-adapter tests certify scheduling and ownership, not GPU initialization,
pixels or performance. Full M1 SVG/examples, parity, feature rechecks and Criterion
acceptance remain required before this draft is integrated.

### Partial-frame cache eviction candidate

`target/native-m1-cache-staging` independently copies the frozen backdrop candidate
(`m1-cache-eviction-1/origin.json`). Review confirmed a reachable older defect:
inserting a grown earlier filter cache could evict a later backdrop before that
backdrop was visited. The eviction flag only invalidated the next frame, leaving
the current partial frame without complete painter-order source history.

`cpu-red-1` actually reproduced this through RetainedRenderState begin/finish;
`clock-cpu-red-1` separately reproduced the old wrapping LRU clock evicting the
newest entry. The candidate protects unvisited old Backdrop entries during partial
root frames with a constant-time clock cutoff, permits ordinary/current-frame LRU
eviction, and discards new optional cache entries when necessary to preserve the
budget. Full root redraws retain ordinary LRU, every finish path clears protection,
and clock overflow rarely rebases timestamps while preserving order and the cutoff.
This fixes history lifetime; it does not introduce a whole-root wait or per-frame
cache scan. The invariant is documented in the candidate testing document.

`checks-1` passed **23 focused and all 513 release CPU tests**, format, default
all-target check and release all-target Clippy; the three previously tracked
native submission warnings remain. The actual GPU case grows a 16² filter to 32²
before a 128² backdrop with an exact-fit first-frame budget and compares the
partial second frame with ForceFull. `gpu-red-build-1` freezes the pre-fix binary;
GPU execution and timing remain pending the serial queue. The earlier Path fix
also now has a separately frozen GREEN executable in
`m1-backdrop-extraction-1/path-gpu-green-build-1`, not yet executed on GPU.

### Filter input dependency candidate

`target/native-m1-dependency-staging` copies the frozen cache candidate. The
input-read radius had been inferred from output expansion, omitting Erode,
convolution and lighting neighbours. Wrap additionally needs the original whole
address domain; cropped passes change its wrap period. Canvas and materializer
damage propagation also incorrectly used output expansion for input influence.

The candidate introduces `FilterDependency::{Local, WholeRegion}` independently
of output bounds, propagates it through Chain/Graph, preserves complete Wrap
surfaces, and reconstructs dirty Wrap caches locally while keeping clean cache
reuse. Shared local scene selection now carries nested filter input domains and
supplements only batches outside the original spatial query/canvas; supplemental
draws retain their batch painter order. This also retains required off-canvas
inputs that root tile bins cannot enumerate. Partial local work cannot alias
root GPU scan/coarse buffers. These are root-cause fixes, not tolerances or
whole-root redraw workarounds.

Actual RED evidence in `m1-filter-dependency-1`:

- `finite-cpu-red-1`: four model failures and two controls.
- `wrap-cpu-red-3`: six model failures, two controls, and two Canvas damage
  failures. Earlier `wrap-cpu-red-1/2` failed compilation due to test imports and
  are not semantic evidence.
- `selection-cpu-red-1`: two local-selection failures and five passing controls,
  covering fully off-canvas input and nested Wrap input outside its outer surface.
- `cpu-red-1` separately reproduced missing Erode processing halo.

`checks-1` passes **56 focused and all 526 single-thread release CPU tests**,
format, default all-target check and release all-target Clippy. The three earlier
native submission dead-code warnings remain; this is not a warning-free gate.
Pre-fix executables and source manifests are frozen in `finite-gpu-red-build-1`
and `wrap-gpu-red-build-1`. The six GPU regressions cover four finite cache-miss
filters, retained Wrap domain/opposite-edge updates, and independent expected
off-canvas source color. Actual GPU RED/GREEN, full SVG/examples/parity and
Criterion remain pending. No new pixel or performance acceptance is claimed.

The subsequent `edge-cpu-red-1` reproduced four additional failures with one
control: early root clipping of nested filter influence, revival of an explicit
empty clip, Canvas oracle loss of an off-canvas Offset intermediate, and integer
wraparound for a large convolution anchor. `nested-damage-cpu-red-1` initially
expected two failures but actually produced one failure and one passing control;
its runner rejected that count, so the complete audited evidence is the later
edge run. `checks-2/3` were compile failures during mechanical signature changes.

`checks-4` passes **61 focused and all 531 release CPU tests**, default all-target
check, format and release all-target Clippy (the same three native submission
warnings remain). Root clipping now happens after complete influence propagation;
`BoundsInfluence` explicitly distinguishes identity from an empty clip, and bounds
expansion saturates rather than wrapping. Standards review found no remaining
actionable issue in this candidate. GPU execution/performance are still pending.

The frozen filter extraction matrix now passes all **24 processes / 200 named
GPU executions** on Vulkan/DX12 and native/portable WGPU texture modes
(`m1-filter-extraction-1/gpu-check-1`). The Path/dependency RED-GREEN matrix is also
complete: **16 processes / 64 named executions** audited against exact expected
outcomes, physical GPU identity, driver, runtime compiler and immutable binaries
(`m1-filter-dependency-1/gpu-check-3`). These remain WGPU reference paths, not
actual native backend acceptance.

The original cache-eviction GPU fixture passed before the fix because uniform blue source-over can hide a missing transparent blur halo. Its rerender count was not valid GPU reproduction evidence. The stronger nonuniform halo regression below closes this gap; the original CPU cache-lifetime/clock RED evidence remains valid.

### Scoped Backdrop damage candidate

`target/native-m1-backdrop-damage-staging` copies the frozen dependency candidate.
`m1-backdrop-damage-1/cpu-red-2` reproduces two real update failures: nested
Backdrop input was queried in root coordinates, and opacity parameter updates
used a layer chunk's empty child list as its output. The candidate separates
resolved root damage events from prunable node-state overlays, captures actual
old/new local output before/after chunk rebuilding, and propagates the complete
command tree only for scoped Backdrop domains. Ordinary root domains retain
their indexed path. Layer parameter patches now use the same output domain as
frame construction.

The completed damage candidate passes **546 single-thread release CPU tests**, default all-target check, release Clippy and formatting (`m1-backdrop-damage-1/checks-4`, `default-checks-1`). The initial `checks-1` was a compilation failure; the earlier positive-coordinate GPU case is a passing control, not a reproduced failure. Audited GPU evidence comprises eight original RED/control executions, four ancestor-Clip RED executions and **12 GREEN executions** on all four WGPU API/texture-mode routes.

`target/native-m1-backdrop-order-staging` then fixes two further input-order defects: a parent Backdrop paints its output before its child samples it, and a Clip-to-Isolate transition changes the child's input domain. The domain comparison now captures old classifications before any command mutation and compares after all command updates converge, including mixed layer/leaf transactions. Ordinary updates within the same isolated domain retain the partial path. Scoped-domain classification is cached alongside dependency refresh, removing repeated whole-scene classification from frame patch/publication.

The final candidate passes **549 single-thread release CPU tests**, default all-target check, release Clippy and formatting (`m1-backdrop-order-1/checks-2`, `default-checks-2`). The same three existing native submission dead-code warnings remain. Actual GPU evidence includes eight parent-order/domain-transition RED executions, four mixed-transaction RED executions, and **24 GREEN executions in eight processes** (`gpu-green-check-2`). Every named result, source/binary hash, compiler, driver and GPU identity was audited. These M1 runs use RTX 4090 LUID `9f3f010000000000` in the recorded 2026-09-09 boot; earlier M0 identities remain historical evidence for their own sessions.

Scoped history includes skipped-renderer updates, independent state-page pruning/compaction, retry without commit, history epoch recovery, off-canvas input, Clip growth, Mask branch separation and isolated-domain controls. Full SVG/examples/parity and Criterion still remain before integration. The independent scope-update Criterion harness in `m1-scoped-damage-bench-1` is prepared but has no timing acceptance yet.

### Same-frame Backdrop cache lifetime: GPU reproduction closed

`target/native-m1-cache-halo-staging` preserves the 549-test production candidate and replaces the insufficient uniform-background GPU fixture with a nonuniform input/foreground halo regression. The test locks the probe tile as dirty and the green source-stripe tile as clean, checks an independent green contribution, and compares every RGBA pixel against an independent ForceFull renderer.

`m1-cache-halo-1/gpu-check-1` audits **eight processes: RED and GREEN on all four WGPU API/texture-mode routes**. The isolated counterfactual changes only the partial-frame protection boundary to disabled. It records an earlier Filter growing from 2,048 to 8,192 bytes under a 526,336-byte budget, evicting the not-yet-visited 524,288-byte Backdrop input. The copied active domain is 48..112 while clean green input begins at x=112. At (111,80), the counterfactual yields `[0,0,255,255]` instead of `[0,82,173,255]`. With protection retained, every route is exactly equal to ForceFull.

Final `gpu-green-build-2` contains no diagnostic logging. Default all-target check, final release Clippy and formatting pass; the three known native submission warnings remain. Two independent reviews found no remaining actionable issue after strengthening the active-tile assertions. The first probe build failed compilation because its diagnostic pixel helper used u8 rather than the renderer's packed u32; it is not semantic RED evidence.

The external CPU-only Criterion harness (`m1-scoped-damage-bench-1`) passes ten smoke scenarios for each candidate. `timing-1` completed eight serial processes (forward/reverse and both same-version controls), each with 60 samples, 3 s warmup and 8 s measurement per case. No build or GPU job overlapped. Root updates with 100/1000 Backdrops measured about 16%/15% faster forward, with the same direction in reverse, but current/current controls drifted by as much as 18.8%. These samples do not pass performance acceptance. A follow-up must first establish stable adjacent same-version controls. This harness compares complete update cost between the damage and order candidates, not full M1 against M0; it cannot attribute all differences to the classification cache.

The M0 long-control run also completed (`m0-existing-long-controls-9`): 56
processes / 18 cases, 60 samples with 5 s warmup and 15 s measurement. Eight
cases clear this control pass, nine retain control drift, and DX12
`middle-layer-insert/5000` remains unresolved with opposite forward/reverse
results. **No performance acceptance is claimed.**

### Shared frame and layer execution candidate

`target/native-m1-frame-staging` now wires the production WGPU frame entry into
the shared executor. It owns lazy plan access, empty-damage history copy,
preparation/scan/clear order, early root batch budget, active batch selection,
direct/recursive dispatch and success-only output copy. The WGPU adapter retains
physical resource operations; active batch queries remain mutable to preserve the
scene-upload query cache. Shared layer dispatch interprets all offscreen layer
kinds once and rejects fused clips before consuming resources or children.

The candidate passes **563 single-thread release CPU tests**, including ten new
frame contracts and four layer-dispatch contracts, plus default all-target check,
release all-target Clippy and formatting (`m1-frame-extraction-1/checks-4`). Three
existing private native submission warnings remain. Independent Spec review
closed both execution-boundary findings; Standards draft review found no new
organization issue. Actual native GPU contexts still belong to M3/M5.

`features-1` passes all **21 Windows/Linux/macOS feature builds**, the
bench-internals all-target check, and three native-only normal/build dependency
audits with no WGPU package. These are compilation/dependency results, not native
renderer or Linux/macOS GPU certification. The final renderer module GPU matrix
is running. Its initial audit rejected a valid forced-portable test constructor;
`gpu-check-2` preserves and reaudits the original successful logs with explicit
constructor-mode coverage rather than labelling every test as four-mode evidence.

### Structural damage recovery correction

The full frame-candidate matrix stopped after 23 successful Vulkan/native modules:
`retained_removal_and_reinsertion_refresh_backdrop_history` unexpectedly selected
`DamageHistoryUnavailable` inside an outer Filter. It repainted all 48 tiles despite
only nine changed tiles. Frozen frame/cache-halo candidates reproduce it; the earlier
dependency candidate passes (`m1-removal-history-1/reproduction-1`).

`target/native-m1-removal-staging` resolves hierarchy damage against old chunks,
metadata and painter order before mutation, then resolves new input against the new
tree. Only final root bounds and dirty Backdrop IDs are combined. Two CPU cases first
reproduced the history gap, then passed. `gpu-check-1` audits four RED and eight GREEN
executions across WGPU DX12/Vulkan and both texture modes, including exact ForceFull
pixels and partial rendering in root/Filter/Isolate/Mask domains.

Two additional CPU tests lock Group reordering and content/mask reparenting to their
actual dirty IDs and root tiles. The first Group fixture incorrectly assumed exact
translation through Offset's existing conservative radius expansion; diagnostics
disproved that expectation. The final fixture uses a pointwise Filter to isolate
ordering, and the temporary logs are removed. This is not a second production fix.

Final `checks-5` passes **567 release CPU tests**, strict all-target default Clippy
with `-D warnings`, and formatting. The three reserved shared submission members
use narrowly scoped, explained lint expectations in non-test WGPU builds; native-only
builds still have unused-contract warnings and are not described as warning-free.
The actual final binary is `gpu-green-build-3`. Its full matrix freezes the binary's
30-module/207-case inventory and requires all 120 processes/828 named calls; it does
not reuse GREEN from a different production binary. Validation is still in progress.

### Remaining M1 and later work

Finish the final candidate's full release, GPU, SVG/example, strict parity and
Criterion gates, review final wiring and packaging/integration, then integrate M1.
Shared frame/layer/cache/filter scheduling and the native API contract are now
implemented in the isolated candidate; this is not M1 acceptance. Actual native context, target and completion
implementations belong to M3/M5; their required executable checks remain tracked
in the contract rather than being claimed as M1 functionality. M2–M6 remain unimplemented; the earlier
DXC binding probe is compiler preparation only. Required AMD/Intel Windows,
Linux Vulkan and macOS WGPU-Metal hardware coverage is outstanding. No remote
machine was connected or tested during this continuation.

## Reproduction

See [runner commands](scripts/ps1/README.md#explicit-wgpu-dx12vulkan-reference) and
[BENCHMARKS.md](BENCHMARKS.md). Keep each report with its immutable manifest and
compiler/pipeline data. Local diagnostic artifacts are ignored by Git. The prior
chronological investigation is preserved locally as
`target/backend-parity/m0-investigation-through-v13.md`; it contains rejected
experiments and must not be used as the current validation status.

The superseded detailed M1 continuation notes are preserved locally in
`target/backend-parity/m1-progress-before-consolidation-1/NATIVE_BACKEND_PROGRESS.md`.
The current status tables above supersede their chronological status statements.


### M1 resize 采样根因定位（2026-09-09，仍为隔离候选）

`native-m1-removal-staging` 的完整 GPU 验证在 DX12 `backdrop_resize` 中中止：
resize 当帧像素 (161,101) 红通道 93/92，不是后续增量 damage 遗漏。
回到 frame/dependency 候选也稳定复现；只在诊断中把 grown scratch 192×144 改为 192×128 即归零。
直接调用生产 sampling helper 的 0.6 秒回归在旧算法下发现 28,232 处容量相关差异。

新 `native-m1-resize-staging` 在逻辑 texel 坐标中做四 tap 双线性插值，保留容量复用。
移除不再使用的 filter sampler，保留 image atlas sampler。初版 source/aux、原 resize、shrink
合计 8 次 GPU 测试通过；补强后的三个 sampling 合同加两个 resize、一个 shrink，
在 DX12/Vulkan portable 各 6 个测试共 12 次通过（`m1-resize-pixel-1/focused-green-2`）。
独立期望覆盖预乘 alpha、四角、中心、quarter 权重、四侧 clamp、1×1/单行/单列；
每轮全幅失败哨兵防止漏 dispatch 被当作成功。两路静态审阅无剩余 finding。

最终 sampling 候选的验证结果（`m1-resize-pixel-1/`）：

- `gpu-full-1`：124 个串行进程、840 次 renderer 测试及 4 个设备选择测试全部通过。
  明确记录强制 portable 的模块及额外默认构造器限制；这些是 WGPU 纹理执行路径。
- `checks-3`：默认 all-target release Clippy `-D warnings` 与格式检查通过；
  `default-cpu-3`：9 个进程、643 次 CPU 测试通过（其中默认 lib 602 项）。
  另有无默认 feature 的 567 项 CPU 测试通过；不据此声称默认 lib 881 项全覆盖。
- `features-1`：Windows/Linux/macOS 共 21 个 feature/lib 构建、bench-internals all-target
  构建及三个 native-only normal/build 依赖图审计通过；交叉编译不代表 GPU 认证。
- `delivery-2`：三个 native feature 的不可用构造合同、default/scene/native doctests 通过。
  Cargo 包已生成；首次把包放在父 workspace 内的检查失败，改用真正独立临时目录后，
  `delivery-package-3` 未修改包内容的六种 feature 独立构建全部通过。
- `website-2-cold`：中英文文档构建成功。共享 Docusaurus 持久缓存曾导致 English DocItem
  SSR 失败；使用其已有 `DOCUSAURUS_NO_PERSISTENT_CACHE=1` 选项即通过，未改网站生产配置。
- `strict-smoke-1`：三个基础场景四条 WGPU 路径零差异，仅是基础检查，未覆盖 filter 全语料。

`m1-sampling-bench-1/build-4` 冻结相同 Criterion 场景的旧采样/新采样程序。
构建清单覆盖文件增删、junction 目标、工具链和有效编译设置；错误纹理模式明确失败。
`checks-4` 的模式 RED/GREEN、格式及严格 bench Clippy 通过，规范复查无剩余 finding。
固定和 resize 各两种 API × 两种纹理模式的独立控制/正反顺序计时在 51 个进程后中止：
Windows 校时使 CIM 的启动时间估计变化，旧检查误判重启。Kernel-General 12 的 StartTime
仍与原始会话完全相符。保留前六个完整 case 的 48 次采样和未完成 case 的三份记录，
未生成完整通过报告；剩余 Vulkan native texture 两个 case 待整组重跑。
这是 sampling fix 对 removal 候选的对照，不能替代完整 M1/M0 对照。

完整 SVG/examples、严格 reference、余下显式 GPU harness、PNG 审阅和性能验收尚未完成。
没有 commit/push，M0/M1 均未宣布通过。

### M1 共享场景准备边界补全

最终规格复查发现 root plan cache/metadata 和 text new/update/reconcile 的决策仍位于 WGPU。
`native-m1-prepare-staging` 在 sampling 候选上继续抽取 `render::prepare`：共享状态保存
外层场景 fingerprint 与 stack-depth metadata，WGPU 消费编译/复用、scratch 和 filter 刷新决策。
临时 localized execution plan、活动栈深度及具体 GPU 资源仍由 Adapter 保存并恢复。
文字数据槽保持现有所有权，共享 helper 统一 retained ranges 与 flat reconciliation 的选择。
这是补齐 M1 的准备职责边界，不是宣称已实现原生 GPU。

七项新 CPU 合同已经加入。首次执行五项通过；另外两项测试误把 flat Canvas 的重复编译
视为同一 Rc、误把 Some(empty text changes) 视为 None，现已按实际 API 语义修正期望。
`m1-scene-preparation-1/checks-2` 的七项合同、574 项无默认 feature CPU 测试、
格式、默认 all-target release Clippy `-D warnings`、Linux/macOS all-features lib 构建已通过。
清理了共享 scratch 计数迁移后的多余 import，并明确单个 Range 的测试期望。
准备边界、所有权及 scratch 阶段计时归属的静态规格复查已通过；最终 WGPU 接线验证继续执行。
sampling 候选的已完成证据不冒充这个新候选的最终验收。


早先两条等待计时完成的队列均在执行前取消，记录保留。`validation-queue-3` 在计时停止后串行执行最终候选验证。性能审计 v2 将 Criterion 实际 `new` 样本、
saved reference、comparison 消费的 reference 连成同一证据链，保留显著性，并校验
实际设备 features、memory hints、pipeline-cache 和编译模式。旧审计和原始采样不修改。
严格像素审计 v2 区分普通 route totals 与 retained 的 29×24 逐帧 pipeline 记录，
校验 API 与 route 名对应、PNG 路径唯一，以及故意缺失的 SVG 资源仍保持缺失。
跨 route 和跨重复运行均比较完整 RGBA 字节，不使用容差或只比较 hash。
`m1-scene-preparation-1/verification-auditors-1` 记录一项预期 RED 和四个 GREEN
工具检查进程；两路静态复核无剩余 finding。这些是验证工具检查，不算最终候选的 GPU 通过记录。

启动检查 v2 使用固定摘要的内核启动事件，并在消费时重新核验原 GPU manifest 摘要、
JSON 和 StartTime 关联，避免校时误判，也防止混入重启后的会话。真实消费端回归先在旧版
复现拒绝缺失，再在新版通过；静态复查无剩余 finding。原始计时和旧检查脚本均保持不变。

### M1 filter program 边界补全

`native-m1-prepare-staging` 的 release harness、650 次默认 CPU 测试及 ABI inventory
（20 个模块、179 个入口实例、37 个 filter/74 remap sets）完成。规格复查发现复合滤镜
调度仍在 WGPU；`validation-queue-3` 因此主动停止，部分 GPU 记录保留但不作为全量通过。

新 `native-m1-filter-program-staging` 保留原候选，抽取 Chain/Graph、scratch/SourceAlpha、
blur/partial/downsample/glass 调度，WGPU 降低为 typed kernel 编码。CPU 故障注入先复现
2 个正常合同通过、5 个失败合同 RED，再修正 clear/pointwise 失败传播与 Merge 无效输入
泄漏。扩展后的 11 项 CPU 合同已通过；最终 `checks-5` 的 585 项无默认 feature CPU 测试、
格式、默认 all-target release Clippy `-D warnings` 及四次实际 WGPU 故障回归也已通过。
WGPU opacity group 曾吞掉 pointwise 编码失败；实际 Adapter 回归先 RED，再修正为向共享执行器返回错误。
两种 glass 路径统一资源生命周期，正常 shader 算法保持不变；不据此宣称性能已通过。
准备候选的构建/反射记录不是新 filter-program 候选的最终证据，后者还需完整验收。


### 最终 M1 验证与 PNG / 性能证据准备

`m1-filter-program-1/validation-queue-1` 串行执行新的最终候选验证，未复用旧候选的通过状态。
21 项平台/feature lib 检查、bench-internals all-target、三个 native-only 依赖图、
三个不可用构造合同、三种 doctest、Cargo 包的六种独立 feature 构建、中英文网站构建已通过。
`default-cpu-1` 完成 661 次测试；shader inventory 完成 20 模块、179 入口实例、
37 filter / 74 remap sets。renderer GPU 部分已完成 124 个进程、844 项测试和四项 selector；
后续独立 blur-composite 测试发现真实像素差异，队列失败停止。严格 reference、普通 PNG 和性能验收未完成。

`png-review-inputs-1` 固定 HEAD `da70e7fea479f64a7f8f8b3d4e2bff338fd02f2e` 与 M0 普通 PNG：
1,757 张当前渲染输出，另 1,731 张已跟踪参考/退役图字节未变。M0 的 181 张变化均与待审 V13
工件字节一致。后续新生成的 native/portable 共 3,514 张图，将同时比较提交→M1 与 M0→M1。
新增工具覆盖透明 RGB、alpha、尺寸和损坏 PNG；IDAT CRC 漏检已先 RED 再修复为 verify+重新解码。
五项像素合同与九项生成证据合同通过，审查发现均关闭。报告必须重验构建/输入/日志/写图集合/归档
SHA 关联，不能仅信任完成状态。新 PNG 尚未生成，更没有新的人工接受记录。

`m1-full-benchmark-1` 已保存完整 M0/M1 源码副本并统一两侧 benchmark/common/support 工作负载。
M0 的 248 个生产源码摘要仍逐项匹配旧 matrix-4 的 current 端；M1 来自最终 filter-program 候选。
未开始构建或采样。计划新跑每 API 749 numeric + 12 absence、共 1,498 numeric / 24 absence /
630 production，以及 resize、CPU 和各项局部优化对照；不会导入旧 M0 计时作为 M1 的测量。
候选 BENCHMARKS 文档的 root-fragment 两帧单位与 absence 说明需同步，待当前冻结验证结束处理。

sampling 的前 48 次完整记录已独立审计。DX12 portable resize 的两个方向约改善 4.2%，
该项同版本控制稳定；其余五项仍有控制漂移或不确定性，不能概括为 sampling 性能全部通过。
剩余 Vulkan native texture 两个 case 的 16 次整组重跑脚本已准备，旧三份未完成记录保留且排除。
计时子进程的 affinity 失败清理已用真实旧启动代码提取做 RED、新 helper 做 GREEN，避免遗留 GPU
进程污染后续采样。所有正式计时等正确性 GPU 工作结束后串行运行。

M0 的既有性能待决项、PNG 人工审阅，以及 M1 完整像素与性能门槛仍开放；未 commit/push。

### M1 混合玻璃场景像素差异：诊断记录

`m1-filter-program-1/remaining-tests-1/wgpu_backend_parity-blur_composite.log`
记录 mixed 场景 `(552,112)`：DX12 native texture 的 RGBA 为 `[252,243,245,255]`，
Vulkan native texture 为 `[252,243,246,255]`，一个像素、一个通道相差 1。
这是严格跨 API 条件的失败，不按视觉容差接受，也不靠更新 golden 关闭。
失败候选及其原始证据保持不变，尚不能确认差异从哪次变更引入。

独立 `native-m1-composite-staging` 只增加临时诊断入口，生产算法未变。
`m1-composite-pixel-1/vulkan-1` 已保存两种 Vulkan 纹理模式的完整原始 RGBA、PNG、
设备与源码/二进制摘要，两种模式逐字节相等。下一步捕获 DX12 并缩小到相关面板/计算阶段。
`native-m1-composite-minimize-staging` 准备逐步移除无关场景元素，结果尚未判定。

另 `m1-full-benchmark-2` 是仅准备的性能输入后继：在两侧相同 benchmark 中将 root Criterion
默认值移到可被 CLI 覆盖的层级，保留 20 samples/2 秒 warmup/4 秒 measurement 和原计时单位。
没有构建或采样；必须在像素修复后的最终候选上重新冻结来源，不能把失败候选当作已验收。
全部正式计时继续暂停。


### M1 玻璃数值修复：最终候选待完整验收

诊断把混合场景缩小到一个玻璃面板、一个圆角矩形和一个圆。源图及 blur 中间值
跨 API 相同；首次差异位于 `liquid_glass_edge`，DX12/Vulkan 分别产生
`0x3f0ecca3` / `0x3f0ecca8`，随后采样跨越 RGBA8 舍入边界。
旧候选和阶段捕获证据保存在 `m1-composite-*-1`，生产版本不包含临时探针。

`native-m1-glass-refraction-staging` 用代数 Snell 关系代替往返反三角运算，
明确 FMA，并处理极大有限折射率的掠射极限。零色散系数保留原坐标，避免
溢出位移乘零产生 NaN；非零色散公式不变。三个永久 GPU 回归分别覆盖真实
场景、365 个折射输入和八种色散坐标。独立 f64 预期也约束普通非零位移，
避免只靠跨 API 一致性接受两边都错。没有放宽 RGBA 比较或引入像素吸附。

`m1-glass-regression-1` 保存原折射函数的几何/像素 RED；
`m1-glass-refraction-1` 保存掠射 infinity 和 Vulkan 色散 NaN 的后续 RED。
代数修复的最小场景和原混合/isolated 场景四条 WGPU 路径已通过；
365 项几何合同及零色散修复也分别通过。复查新增普通位移预期后须冻结最终
版本，再跑最终 Clippy、完整渲染、严格 reference、PNG 与性能验收。
这里的四条路径是两种 WGPU API × 两种纹理模式，原生 DX12/Vulkan 尚未实现。

M1 共享模块候选已经实现，当前处于验证修复阶段；尚未集成到主工作树。
M0 性能待决项、PNG 人工审阅、M1 全量验收仍开放，没有 commit/push。


### M1 玻璃最终候选验收已启动

`m1-glass-refraction-1/build-6` 冻结最终三个玻璃回归。`geometry-6` 的 365 组
折射输入及 `dispersion-6` 的八组坐标检查通过。正常色散现在还有独立 f64
预期，补齐“全部位移错误地返回零也能通过”的测试缺口；两路复查 finding 已关闭。

新证据目录 `m1-glass-final-1`：`checks-1` 的 11 项滤镜合同、585 项 CPU 测试、
格式、严格 all-target release Clippy 和四条 WGPU 路径的实际错误传播回归通过。
21 项平台/feature 检查、native-only 依赖图、构造/doctest、六种独立包构建和
中英文网站通过。默认 CPU harness 共 661 次测试通过；最终库清单 900 项、
parity harness 47 项（包含新增三项玻璃回归），已冻结并开始串行完整 GPU 验证。

最新反射为 20 variants / 179 entrypoint instances / 74 filter remap sets，
确认滤镜 sampler 51 不存在。`documentation-1` 保存与实际反射 SHA 相同的 JSON
及更新 MD，经静态复核通过；尚未替换正在验收的冻结候选中的旧 V13 文档。
GPU 队列未完成，严格 reference 和普通 PNG 尚未开始，不声明全量通过。

`m1-full-benchmark-3` / `m1-full-benchmark-4` 重新保存最终修复候选与 M0 来源，
两边使用相同 workload；后者统一已审阅的 Criterion CLI 默认值覆盖规则。
新的来源链/原始归档/计时单位检查八项通过，两路性能工具静态审阅关闭。
没有构建或正式计时；旧失败候选的记录不会计入最终 M1 性能验收。

PNG 报告已重定向到新最终候选，继续使用原封存的提交/M0 图像基线；尚未生成新图。
M0/M1 未宣布验收完成，M1 未集成，没有 commit/push。


### M1 最终 GPU 测试通过；严格参考审计修正

`m1-glass-final-1/gpu-full-1` 的 124 个串行进程、844 次具名 GPU 测试及四项
device selector 检查通过。`remaining-tests-1` 的 42 个进程、178 次测试通过，
包含两种普通纹理模式的 126 次 SVG 语义测试、四项补充测试，以及明确启用的
48 项 harness 测试（47 项 parity 和一项 DX12 顺序回归）。最终三个玻璃回归、
原混合玻璃场景都已通过。测试进程墙钟时间包含初始化和编译，不能当作帧性能数据。

`validation-queue-1` 后续在示例的独立 pipeline 审计失败，原始记录保留。
实际示例已成功渲染；审计误读了未参与绘制的 route shell 的零计数。真正的
`example-pipelines.json` 原本就记录 45 个图片 workload 和两个诊断 workload。
新的审计逐个绑定这些输出，runtime 路径要求 embedded 为零，所请求的 DX12
portable precompiled 路径每个 workload 都必须有 embedded pipeline。
计数是 renderer 的累计编译快照，不求和，也不代替实际帧执行证明。

`reference-audit-repair-1` 复现旧断言，十项回归通过，已有 180 张示例 PNG
重新独立比较全部有效 RGBA 字节，零差异。这个修正仅涉及验证工具；候选源码与
二进制保持冻结，因此不重跑已经通过的 844 + 178 项测试。
`strict-reference-2` 重新审计前四次原始 capture，并继续其余 16 次 GPU capture。
完整运行后还必须通过 `reference-seal-1`：消费原审计摘要再检查所有报告、实际
pipeline 记录、非参考路线 PNG、字体、资源（含刻意缺失输入）和跨变体/重复结果。
普通 PNG、性能和集成入口必须验证封存摘要及原证据，不能只读取队列 complete 标记。

M0 的旧长时间控制记录 `m0-existing-long-controls-9/summary.json` 仍有待决项：
18 项中八项没有回归，九项受控制漂移影响，一项方向不一致
（DX12 middle-layer-insert/5000）。这不是八项已确认性能回归，也不代表全部通过。
新 M1/M0 正式计时尚未开始，M0/M1 PNG 人工接受和性能验收仍待完成；没有集成或 commit/push。


### M1 全量严格参考与最终证据封存通过

`m1-glass-final-1/strict-reference-2` 完成 20 次 capture（四次原始 capture 重新审计，
16 次新 GPU 进程）。两次 smoke，加上 runtime/precompiled 各三轮的 1712 SVG、
45 examples、29 retained 帧；retained 每帧覆盖 24 个 route/target/policy 组合。
合计 46,368 张输出，全部有效 RGBA（含透明 RGB）跨路线、编译变体和重复运行零差异。
这是 WGPU DX12/Vulkan × native/portable texture 的参考证据，原生 API Adapter 尚未实现。

`reference-seal-1` 已消费原审计摘要，再重验每次 capture 的 report、manifest、实际
pipeline 记录、全部 PNG、字体及资源；持久化刻意缺失资源的状态，并在末尾复验所有
文件摘要。七项捕获证据回归和三项计时隔离回归通过，两路工具审查的 findings 已关闭。
`validation-queue-2` 的 11 项已通过阶段及严格参考均完成；旧 queue-1 的审计失败保持归档。

`acceptance-queue-1` 继续串行生成普通 PNG、提交/M0/M1 三方对照，再构建相同来源的
性能比较程序。PNG 人工审阅、M1/M0 Criterion 与 resize 尖峰验收尚未完成。具体尚待执行的
通用 CPU、uniform、atlas、scoped damage 和 sampling 门槛见
`target/backend-parity/m1-glass-final-1/performance-worklist-1.json`。没有导入旧候选计时，
没有宣布 M1 完成，没有集成或 commit/push。


### M1 普通 PNG 验收与人工接受

普通 PNG 首次运行在 portable examples 的输出目录核对失败：工具误用
`examples/wgpu-portable/out`，实际公开示例入口为 `examples/wgpu_portable/out`；
SVG 文件后缀仍是 `.wgpu-portable.png`。新增独立字面量路径回归先 RED 再 GREEN，
三项路径与九项证据链测试通过。Renderer 未修改；旧失败目录及未归档的 45 张输出保留。
修正后的 `ordinary-pngs-2` 四个普通渲染进程、3,514 张图和完整归档审计通过。

`png-review-1` 对照 1,757 组 native/portable 输出，全部像素一致；1,731 张未渲染的
已跟踪图保持原字节。提交→M1 共 226 张图、19,313 个差异像素；M0→M1 为 64 张图、
14,630 个差异像素。另对 1,757 帧原始预乘 RGBA 独立比较，M0→M1 每通道最大差为 1；
普通 PNG 反预乘会放大极低 alpha 处 RGB 数值差，说明和完整统计均已提供。

用户回复“没问题”，接受本次完整三方报告（包含旧 M0 待审变化）；接受记录和报告 SHA
保存在 `png-review-approval-1.json`。报告原始生成状态保持不变，人工接受使用独立记录。
这不放宽同一版本跨 API 的零差异要求，也不豁免剩余性能门槛。

`acceptance-queue-2` 已完成普通 PNG、报告及 GPU benchmark 构建；
`m1-full-benchmark-4/build-1` 保存双方九个 GPU benchmark 与两个诊断工具，
`build-cpu-1` 保存双方九个相同 CPU workload 的 bench-internals 构建。
正式 `timing-1` 已启动；全部初始结果、反向顺序和同版本控制仍需验证。
M0 的旧性能待决项与 M1 其他局部/resize 性能门槛保持开放，没有集成或 commit/push。


### M1 补充性能采样工具与来源核对

`performance-preparation-2` 记录 45 项纯工具测试和独立审查；本阶段没有修改候选
renderer，没有启动新的 GPU 或 Cargo 工作。主 `m1-full-benchmark-4/timing-1` 仍串行采样。

resize 入口准备了两种 API、三轮正反顺序及邻接同版本控制，共 48 个进程、96 条路线。
每条路线保留 256 个生产帧及单独的诊断 pass；PMax 时间占比来自生产最大帧本身。
结果 JSON 与日志必须绑定实际审计字节，失败启动/日志创建也保留失败记录。

共享 CPU 的 92 个 ID、实际 Criterion CLI 参数和输出格式已经核对；采样器先对双方
binary --list 作精确匹配，再保留 new→saved before→consumed reference 及全部原始分类。
额外 GPU 有 64 个比较，另加最终 M1/M0 的 8 个 filter-sampling 比较。旧 pattern 入口
没有显式选择 API，numeric 未按物理 GPU 选择；已准备双方相同的 helper 修正，并把
三个 benchmark 的原默认采样配置移至顶层，使后续长控制可由 CLI 覆盖。

修订后的额外 GPU 源码将放在独立 `m1-extra-benchmark-1`，不修改 full4。来源核对限制
四个 benchmark 的精确变换、baseline 新增相同 sampling workload 和单个 Cargo bench
条目；不能夹带依赖、feature 或 renderer 改动。uniform 局部前版本保留真实 buffer
身份及完整字节写入，恢复线性查找。该局部对照包含容器分配、查找及遍历成本，
不能将总差值全部归因于单次 lookup，也不作为完整 M0 基线。

这些补充入口仍待实际派生、构建、binary inventory、语义测试及采样；不能将工具测试
或初始 Criterion 完成状态当作性能接受。最新待办见 `performance-worklist-2.json`。
人工 PNG 接受仍有效；M0 未决控制、M1 性能、集成和 commit/push 继续等待相应验证。


### M1 局部 uniform / atlas 验证已接入串行队列

`performance-preparation-4` 保存 23 项 Python 和四项 PowerShell 纯工具测试，以及
独立审查闭环。准备 3 与首版桥接保留，首版未启动；v2 修复同一份 JSON 的解析值与
摘要可能来自不同读取的问题，并消费 preparation 的 test-runs 记录。

新局部工具另外修复三个经回归复现的问题：父源码字节先匹配冻结摘要再变换，并保留
父文件 pins；处理 libtest 测试名称与首条 adapter 元数据同一行的真实日志；测试二进制
归档为现有 idle guard 能识别的 `lib-tileink.exe`，防止控制器退出后遗漏遗留测试进程。
这些是验证工具修正，未改变候选 renderer 或已接受的 PNG。

`local-performance-queue-1` 已启动等待现有 `performance-queue-1` 成功退出，再依次
执行 uniform、atlas 的新源码派生、release 构建、真实语义测试和采样。主
`m1-full-benchmark-4/timing-1` 仍在运行，等待进程不启动额外 GPU 或构建负载。

uniform 计划双方各九项语义测试；atlas 计划在两种 API、两种纹理路径下验证共同的
三个像素用例，current 额外保留原写入范围 canary，共 18 次 CPU 和 28 次 GPU 测试。
局部性能包含 34 个 case/API，正反方向各带双方相邻同版本控制：68 个跨版本比较、
136 个同版本比较。atlas 的 100/3s/5s 是双方显式长采样设置，原文档默认仍为 10/1s/2s。

这些测试与计时尚未实际执行；队列失败即停。两侧局部源码都来自最终 M1，不能用来
冒充 M0 或完整 M1/M0 性能门槛。主矩阵 follow-up、scoped damage、M0 未决控制以及
最终性能接受仍待完成；没有集成或 commit/push。最新待办见 `performance-worklist-3.json`。


### M1 scoped damage 完整版本对照已接入串行队列

`performance-preparation-5` 保存 16 项 Python 和两项 PowerShell 纯工具检查。
独立审查修复构建启动异常时遗漏尝试记录的问题，并将例子的私有 `to_canvas` 调用
替换为公开 Canvas 命令构建的独立参考场景。候选 renderer 和已接受 PNG 均未改动。

`scoped-performance-queue-1` 已启动，持有原 local queue 的进程句柄，等待其成功退出。
随后才派生 full4 的完整 M0/M1 源码，新增同字节的原始 scoped bench、相同像素验证例子
及对应 Cargo 条目；禁止夹带 renderer、依赖或 feature 更改。两侧 release 构建、例子
格式和严格 Clippy 检查完成后，先执行两种 API × 两种纹理路径 × 两个版本的八组像素门槛。

10 个场景各覆盖初始状态与两次切换，共 240 次帧验证。每帧增量输出必须与 ForceFull
及独立 immediate Canvas 命令逐字节一致，同时检查事务版本推进、解析颜色、真正变化和
恢复；八条路线的原始 RGBA 摘要也必须完全一致。该验证独立于后续 CPU 计时。

CPU 采样沿用原始 60 samples / 3s warmup / 8s measurement，单位是一次事务提交和
materializer 更新，不是 GPU 帧。10 个 case 保留正反顺序及双方相邻同版本控制，
共 20 个跨版本和 40 个同版本比较。旧 damage/order 局部实验及 18.8% 控制漂移不重用。

目前 scoped 源码派生、真实构建、像素运行和计时均尚未执行；主矩阵仍在进行。
scoped 队列失败即停，完成也不自动接受性能。主矩阵 follow-up、M0 未决控制和最终
性能审阅仍待完成，尚未集成、commit 或 push。最新待办见 `performance-worklist-4.json`。


### M1 性能对照的透明度漏画诊断

停止旧 full4 初始矩阵并保留已完成 390 项 DX12 retained_scale 对照及全部失败记录；
后续等待队列随父任务退出，没有运行各自阶段。此前“队列运行中”文字为历史状态。

`m1-layer-semantics-3/pixels-1` 已完成两种 API × 两种纹理路径 × 两个版本的八路
核验，共 96 帧、192 份原始 RGBA。使用原 benchmark 的 1,000 节点 workload，
同一 Renderer 连续更新六帧，并与独立 ForceFull 逐字节比较。旧 M0 在透明度降低的
两帧中分别漏改 64,000（单图层）和 64（多图层）个像素；四条路径均复现，共 16 个
失败状态。M1 的全部 Auto 帧与 ForceFull 相等，所有路线的 ForceFull 原始字节相等。

根因是旧 `patch_frame_override` 用没有子命令的 layer shell 的 `visual_bounds`
覆盖输出范围，后续透明度变化不再产生损伤。M1 已有根因修正及连续更新回归测试，
本次不修改已验收的 renderer。旧版更快但漏画的数据不能证明共享模块发生同等幅度回归。
新对照只在旧 M0 恢复 layer domain，保留旧调度；其像素门槛和性能控制仍待运行。
该结论只覆盖上述两个规模，其他 workload 的初始回归仍需分别定位。

诊断工具前两次编译/Clippy 失败及第三次编辑器 Cargo 检查冲突均保留；第三次构建的
release、格式和严格 Clippy 已通过，恢复入口使用原构建，没有覆盖此前失败。
最新待办见 `performance-worklist-5.json`。M1 性能、集成和 commit/push 尚未完成。


### M1 root-opacity equivalent work and controlled timing

- `m1-layer-equivalent-2/pixels-1`: all five sizes, 8 API/texture/revision routes, 480 frames and 960 packed RGBA captures passed full-byte Auto/ForceFull and cross-route comparison. M0 has only the declared Layer output-domain correction.
- `m1-layer-equivalent-1/timing-pinned-1`: 24 processes / 24 Criterion comparisons (8 cross-version, 16 same-version). Remaining cross-version regressions: 1; same-version drift flags: 1. This subset is not the full M1 performance gate. Default-affinity `timing-1` and its two control drifts remain archived.
- The benchmark primary thread was fixed to logical CPU 2. The process and newly created worker retained all 32 logical processors; the real Rust probe reports available_parallelism=32 in both. This does not establish migration as the cause of the original drift and does not change application settings.
- A RED/GREEN regression exposed odd-sample phase bias in scale/materialize/dirty-ratio diagnostics. A shared `paired_cycles` benchmark constructor is prepared, retaining existing stress and phase-specific units. The new matrix source/build has not yet been admitted.
- Current worklist: `target/backend-parity/m1-glass-final-1/performance-worklist-6.json`. Full matrix, supplemental/local controls, M0 outstanding controls, integration and commit/push remain required.


### M1 Criterion calibration repair (2026-09-10)

- Cold first-use compilation was included in Criterion elapsed-time calibration even though its duration was excluded from the reported frame measurement. The old 1000-node opacity probe estimated roughly 1055 seconds for ten iterations, then recorded millisecond frame times with one iteration per sample.
- `m1-balanced-matrix-3/build-1` applies identical case-local prewarm before `iter_custom` to both versions, and complete paired cycles to scale/materialize/dirty-ratio diagnostic groups. Renderer code is unchanged; M0 retains the declared root-layer bounds correction. Both release builds, actual cycle/calibration tests, formatting and strict Clippy passed. Failed matrix-2 and matrix-3 preparation records remain preserved.
- `calibration-diagnosis-1` completed on the same Vulkan device with read-only clock logs. Ten M0 samples use 13..130 iterations (715 total), and ten M1 samples use 16..160 (880 total). Mean two-frame durations are 0.853069 ms and 0.846639 ms. This diagnoses recovered calibration; it is not a paired performance acceptance. Cold setup remains visible in process and clock records.
- Previous cross-version +61% and same-version -38% opacity observations remain unresolved historical measurements. They are not relabelled as passed. Full 1498 numeric plus 24 absence comparisons and all remaining controls still precede M1 acceptance.
- A read-only integration draft contains 281 operations (140 replacements, 133 additions, 8 deletions). Six existing documentation files absent from the benchmark baseline are explicitly flagged for content review. No integration, commit or push has occurred. Root progress/plan documents remain separate from the candidate copy.
- Current worklist: `target/backend-parity/m1-glass-final-1/performance-worklist-7.json`.

### M0/M1 latest continuation and M2 Metal scope

The current production candidate is `target/backend-parity/m1-stable-delta-index-3/source`. Its default release library tests (931), CPU-only release library tests (613), default strict Clippy and formatting passed. CPU-only repository Clippy passed with no new warnings compared with the predecessor; the additional strict CPU-only check still reports inherited warnings. Final runtime/precompiled pixel validation produced 15,448 outputs with zero raw RGBA difference. The four routes here are wgpu DX12/Vulkan times native/portable texture execution, not native API renderers.

The latest retained optimization reuses an unchanged empty/single-element patch index. Nonempty patch payloads and scene history remain independently owned, and larger indexes are built directly without an additional linear equality scan. The experimental profiler identity change was rejected after the one-affine-frame diagnostic regressed in both comparison orders; it is not part of the production candidate.

The remaining retained control matrix resumes through `run_m1_local_benchmarks_v107.py`: 19 prior successful processes and 288 comparisons were re-audited, and only the missing 65 processes are scheduled. The interrupted reverse-reference is retained as a failed record, not accepted data. The complete ledger will contain 84 successful processes and 1,692 comparisons, with original classifications and same-version drift preserved. Two short preflat regression signals received longer local controls with no adverse current-version comparison; this does not accept the entire matrix. The separate range-scatter source-impact review passed for its exact case.

Supplemental CPU, owned-target resize and extra GPU gates, final integration/review, and commit/push remain pending. Owned-target resize is not an application window/swapchain measurement. M0 and M1 are not yet marked complete.

The user additionally requested macOS shader support in M2. `NATIVE_BACKEND_PLAN.md` now explicitly includes Metal shader products from the shared HLSL source, target-specific compiler modules, ABI/cache/package checks and actual Mac probe validation. This does not expand M0/M1 runtime scope or claim a complete native Metal renderer. Preserve this addition when assembling the final documentation.

## M0 基线阶段关闭与 M1 最新待办

M0 的基线采集、规定参考语料校验和 pipeline/ABI/设备盘点已完成，证据见 [M0 阶段记录](docs/native/m0-baseline-closeout.md)。旧 V13 的 arena 性能观察和 root-opacity 漏画事实保留；此阶段关闭不是旧源码的独立发布接受。

M1 仍在性能修复和验收。候选3已经通过931项默认库测试、613项CPU-only测试及15448张输出零RGBA差异；剩余矩阵在2组/24进程/498比较处完整停止。当前独立探针合并旧batch分类与chunk查找，85项相关测试通过，正在正反顺序对照完整materialize和生产帧。探针尚未采用，剩余矩阵、补充验证、集成、审查和commit/push继续保留待办。上述候选3验证不自动覆盖探针。


## 2026-09-13 — macOS shader 源码策略更正

用户明确 macOS 使用独立 Metal Shading Language（MSL）源码，不使用 HLSL。本条取代此前 M2 的 HLSL→Metal 设想：DX12/Vulkan 共用 HLSL，macOS 独立维护 `.metal`；共享算法/数值规格、ABI、逻辑 program/variant 和测试合同。PLAN 第 4/5/8/10 节和 Metal 工具链补充已同步修正。M0/M1 验证继续，M2 仍未实现或在 Mac 上验收。


## 2026-09-13 — 当前状态：M4 range scatter 内核切片

以上条目为历史过程记录，不代表当前待办。Windows M3 已完成并推送
`78717916939326456f5df7819929768d094c699b`，见
[阶段记录](docs/native/m3-completion.md)。本次完成 M4 的 range scatter HLSL
内核，两种原生 API 与原生产 wgpu shader 在同 GPU 上共 684 份输出逐字节一致。
完整原生 runtime 25 项测试、956 项 release 库测试、全量 SVG/示例及严格
Clippy 已通过；3471 张既有 PNG 无变化。运行时禁用 DXC 可执行路径仍通过，
120 次原生 pipeline 缓存命中、零 pipeline 编译。

179 项迁移清单目前仅 1 项 HLSL 内核验收，下一项为 scan/cumsum。完整 M4、
公开 NativeRenderer、共享 GPU 阶段资源与后续 M5/M6 尚未完成。用户取消性能
比较；Mac 独立 MSL 和真实硬件验收仍延期。详见
[本次合同和验证证据](docs/native/m4-range-scatter.md)。


## 2026-09-13 — M4 cumsum / GPU batch progress

- cumsum 的 prefix/chunk offsets/apply 三个 HLSL 入口已在 DX12、Vulkan 和两条 wgpu 参考路径通过 144 份严格字节对照；累计 4/179 HLSL 程序已验收，M4 尚未完成。
- 原生多 buffer batch 保持中间结果在 GPU，一个批次提交整条链；DX12/Vulkan 分目录，新增模块均不使用 mod.rs。
- 按用户要求集中 CUMSUM_CHUNK_SIZE，Rust/HLSL/WGSL 共用；DX12 对齐使用 SDK 常量，Vulkan 查询设备对齐。
- 修复只读 CBV/SRV 别名的 DX12 状态合并根因，直接回归先失败再通过；两项独立审查已关闭该发现。
- 33 个原生 runtime 测试含实际 GPU 均通过；移除 DXC 可执行路径后同样通过，pipeline 编译数为零。
- 全 SVG 与 examples native/portable 通过，3471 张 PNG 文件摘要与旧基线一致。
- 详见 [cumsum 实现和验证](docs/native/m4-cumsum.md)。共享资源池与完整 NativeRenderer 接入仍在 M4 后续范围。

## Windows M4 scan continuation — 2026-09-13

- Six scan HLSL entries pass actual four-route exact-byte tests; current inventory
  is 10/179 kernel-validated, with 169 entries and full NativeRenderer integration
  remaining. See [scan scope and verification](docs/native/m4-scan.md).
- Algorithm constants now share one Rust source across host planning and generated
  HLSL/WGSL. API alignments retain their own SDK/device definitions.
- Fixed scan/cumsum stale-metadata accesses in padded WGSL groups with red/green
  four-route regressions. All 41 runtime tests and the no-DXC executable run pass.
- Full release/CPU-only tests, feature checks, strict Clippy and review pass. Full
  existing SVG/examples preserve all 3471 PNG hashes. No performance run was made.

## Windows M4 coarse allocation continuation — 2026-09-13

- Eight coarse allocation entries now pass both native APIs and both wgpu routes
  against independent CPU byte oracles (288 output executions). Current inventory
  is 18/179; complete coarse/NativeRenderer and M4 exit remain unfinished.
- Host and both shader languages share the coarse workgroup constant. Raw packed
  layouts are checked against 14 host layout facts; allocation preserves guards.
- All 44 runtime tests and their no-DXC repeat pass, with zero pipeline compiles
  on the repeat. Full release, CPU-only, feature, strict Clippy, SPIR-V and review
  checks pass. All 3471 existing SVG/example PNG hashes are unchanged.
- See [coarse allocation scope and receipt](docs/native/m4-coarse-allocation.md).
  Performance comparisons remain waived; Mac hardware validation remains deferred.


## Windows M4 coarse counting/classification — 2026-09-13

- Six further maintained HLSL entries pass four-route exact-byte tests; inventory
  is now 24/179. Full particle emission, fine/effects, shared resource pools and
  NativeRenderer/Canvas remain pending. M4 is not complete.
- Fixed demonstrated stale-capacity writes in padded WGSL particle-count groups;
  both languages guard the live contiguous reference total before reads/barriers.
- Tests cover paged draws, glyph geometry, analytic clips, wrapping totals, sparse
  17x19 grids, final empty ranges and classification precedence. Shared execution
  is separated from independent CPU oracles; HLSL helpers have explicit dependencies.
- All 52 runtime tests pass, including a no-DXC repeat with zero pipeline compiles.
  Full release, strict Clippy, header/editor/SPIR-V checks, SVG/examples and both
  reviews pass; 3471 existing PNG hashes remain unchanged.
- See [implementation and receipt](docs/native/m4-coarse-count.md). Performance
  comparisons remain waived and real Mac MSL validation remains deferred.


## Windows M4 coarse emission — 2026-09-13

- Tile, bin and chunk particle output now pass four-route complete-buffer tests;
  all coarse inventory entries are kernel-validated. Total is 27/179; fine/effects,
  shared production resources and NativeRenderer/Canvas remain pending.
- Painter order, physical capacities, logical glyph limits, nonzero brush offsets,
  negative winding, nested stack payload/order, cross-page glyph carry and sparse
  17x19 grids are covered. The count/offset/chunk-output chain stays on the GPU.
- Fixed demonstrated stale-capacity accesses in padded WGSL chunk output. Native
  chunk dispatch has a distinct entry and an explicit production-reference mapping.
- Full release, 60 runtime tests, strict Clippy, header/editor/SPIR-V checks, both
  reviews and SVG/examples pass. A post-review 17-test coarse run and 60-test no-DXC
  run pass, the latter with zero pipeline compiles. All 3471 PNG hashes are unchanged.
- See [coarse emission evidence](docs/native/m4-coarse-emission.md). M4 remains
  incomplete; performance comparisons are waived and real Mac validation deferred.


### M4 fine shared math (in progress)

Pixel, coverage and pattern-transform HLSL helpers now have four-API byte tests.
The direct endpoint correction is shared with WGSL and CPU debug; see
[the math contract and test corpus](docs/native/m4-fine-math.md).
The production inventory remains **27/179**; helper adapters are not production
fine entries. Full release, GPU, shader/editor, Clippy and SVG/examples checks passed; all 3,471 PNGs remain unchanged. Three helper tests also passed after review and without DXC, with zero runtime pipeline compilations. M4 work continues with blend modes.


### M4 blend helpers (in progress)

All mix/compose HLSL helpers are implemented and first four-route tests passed.
Independent review found and corrected shared Dodge/Burn endpoint precedence;
explicit luminosity FMA also fixes a pre-existing wgpu DX12/Vulkan byte difference.
See [blend implementation and verification](docs/native/m4-fine-blend.md).
Full release, 64 native runtime tests, shader/editor, Clippy and SVG/examples passed; all 3,471 PNGs are unchanged. Four math GPU tests passed without DXC with zero runtime recompilations. The production inventory stays at 27/179; gradient work follows.


### M4 gradient helpers (in progress)

Linear, radial, sweep and four-corner HLSL now pass 10,315 four-route packed-pixel
cases. Explicit fused evaluation removes demonstrated half-channel differences;
the sweep center has a defined zero angle. Production FineConfig is shared by
wgpu/native and its actual field offsets define the native uniform interface.
See [gradient implementation and verification](docs/native/m4-fine-gradients.md).
Full release, 65 native runtime tests, shader/editor checks, strict Clippy and
SVG/examples passed; all 3,471 PNG hashes remain unchanged. No-DXC gradient replay
passed with zero new runtime pipeline compilations. The production inventory is
still 27/179; general texture resources and full fine/effects integration follow.


### M4 compute textures

Typed RGBA8 textures now share compute batches with buffers in native DX12 and
Vulkan. Multirow uploads, storage writes, subsequent sampled reads and tightly
packed readbacks match both wgpu routes, including mixed outputs and upload-only
batches. Full release, 67 runtime tests, shader/editor checks, strict Clippy and
SVG/examples pass; all 3,471 PNGs remain unchanged. No-DXC replay compiles no new
pipelines. See [texture validation](docs/native/m4-compute-textures.md).
Production inventory remains 27/179; M4 is still incomplete.


### M4 sampled texture arrays

Explicit single/multiple-layer array views, DX12 per-layer footprints and Vulkan
array copies now match all four routes. Complete array readback and reverse layer
selection have independent byte oracles. Full release, 69 runtime tests, Clippy,
shader/editor checks and SVG/examples pass; all 3,471 PNGs remain unchanged.
No-DXC texture replay compiles no pipelines. See [array validation](docs/native/m4-compute-arrays.md).
M4 remains incomplete at 27/179; explicit sampler resources follow.


### M4 explicit sampler resources

Nearest/linear clamp samplers are explicit batch resources. Separate DX12 tables
and Vulkan sampler descriptors preserve per-pass bindings and frame ownership.
The four-route regression also found and fixed the maintained wgpu HAL ordinary
sampler comparison-field warning; explicit comparisons keep their semantics.
Full release, 71 runtime tests, shader/editor checks, strict Clippy and SVG/examples
pass; 3,471 PNGs are unchanged. No-DXC replay adds no pipeline compilations.
See [sampler validation](docs/native/m4-compute-samplers.md). M4 remains incomplete
at 27/179; production atlas pattern sampling follows.


### M4 atlas pattern sampling

Maintained HLSL atlas sampling matches production WGSL and independent CPU bytes
for 89,046 requests. Non-power-of-two repeat coverage exposed undefined mixed-sign
HLSL remainder; unsigned Euclidean correction fixes the minimal Vulkan regression.
Full release, 73 runtime tests, strict Clippy, shader/editor/SPIR-V and SVG/examples
pass; 3,471 PNGs remain unchanged. No-DXC replay compiles no pipelines. Both reviews
are closed. See [pattern validation](docs/native/m4-fine-patterns.md). Production
inventory remains 27/179; M4 continues with production filter kernels.


### M4 basic filter kernels

Six production HLSL kernels match CPU pixels and all four production WGSL variants
on all four GPU routes. Kernel-validated inventory advances to **51/179**.
Shared FilterConfig has typed signed/float/vector reflection and versioned cache
keys. The encoder derives live counts and validates region bounds and unique tile
writes. Full release (971), runtime (75), strict Clippy, shader/editor/SPIR-V and
SVG/examples pass; 3,471 PNGs are unchanged. No-DXC replay compiles no pipelines.
Both independent reviews are closed. See [basic filter evidence](docs/native/m4-filter-basic.md).
M4 continues with other filters, full fine and NativeRenderer/Canvas integration.

### M4 point, input and sampling filters

Validated inventory is **91/179** after the point/color, input-combination and
morphology/displacement/component-transfer groups. Explicit FMA and opaque-alpha
contracts resolve actual four-route byte differences. Stage-owned typed buffers
validate transfer indices, values and ownership; HLSLI owns table constants.
Full release (975), native runtime (91), strict Clippy, shader/editor/SPIR-V checks
and SVG/examples pass; 3,471 PNGs stay unchanged. No-DXC replay passes 13 filter
GPU tests with no compilation. See [sampling evidence](docs/native/m4-filter-sampling.md).
Remaining filters, full fine and NativeRenderer/Canvas integration keep M4 open.

### M4 convolution and resampling

The three entries pass all four APIs and production variants, bringing the
validated inventory to **103/179**. Reversed kernels, alpha/bias, signed wrapping,
fractional/empty rectangles, single-pixel and 2D interpolation semantics have
independent CPU oracles. Shared Euclidean remainder fixes a reproduced Vulkan
negative-wrap error. Full release (977), native runtime (99), strict Clippy,
shader/editor/SPIR-V and SVG/examples pass; 3,471 PNGs remain unchanged. No-DXC
replay passes 19 GPU filter tests without compilation. Both reviews are closed.
See [convolution/resample evidence](docs/native/m4-filter-convolve-resample.md).
M4 continues with blur and the other remaining filters, full fine and renderer integration.
