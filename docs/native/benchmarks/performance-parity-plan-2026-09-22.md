# Native backend performance parity plan

This plan follows the complete Windows RTX 4090 rerun in
`full-windows-2026-09-22.md`. The objective is exact-pixel parity and native
DX12/Vulkan time no greater than wgpu on the same API for every comparable
workload. Rank **confirmed, repeatable additional mean time per frame** before
relative ratio. Use three alternating-order runs on the same GPU and API to
promote or close a candidate. A broad sweep alone is a discovery pass, not a
stable measurement; GPU clock, shader-cache, and case-order effects can reverse
its ranking. Record the measured distribution and rerank after each fix.
The raw, sorted inventory of all 62 positive single-sweep comparisons across
basic, retained, immediate, and pipelined suites is in
`performance-parity-discovery-inventory-2026-09-23.csv`. All four sweeps are
from September 23. The retained benchmark now
omits the already-present 100,000-node tier at the user's request; 1,000 nodes
was already a separate tier, and historical receipts remain unchanged. Do not
use the raw ordering as the execution queue: a 20,000-node fragmentation
outlier reversed on DX12 in a three-run repeat, and short warmup can exaggerate
large-update cases.

| Rank | Issue | wgpu → native | Extra time | State |
| --- | --- | ---: | ---: | --- |
| Recheck | 20,000-node all-revisions update, DX12 | 43,900 → 45,225 µs | +1,325 µs | Three alternating 100-frame-prewarmed exact-pixel runs after idle-buffer retention (`target/perf-allrev-after-idle-cache-1000/receipt.json`). Stage sampling found ~24 ms rebuilding chunks and ~6.6 ms updating frame metadata in the shared materializer on both routes. Shared `Canvas`/plan reference counts were identical, and reversing route order moved the ~1 ms stage difference with clock variation. No backend-specific materializer root cause is confirmed; keep this candidate open without speculative algorithm changes. |
| Recheck | 3840×2160 dense immediate, DX12 | 6,580 → 6,754 µs | +174 µs | Three-run median after idle-buffer retention; median p95 6,782 → 7,073 µs. One of three runs reverses the mean sign (`target/perf-immediate-after-dx12-idle-cache-1000/receipt.json`). |
| Recheck | 3200 tiger immediate, DX12 | 11,758 → 11,905 µs | +147 µs | Three-run median after idle-buffer retention; one later profile run reverses the sign by ~0.7 ms, exceeding the gap. Resolve GPU/order variance before editing the shader. |
| Closed | Filter resize immediate, DX12 | 703 → 578 µs | −125 µs | Three alternating exact-pixel runs after retaining completed idle DX12 buffers; median p95 1,101 → 927 µs (`target/perf-immediate-after-dx12-idle-cache-1000/receipt.json`) |
| Recheck | 1600×1000 dense, 64 broad clips, DX12 | 5,580 → 5,975 µs | +395 µs before dense slot preallocation | The newer complete single sweep no longer flags this case; repeat it three times before keeping it ranked |
| Closed mean/p95; recheck PMax | One layer edit among 20,000 layers, DX12/Vulkan | DX12 701 → 610 µs; Vulkan 778 → 492 µs | Native faster | Latest three alternating runs with 100-frame warmup and exact pixels (`target/perf-many-layer-direct-root-resources-1000/receipt.json`); DX12 median p95 1,094 → 964 µs, but median PMax 1,174 → 1,179 µs is effectively tied. Earlier DX12 was 599 → 779 µs (`target/perf-many-layer-update-focused-1000/receipt.json`). |
| Closed | 32 root layers add/remove, DX12 | 2,127 → 972 µs | −1,156 µs | Current three alternating 100-frame-prewarmed exact-pixel runs; Vulkan 1,855 → 650 µs (`target/perf-retained-confirmed-after-direct-root-1000/receipt.json`) |
| Closed | 30% dirty retained update, DX12 | 980 → 720 µs | −260 µs | Same focused receipt; Vulkan 803 → 675 µs |
| Closed | 3840×2160 sparse immediate, DX12 | 3,843 → 1,549 µs | −2,294 µs | Broad clips now schedule only child-content tiles; p95 4,008 → 1,604 µs and PMax 4,076 → 1,905 µs, exact pixels |
| Closed | Same sparse immediate, Vulkan | 2,814 → 1,178 µs | −1,636 µs | Three alternating runs, p95 3,020 → 1,339 µs, exact pixels |
| Closed | 1600×1000 sparse, 64 broad clips, DX12 | See `target/perf-immediate-after-content-budget/receipt.json` | Native faster | Complete four-route sweep after content culling; sparse family has no positive single-sweep gap |
| Closed | Same tiger immediate, Vulkan | 11,512 → 10,569 µs | −943 µs | Was +1,437 µs; three-run mean, p95 and PMax faster; exact pixels |
| Closed | Same dense immediate, Vulkan | 5,055 → 4,598 µs | −458 µs | Was +10,010 µs; 0.909× and p95/PMax faster; three exact-pixel runs |
| Recheck | 20,000-node all-revisions update, Vulkan | 44,033 → 44,499 µs | +466 µs | Latest three-run median differs from earlier native-faster result; a paired submit/wait sample also reverses the sign (`target/perf-allrev-after-idle-cache-1000/receipt.json`, `target/perf-allrev-paired-submit-wait-idle-cache-1000`). Do not change Vulkan based on this unstable gap. |
| Closed | Same sparse immediate, Vulkan | 2,903 → 2,739 µs | −164 µs | Was +4,974 µs; now 0.943× with p95/PMax faster and exact pixels |
| Observe | 20,000-node fragmentation | DX12 495,177 → 493,334 / 493,576 → 490,393 / 489,786 → 492,729 µs | Mixed sign | Single sweep suggested +9.3 ms, but the three-run DX12 gap reversed twice. Vulkan excess was only 0.1%–0.6% of a ~490 ms case; do not promote without more evidence |
| Closed | Large two-frame burst, Vulkan | 3,039 → 2,895 µs | −144 µs | Was 1.036×; staging reuse makes it 0.953× |
| Closed | Sparse two-frame burst, Vulkan | 112 → 69 µs | −43 µs | Was 1.506×; staging reuse makes it 0.615× |
| Closed | DX12 image replacement; Vulkan large basic scene | 8,106 → 7,959 µs; 3,936 → 3,742 µs | −147 / −194 µs | Three full alternating-order runs; all 11 basic cases pass on both APIs |
| Closed | Basic suite and four pipelined outliers | 0/11 basic slower on either API; image replacement 2-frame DX12 3,797 → 3,711 µs | No repeated pipelined regression in the four largest candidates | September 23 complete sweep and three-run pipelined top-case repeat; exact pixels |
| Reproduce | Large-chunk revision, Vulkan | Latest single sweep is 7,637 → 7,315 µs | −322 µs | The old Vulkan outlier reversed; isolate before promotion |
| Closed | 128 root layers add/remove, DX12/Vulkan | DX12 10,005 → 2,866 µs; Vulkan 9,442 → 1,834 µs | Native faster | Medians of three 100-frame-prewarmed alternating exact-pixel runs supersede the single-sweep DX12 +571 µs signal (`target/perf-root-128-confirmation/receipt.json`) |
| Closed | Clip count/depth/area matrix, 16 cases | Native/wgpu ratio ≤0.767 DX12, ≤0.803 Vulkan | All faster | Three new alternating-order runs: 16/16 means, p95, and PMax faster on both APIs; exact pixels |

