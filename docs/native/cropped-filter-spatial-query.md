# Native cropped-filter candidate selection

Native filter localization uses the prepared scene's tile draw index, as wgpu
already does. Passing the entire scene draw order to the localizer forced every
cropped filter to test every draw on every invalidation, even if nearly all draws
were outside the filter surface. For queries within the indexed domain, this
change removes that full-scene traversal at the source; the exact localizer
still checks bounds and preserves draw order.

`SceneUploadStaging` owns an `Rc<TileDrawBins>` and each native recorded `Scene`
shares its matching prepared index. Mutable preparation uses copy-on-write, so
an older recorded scene cannot observe new physical indices, extents or painter
ranks. Normal frame teardown drops the scene handle before the next preparation,
leaving the cache uniquely owned and avoiding an index clone. Nested filter
contexts swap the entire scene, including its own local-coordinate index, then
restore their parent's scene. No root-coordinate index is used for a child.

The regression `filter_candidates_are_spatial_and_keep_recorded_scene_identity`
first failed because an out-of-region draw was returned. It checks sparse/empty
regions and two recorded scenes with different coordinates while both are alive.
Existing shared tile-index tests cover ordering and persistent/flat storage;
full native corpus and nested-filter GPU tests guard final pixels.

Queries extending beyond the indexed tile domain conservatively return the full
painter order. Off-canvas source pixels can enter visible output through offset,
blur or shadow; clipping the candidate query to the viewport would incorrectly
lose them. The exact localizer performs the final bounds checks. This guard is
shared with wgpu and fixes that pre-existing query limitation there too.
`filter_candidates_keep_offcanvas_sources` first failed with an empty candidate
list; `offset_filter_preserves_offcanvas_input` checks exact expected GPU pixels
for source rectangles beyond all four canvas edges on every backend.

## Windows repeated performance results (2026-09-22)

RTX 4090, driver 610.62, LUID `fe3e010000000000`. Nine cases per route,
three independent repetitions, serialized wgpu DX12/native DX12/wgpu
Vulkan/native Vulkan (reverse order in repetition two). The table contains
median Criterion mean completed-frame times in milliseconds. Each iteration
includes two workload mutations, render/submit and completion; means are divided
by two. Initialization and image capture are outside timing. Temporary diagnostic
probes were removed before these measurements.

| Case | wgpu DX12 | Native DX12 | wgpu Vulkan | Native Vulkan |
|---|---:|---:|---:|---:|
| scale-affine-clip-update-100000 | 0.4367 | 2.2476 | 0.2943 | 1.7528 |
| scale-backdrop-manual-invalidation-100000 | 0.4796 | 0.2495 | 0.3485 | 0.2049 |
| scale-cropped-filter-manual-invalidation-100 | 0.5417 | 0.2775 | 0.3513 | 0.2013 |
| scale-cropped-filter-manual-invalidation-1000 | 0.6974 | 0.4068 | 0.5166 | 0.3561 |
| scale-cropped-filter-manual-invalidation-100000 | 1.5667 | 0.5271 | 1.6491 | 0.4740 |
| scale-cropped-filter-manual-invalidation-20000 | 0.7669 | 0.5219 | 0.7435 | 0.4326 |
| scale-cropped-filter-manual-invalidation-5000 | 0.7795 | 0.4839 | 0.5766 | 0.3957 |
| scale-manual-invalidation-100000 | 0.2647 | 0.1915 | 0.1714 | 0.1377 |
| scale-one-move-100000 | 0.3290 | 1.4980 | 0.1735 | 1.4828 |

For 100,000-node cropped-filter invalidation:

- native-dx12: median per-run P95 0.6898 ms; largest observed maximum 1.0489 ms. Samples per run: [64, 64, 64].
- native-vulkan: median per-run P95 0.7141 ms; largest observed maximum 1.0853 ms. Samples per run: [64, 64, 64].

Tail summaries are separately collected completion observations, not worst-case
bounds or window FPS. An earlier same-session DX12 before/after Criterion pilot
at 100,000 nodes measured 1.5416 to 0.6189 ms/frame, a 59.9% decrease with
reported 95% change interval [-63.8%, -55.5%]. The pre-fix historical three-run
results were 4.3891 ms (native DX12) and 4.9766 ms (native Vulkan); these used a
different selected case sequence. Run-order/context effects remain relevant:
use each contemporaneous table for native/wgpu comparison rather than treating
all historical numbers as identical conditions.

All five cropped-filter sizes beat their matching wgpu mean in this repeated
matrix. This does not establish universal native parity: move and affine-clip
updates remain separate outstanding workloads, and a nine-case sequence is not
the entire retained corpus or an isolated-case timing contract.

Validation: three feature-specific single-threaded release suites, strict
all-target Clippy and formatting passed; native sibling/nested-filter GPU tests
passed; exact expected four-edge Offset pixels passed on all four routes.
The four-route full SVG/examples/retained corpus compared 7,724 outputs without
any differences from existing references. All captured benchmark phases matched
raw RGBA exactly across routes and repetitions.

[Per-run means, intervals, latency summaries and pixel hashes](benchmarks/cropped-filter-2026-09-22.json).
Full source snapshots/raw latency remain under `target/cropped-filter-final`;
checks are in `target/cropped-filter-final-checks`, corpus receipts in
`target/remaining-corpus/cropped-*`. [Previous manual-invalidation results](manual-invalidation-performance.md).