The latest complete immediate discovery sweep, after dense clip-slot
preallocation, found **3/62 DX12** cases slower and **0/62 Vulkan** cases slower
(`target/perf-immediate-after-dense-preallocation/receipt.json`). The single-run
outliers are filter-resize, 3200 Tiger, and 3840×2160 dense. The latter's sign
reversed in three alternating runs, though its p95 and PMax remain slower.
The previous eight-case discovery inventory is retained below as historical
context; do not use its order as the current queue.

| Discovery rank | DX12 immediate case | Extra µs, single sweep |
| ---: | --- | ---: |
| 1 | root-tiger-3200-b8 | +1,246 |
| 2 | root-dense-3840x2160-b32 | +1,168 |
| 3 | root-dense-1600x1000-b64 | +627 |
| 4 | root-dense-2560x1440-b32 | +432 |
| 5 | root-dense-1601x1001-b32 | +431 |
| 6 | root-dense-1600x1000-b32 | +388 |
| 7 | filter-resize | +142 |
| 8 | root-dense-1600x1000-b16 | +53 |

## Execution sequence

The next GPU timestamp sample isolated Tiger's steady native DX12 frame:
eight fine passes take about 3.09 ms, eight coarse count passes 0.74 ms, and
eight coarse emit passes 0.79 ms. Pure clip scenes already have disjoint
particle ranges in their tile-bin records, so native DX12/Vulkan now reserves
those tile slots once and emits dense batches directly into them, removing the
count and prefix dispatches. Sparse clip selection still uses its existing
per-tile path. An unchanged slot-header snapshot avoids another upload on
later frames; a changed layout or a non-preallocated frame invalidates it.
This removes redundant work at its source, not a case-specific workaround.
The focused red-then-green pass-count test and reused-renderer GPU test cover
the two paths. Three alternating exact-pixel runs put Tiger's DX12 mean gap at
+295 µs rather than +734 µs and make the 4K dense mean 388 µs faster than wgpu,
although both retain worse p95/PMax. The full immediate sweep moves from 8 to
3 slower DX12 cases out of 62; Vulkan remains 0/62. The retained discovery
sweep moves DX12 from 42 to 21 slower cases out of 154 and Vulkan from 5 to 4,
but its top remaining gaps require repeated measurement before attribution.
Receipts are `target/perf-dense-preallocated-cached-repeat/receipt.json`,
`target/perf-immediate-after-dense-preallocation/receipt.json`, and
`target/perf-retained-after-dense-preallocation/receipt.json`. Three full
seven-route acceptance rounds passed: 45 example images, 174 retained images,
and 1,712 SVG images per route, all exact pixels. The first native DX12 round
also matches the pre-change images byte for byte, so this optimization does not
introduce new PNG changes.

The 20,000-layer one-layer edit had a separate CPU recording bottleneck.
After 100-frame prewarm, a 32-frame DX12 probe measured native submission at
~467 µs versus wgpu's ~356 µs; the native scene update itself took only ~5 µs.
Temporary stage timing isolated ~307 µs in native `scan_scene`, including about
133 µs rescanning direct-root operations for filter resources and 34 µs for
filter paths even though there were no offscreen operations. Clip dispatch
construction also walked the large opacity-only plan, and native converted the
whole layer stack on every one-entry edit while wgpu patched the dirty range.
The prepared clip depth now skips clip dispatch at depth zero; shared layer
staging converts only dirty ranges; direct-root plans skip offscreen filter and
path resource construction. These remove redundant CPU work at the source,
not a timing-specific workaround. Focused red-then-green tests cover the
range update and direct/offscreen distinction; the existing 20,000-layer
Criterion case is the performance regression. Three alternating four-route
exact-pixel runs moved native DX12 from ~779 to ~610 µs and Vulkan from ~692
to ~492 µs; both now beat same-API wgpu on mean and p95, while DX12 PMax is
effectively tied. Receipts are `target/perf-many-layer-update-focused-1000`,
`target/perf-many-layer-no-clip-dispatch-1000`,
`target/perf-many-layer-dirty-layer-stack-1000`, and
`target/perf-many-layer-direct-root-resources-1000`. Temporary profilers were
removed after sampling.

After the idle-buffer and shared materializer changes, three full Windows
seven-route acceptance rounds passed for examples, retained, and SVG: 21/21
routes per suite, with 45, 174, and 1,712 exact-pixel images per route. The
native DX12 RGBA files match the preceding accepted version byte for byte in
all three suites, so no new PNG review is needed. Receipts are
`target/perf-allrev-redundant-work-accept-examples-v2/receipt.json`,
`target/perf-allrev-redundant-work-accept-retained/receipt.json`, and
`target/perf-allrev-redundant-work-accept-svg/receipt.json`.

The shared all-revisions chunk rebuild also performed two dependency-index
updates per changed node even when neither dependency membership nor execution
plan changed. Re-encoding now refreshes the chunk's backdrop geometry but
updates nonlocal membership only across empty/nonempty transitions and
reclassifies surface-dependent membership only when the plan fingerprint
changes. The existing backdrop enter/leave regression, a new same-plan backdrop
revision test, and the retained-scene release tests pass. This is a source-level
removal of redundant work. A second pass also skips empty Backdrop command-tree
scans when the plan fingerprint and dependency-free state are unchanged, while
existing Backdrops still recompute their bounds. This is **not a measured parity
win yet**; keep the all-revisions DX12/Vulkan rows open until
the deferred final comparison measures them.
The frame-patch path also now reuses the node and fixed translation domain it
already carries, avoiding two further scene-node hash lookups per changed node.
The bounded-translation spatial regression covers the retained patch domain;
the all-revisions Criterion case remains the deferred performance gate.
DX12 command recording also allocated a `BTreeMap` for every compute pass merely
to combine descriptor access states. It now reuses a resource-indexed scratch
table across passes and sorts only distinct touched resources, preserving
CBV/SRV alias unions and texture-table deduplication. Focused tests cover both
and verify that the next pass cannot inherit stale states. This is a source-level
reduction in repeated allocations; its effect on Tiger, 4K dense, and
all-revisions remains unmeasured until the final comparison.
The affected native DX12 route then passed three independent full examples,
retained, and SVG corpus runs (45, 174, and 1,712 images respectively). All
nine runs are byte-identical to the preceding accepted DX12 corpus; the binary
and current 6,627-file source snapshot were checked. Receipt:
`target/perf-dx12-required-states-accept/receipt.json`. Other routes retain the
preceding full three-round seven-route acceptance because their code did not
change in this step.

Retained chunk re-encoding previously resized the old canvas before resetting
it. During a simultaneous viewport resize and node revision, that recomputed
path backdrop/segment allocations only to discard them. The encoder now clears
old records first and sets the new extent before appending the replacement;
the focused fresh-versus-reused path-record regression covers the resulting
chunk data. A permanent 100/1,000-node Criterion resize-plus-revisions case
guards this path. This removes redundant work by construction, with timing
deferred to the final benchmark pass. DX12 then passed three full pixel/API
validation rounds across its four routes and all three corpora (45 examples,
174 retained images, and 1,712 SVG images per route); native DX12 output was
also byte-identical to the preceding accepted version for every image. Receipt:
`target/perf-resize-reencode-dx12-accept/receipt.json`. An initial all-route
attempt stopped because `VK_LAYER_KHRONOS_validation` was not installed. The
official LunarG 1.4.357.0 SDK was then copied into the ignored `target/`
directory without global registration, and its `Bin` directory supplied through
`VK_LAYER_PATH` only during acceptance. Three complete Vulkan pixel/API
validation rounds then passed across wgpu Vulkan native/portable, native Vulkan,
and the wgpu DX12 reference (36/36 route executions over examples, retained,
and SVG). The native Vulkan output for all 1,931 images per run also matches
the preceding accepted version byte for byte. Receipt:
`target/perf-resize-reencode-vulkan-validated-v2/receipt.json`.

A temporary wgpu GPU-stage timestamp probe for Tiger/4K dense was removed:
its instrumented stage sums exceeded the ordinary completed-frame times, so
the absolute values cannot isolate a reliable backend-specific shader gap.
No shader algorithm change is justified by that probe.

Do not start another full benchmark sweep while confirmed performance issues
are still being optimized. Use only focused
profiling to identify a root cause and focused Criterion checks required to
validate an individual fix; run the complete benchmark inventory once after
the issue queue is closed, as requested.

DX12 `filter-resize` exposed a separate CPU recording bottleneck. A current
three-run comparison put native at 903 µs against wgpu's 715 µs. Submit/wait
sampling assigned about 530 µs to native submission, and temporary phase
timers traced 0.2–0.3 ms per frame to destruction of completed upload/storage
buffers that a smaller alternating frame did not consume. The DX12 buffer pool
now retains those unused completed buffers, including across an empty batch,
with a peak-frame-capacity/count bound. In-flight buffers remain unavailable
until their fence completes. A pinned-GPU regression test was red before the
change and green after it; the existing in-flight/resize/signal-failure test
also passed. Three alternating exact-pixel runs moved filter-resize to native
578 µs versus wgpu 703 µs with p95 927 versus 1,101 µs. Tiger and 4K dense
still show smaller DX12 mean gaps and require separate GPU-stage work. Receipts:
`target/perf-immediate-confirmed-after-direct-root-1000`,
`target/perf-immediate-after-dx12-idle-cache-1000`; temporary timing code was
removed after sampling.

1. **Normalize and rank.** The all-revisions case exposed a warmup asymmetry:
   native materialization fell from ~30 to ~19 ms only after roughly 80 dense
   updates, while wgpu's longer startup reached a stable CPU state sooner. Keep
   cold-start latency separate from steady-state throughput. The retained
   four-route harness now accepts an explicit, recorded `--prewarm-frames` count;
   the top case was repeated three times at 100 frames on every route. The
   receipt is `target/perf-all-revisions-ranked-repeat/receipt.json`.
2. **Fix the immediate family first.** DX12 GPU timestamps isolated the 4K
   regression to incremental `coarse_count` and `coarse_emit`: together ~10.3 ms
   for 30 broad clip batches. The native clip preselection covered 12,496–31,654
   of 32,400 tiles per batch, replacing the wgpu-style 135-bin dense path with
   per-tile work. A one-third density gate, a red-then-green unit regression,
   and three exact-pixel four-route runs removed ~10–11 ms from both APIs.
   The 3200 tiger workload initially spent ~1.9 ms/frame reuploading unchanged
   lines through native `CachedBuffer`. Content comparison now skips those
   accepted uploads, with a red-then-green GPU regression that also checks a
   subsequent in-place edit. A second profile found another 9.36 MB unchanged
   tile-bin upload per frame, consuming ~0.9 ms of CPU preparation; an exact
   snapshot comparison now skips it on DX12/Vulkan while preserving changed
   bins and failed-submit recovery. The scene-work GPU test was red then green
   on both APIs, and three exact-pixel runs reduced the Tiger gap to ~1.20 ms
   DX12 and ~0.11 ms Vulkan
   (`target/perf-tiger-bin-cache-after/receipt.json`). A later profile isolated
   ~326 µs/frame in native cumsum metadata copying and validation. Cumsum now
   borrows the already-owned arrays and avoids sorting in the ordered case,
   while still validating unsorted, overlapping and missing ranges. Its
   constructor fell to ~120 µs, and the final three-run four-route comparison
   moved Tiger to +386 µs DX12 and −943 µs Vulkan, with exact pixels
   (`target/perf-root-cumsum-official/receipt.json`). The same comparison puts
   DX12 4K sparse at +696 µs and dense at +453 µs. An earlier 32-frame
   diagnostic found 4K DX12 CPU submission faster than wgpu but GPU completion
   wait ~1.0–1.2 ms longer. Resolve that GPU wait first; use DX12 GPU timestamps
   around copies, coarse count/emit, and fine passes before changing barriers
   or shaders. Then profile the smaller Tiger CPU/GPU remainder separately.
   GPU timestamp sampling found fine_tile_main dominating the remaining 4K
   native time (~3.0 ms sparse, ~4.0 ms dense). Declaring the read-only coarse
   input as an SRV instead of a UAV passed four-route pixel checks but did not
   improve three-run DX12 latency (`target/perf-fine-coarse-readonly-trial`), so
   the experiment was reverted. Branch/early-return fine-shader experiments
   also failed to improve the repeated comparison and were reverted. The
   rectangle-SDF early-load experiment passed exact pixels but changed the
   three-run 4K gap inconsistently, so it too was reverted
   (`target/perf-sdf-rect-load-trial`). Future fine-stage work should target
   measured shader instruction count or clip-content tile culling rather than
   source-level load ordering. A conservative rounded-rect interior fast path
   also preserved exact four-route pixels but left the sparse 4K DX12 gap at
   ~694 µs (`target/perf-sdf-interior-fastpath-trial`), so it was reverted.
   The remaining sparse case's root cause was different: each broad clip
   scheduled tiles across the clip's full bounds even where no child draw
   existed. Clip selection now unions the child draw bounds per layer stack
   and clips that union to the clip bounds. A candidate-tile work budget
   preserves the dense path when querying the child bounds would cost more
   CPU work than it saves GPU work. This content restriction applies only to
   complete repaints with nonempty child batches: retained damage can expose
   previous content after a reparent, and an empty clip batch can still supply
   a mask for later draws. The retained acceptance caught both distinctions at
   `06-reparent-into-clip`; focused active-damage and empty-batch regressions
   were red then green, and all 174 DX12 retained images now match wgpu.
   Three alternating
   four-route exact-pixel
   runs moved 4K sparse DX12 from 3,843 to 1,549 µs relative to wgpu and
   Vulkan from 2,814 to 1,178 µs; p95 and PMax improved too
   (`target/perf-clip-content-tiles-budget/receipt.json`). The complete
   immediate sweep has no sparse native regression on either API
   (`target/perf-immediate-after-content-budget/receipt.json`). Dense 4K and
   Tiger remain the highest confirmed DX12 gaps. A new three-run dense 4K
   profile (`target/perf-dense-4k-final-profile/receipt.json`) places the DX12
   median CPU submit at 2,012 µs wgpu versus 1,779 µs native, but completion
   wait at 3,997 versus 5,108 µs. Vulkan native has a shorter submit and
   comparable wait. The residual DX12 issue is GPU execution/synchronization,
   not CPU command recording; collect per-pass DX12 GPU timestamps before
   changing barriers or fine-shader code.
   A one-run red-capable parity probe reproduced native DX12 at 1.103× wgpu
   (`target/perf-dense-4k-red-loop-1/receipt.json`). One frame records 318
   repeated UAV barriers, mostly across 32 clip batches. Skipping them reduced
   native frame time by roughly 0.6 ms but corrupted pixels, so they cannot be
   removed wholesale (`target/perf-dense-4k-skip-uav-trial`). Batching the same
   barriers per pass preserved exact pixels, but native remained about 6.84 ms
   in three runs, unchanged from the pretrial 6.84 ms; the batching experiment
   was reverted (`target/perf-dense-4k-batched-barrier-trial/receipt.json`).
   A temporary DX12 timestamp probe put steady 4K dense GPU work at ~5.36 ms:
   32 `fine_tile_main` passes consumed ~4.24 ms, coarse passes ~1.07 ms, and
   warm resource upload ~3–4 µs. Its instrumentation was removed. Raising the
   sparse clip threshold to two-thirds made coarse binning much slower and was
   reverted (`target/perf-single-rect-threshold-two-thirds-trial/receipt.json`).
   Separating broad-clip fine selection from dense coarse binning preserved
   four-route pixels but did not improve the 4K DX12 gap and worsened its
   Vulkan time in a one-run trial (`target/perf-fine-only-broad-clip-trial/receipt.json`).
   The 16-case clip matrix moved similarly on wgpu and native in the trial,
   consistent with run-to-run GPU variance rather than a measured win, so this
   trial was also reverted (`target/perf-fine-only-clip-matrix-trial/receipt.json`).
   Continue at the fine shader's GPU instruction cost rather than broadening
   the selected tile lists. A resource-specific barrier inventory found 318
   repeated UAV barriers per frame, including 31 on the four-byte no-spill
   fine scratch buffer. Skipping only those 31 kept four-route pixels exact
   but changed native DX12 from 6,893 to 6,868 µs in a single run, well below
   run-to-run variance; this temporary trial was reverted
   (`target/perf-dense-4k-barrier-inventory/receipt.json`,
   `target/perf-dense-4k-no-spill-barrier-trial/receipt.json`).
   The latest paired three-run comparison still places Tiger and 4K dense
   ahead of the remaining confirmed gaps. A new 32-frame submit/wait split
   puts 4K DX12 native submission at 1.81 ms versus wgpu's 1.96 ms, but
   native completion wait at 5.13 ms versus 4.01 ms. Tiger submission is
   similar (5.41 vs 5.52 ms) and native wait 5.08 vs 4.86 ms
   (`target/perf-immediate-top-two-submit-wait`). Target GPU execution and
   synchronization, keeping the Criterion result as the acceptance measure.
   A DXIL shader-model 6.6 trial retained exact pixels but moved the single-run
   4K dense native mean from 6.81 to 7.32 ms while wgpu stayed near 6.8 ms;
   Tiger's native time was approximately unchanged. The trial was reverted
   (`target/perf-dxil-sm66-trial/receipt.json`).
   Temporary DX12 GPU timestamps for Tiger's steady frames show eight fine
   passes at ~3.09 ms total, eight coarse count passes at ~0.74 ms, eight
   coarse emit passes at ~0.79 ms, cumsum ~0.22 ms, and scan passes ~0.25 ms.
   Fine therefore accounts for roughly 58% of ~5.3 ms timed GPU work, while
   coarse count and emission account for about 29%. The instrumentation was
   removed after the exact-pixel four-route profile
   (`target/perf-tiger-3200-gpu-stage-profile/receipt.json`).
3. **Eliminate the confirmed all-revisions residual.** The 32-frame submit/wait
   diagnostic (`target/perf-all-revisions-profile-submit-wait`) shows that DX12
   native CPU submit is ~25.64 ms versus wgpu ~22.95 ms, offset by a shorter
   wait (2.56 versus 3.12 ms). For Vulkan, CPU submit is close (25.62 versus
   25.29 ms), but native completion wait is ~3.09 versus ~0.90 ms. These are
   distinct root-cause investigations: profile Vulkan GPU pass occupancy and
   transfer/synchronization first, then DX12 preparation and command recording.
   Temporary Vulkan timestamp queries isolated ~2.4–2.9 ms of the ~2.7–3.3 ms
   GPU batch before the first compute shader. The cause was 40,000 tiny copies
   of a 3.04 MB dirty journal. `CachedBuffer::upload` now chooses one contiguous
   copy when the changed byte count plus per-region transfer cost exceeds the
   current buffer contents. The focused GPU test was red then green on DX12 and
   Vulkan, and three alternating exact-pixel runs moved Vulkan to 0.989× wgpu
   and reduced the DX12 gap to ~579 µs
   (`target/perf-all-revisions-dense-upload-after/receipt.json`). Recheck the
   Vulkan p95 tail and profile the remaining DX12 CPU submission cost.
   The post-clip-content three-run repeat showed no stable DX12 gap and a
   ~620 µs Vulkan median gap (`target/perf-all-revisions-current-confirmation/receipt.json`).
   In a fresh 32-frame split, native Vulkan submitted in 32.48 ms versus
   wgpu's 34.84 ms but waited 2.79 ms versus 0.51 ms; completed median totals
   were 35.28 versus 35.37 ms. This contradicts a simple GPU-only explanation
   of the Criterion gap (`target/perf-all-revisions-submit-wait-current`).
   Reconcile the differing measurement paths before editing the renderer.
   Temporary stage timers showed native materialization stabilizing near 19 ms,
   native scene recording near 3 ms; they were removed after diagnosis. Every
   candidate fix must recheck both APIs and exact pixels.
4. **Close already fixed paths.** The original 20,000-layer plan-COW regression
   and the Vulkan two-frame staging-cache regression have focused regression
   tests and exact-pixel Criterion gains. A newer 100-frame-prewarmed repeat
   still found a +226 µs DX12 layer-update residual
   (`target/perf-retained-top-prewarm100/receipt.json`), ranked above. Repeat
   3/4-frame bursts and p95/PMax after all changes; finish release tests and
   SVG/example acceptance.
   The 16-case clip count/depth/area matrix is already closed after the latest
   upload changes (`target/perf-clip-matrix-current/receipt.json`): all 16 cases
   have faster native mean, p95, and PMax on both DX12 and Vulkan.
5. **Remaining broad-sweep candidates.** Isolate retained large-chunk and
   root-layer cases, then triage every case whose native mean exceeded wgpu in
   the full sweep, using three alternating-order runs. Promote every repeatable
   regression into the queue above, ordered by additional microseconds, and
   continue until none remain. The September 23 single-sweep inventory has 0/0
   basic, 42/5 retained, 8/0 immediate, and 7/0 pipelined slower cases on
   DX12/Vulkan. These are candidates, not confirmed regressions: the largest
   cropped-filter, fragmentation, dirty-immediate, and pipelined outliers
   reversed in isolated repeats.
   Rebuild this inventory after each fix because a confirmed root cause can
   close many cases at once.

For each confirmed issue: establish a red performance loop; profile before
changing code; add a focused semantic regression test for the real mutation;
make one root-cause fix; verify exact pixels and Criterion improvement; run
release tests, formatting, clippy, and the required SVG/example checks for
rendering changes. Record before/after mean, p95, and maximum latency. A row
closes only when the three-run median native mean is no greater than wgpu on
the same API, p95/PMax do not materially regress, and output pixels remain
byte-for-byte equal across all four routes.

After the clip-content fix and its retained edge-case corrections, three-run
examples, retained, and full SVG acceptance passed all 21 route executions per
suite, with API validation and byte-exact pixels across 45 examples, 174
retained images, and 1,712 SVG images per route
(`target/perf-content-tiles-accept-examples-final-code/receipt.json`,
`target/perf-content-tiles-accept-retained-final-pass/receipt.json`,
`target/perf-content-tiles-accept-svg-final/receipt.json`).
Supply the local Vulkan layer at
`target/toolchains/vulkan-validation-1.4.357.0/Bin` as `VK_LAYER_PATH`.

After the queue is empty, rerun all 42 declared benchmark targets, the
16-case clip matrix, and the full four-route comparison inventories. Repeat
any new slower case, fix confirmed cases, and publish the final receipts. This
Windows run does not certify AMD/Intel, Linux, or macOS.
