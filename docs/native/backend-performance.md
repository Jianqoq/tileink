# Native versus wgpu backend comparison

The September 2026 follow-up explicitly restores performance comparisons after the
earlier M0/M1/M6 closeout waived them. It compares native DX12 with wgpu-DX12 and
native Vulkan with wgpu-Vulkan on the same explicitly selected physical GPU.

Run from the checkout, with an unused ignored evidence directory:

```powershell
python scripts/native/benchmark.py --gpu 0f42010000000000 --dxcompiler target/toolchains/dxc-v1.8.2502/bin/x64/dxcompiler.dll --output target/backend-comparison/final --runs 3
```

The runner builds each mutually exclusive feature, records source and executable
hashes, reverses route order between repetitions, preserves Criterion estimates and
raw samples, and fails if sources change or any of the 176 output images differ.
Run without other GPU workloads, compilation or profiling. `passed` certifies
execution and exact pixels; `mean_parity` separately requires every median native
mean to be no higher than its corresponding wgpu mean. P95/max remain explicit
observations rather than universal latency guarantees.

Each Criterion iteration includes a complete 16-frame phase cycle. Reported means
and confidence intervals divide cycle time by 16. Each frame includes the retained
transaction, CPU recording/submission and GPU completion/retirement. Device/pipeline
creation, shader compilation, captures and Criterion analysis are outside timing.
After analysis, 64 warmup frames precede 400 individual completed-frame latency
samples. This is an offscreen renderer benchmark, not a window swapchain/FPS test.

The original five cases render the same 384 colored retained leaves at 1280x800: unchanged;
one translated leaf; all translated leaves; a triangular 1280x800 to 1216x760
resize cycle; and all translated leaves with eight local Gaussian blur layers.
The extended matrix adds 384 moving text labels using bundled Noto Sans, 384 distinct bilinearly sampled images, 384 circular clips, 384 eight-segment cubic paths, and 4096 moving small rectangles. These are warmed retained updates; they do not measure cold glyph/image uploads or text shaping on every frame.
An eleventh case continuously replaces all 384 image resources with fresh revisions.
See the [expanded coverage and current results](backend-performance-followup.md).
Every phase is captured separately and checked for exact logical dimensions and
raw premultiplied RGBA equality. Transparent RGB is included.

## Root causes removed

Native rendering previously repeatedly allocated filter scene buffers, GPU work
storage, descriptor and command resources. Pinned scratch leases could disappear
from the pool. The new caches retain owners until confirmed retirement and then
reuse them, with failure quarantine unchanged. GPU-owned work no longer receives
redundant full CPU zero uploads; CPU-owned regions are explicitly patched.
Vulkan also shares immutable nearest/linear samplers across in-flight frames;
descriptor/command owners remain exclusively leased until retirement. Batching
descriptor update API calls was tested and rejected because Criterion showed no
stable benefit.

Owned targets now use the same geometric capacity policy as wgpu. Kernels, external
copies and captures use logical extents. A cropped capture copies into an exact-size
temporary texture before readback; it never rescales spare capacity. Completed owned
frames with no damage skip submission only when prior completion was observed and
there is no readback, external output or synchronization request. Pending work still
receives a real receipt.

GPU regressions cover these ownership and extent contracts. Performance results
must be paired with full SVG, examples and retained-sequence pixel validation.

## Windows RTX 4090 results (2026-09-20)

NVIDIA RTX 4090, driver 610.62, physical LUID `0f42010000000000`. Three repetitions per route, with the second in reverse order. All 12 runs completed and all 80 phase images in every run matched exactly. Every native case mean was lower than its same-API wgpu counterpart in every repetition.

Median of three Criterion per-frame means, in milliseconds (lower is better):

| Case | wgpu DX12 | Native DX12 | wgpu Vulkan | Native Vulkan |
|---|---:|---:|---:|---:|
| unchanged | 0.000294 | 0.000196 | 0.001196 | 0.000196 |
| sparse | 0.275944 | 0.153315 | 0.193502 | 0.116515 |
| full | 0.568881 | 0.473313 | 0.487102 | 0.420895 |
| resize | 0.315906 | 0.195666 | 0.229359 | 0.154921 |
| blur | 3.012443 | 1.346819 | 2.285418 | 0.983983 |

Excluding unchanged frames, native DX12 reduced median means by 16.8-55.3%, and native Vulkan by 13.6-56.9%. These are completed offscreen frame costs, including retained transactions, rather than pure GPU timestamps or presented-window FPS.

Per-frame latency in milliseconds: median of each run's p95, followed by the **worst observed maximum across all three runs**. Each run sampled 400 warmed frames per case; maxima are not hard upper bounds.

| Case | wgpu DX12 p95 / max | Native DX12 p95 / max | wgpu Vulkan p95 / max | Native Vulkan p95 / max |
|---|---:|---:|---:|---:|
| unchanged | 0.0004 / 0.0015 | 0.0003 / 0.0004 | 0.0016 / 0.0267 | 0.0003 / 0.0009 |
| sparse | 0.4964 / 1.2094 | 0.2177 / 0.5953 | 0.2973 / 1.8456 | 0.1517 / 1.7955 |
| full | 0.8549 / 1.3642 | 0.7465 / 1.6343 | 0.8925 / 2.1801 | 0.6623 / 1.1269 |
| resize | 0.5487 / 1.9928 | 0.2475 / 0.5601 | 0.2795 / 0.6273 | 0.1932 / 0.3894 |
| blur | 3.3838 / 4.8382 | 1.5836 / 2.4694 | 2.6813 / 3.4078 | 1.1375 / 1.9631 |

All native p95 medians are lower. Worst observed maxima do not all win: native DX12 full-update reached 1.6343 ms versus wgpu-DX12 1.3642 ms. Native Vulkan sparse updates still showed roughly 1.7-1.8 ms outliers, also present in the wgpu-Vulkan comparison. These observations are retained; no samples were removed to satisfy parity.

## Verification and evidence

- Release tests ran single-threaded for `wgpu`, `dx12`, and `vulkan`; strict release Clippy passed on all targets for each feature. Focused GPU regressions passed on both native APIs, including failure quarantine, owned-target cropping, command/storage reuse and immutable sampler sharing.
- Both native APIs matched the approved corpus byte-for-byte: 1,712 SVGs, 45 examples and 174 retained outputs each. Vulkan corpus validation was repeated after the sampler change, with synchronization validation enabled. No PNG approval was necessary because there were zero changed pixels.
- Local evidence: `target/backend-comparison/final-three-rounds/receipt.json` (source/executable hashes, all Criterion estimates and raw latencies); `final-corpus-native-dx12/receipt.json`; `sampler-final-corpus-native-vulkan/receipt.json`. The latter two paths are also under `target/backend-comparison/`. Renderer and benchmark sources were frozen during sampling; this result section was added afterward.
- Criterion trials in `sampler-cache/run.log` confirm significant improvement from sampler reuse. `batched-bindings/run.log` records the rejected descriptor-update batching experiment.

## Review

### Standards

The descriptor-pool growth finding was fixed: every descriptor class and the set count retain high-water capacity, with a failing-then-passing alternating-demand regression. Final review has zero unresolved findings.

### Spec

Final ownership, failure/quarantine, scratch initialization, logical output extent and sampler lifecycle review has zero unresolved findings. Performance acceptance is limited to the measured Windows GPU and these workloads; Metal and other physical GPUs were not benchmarked.

A final direct Criterion comparison against the matching wgpu baseline reported `Performance has improved` for all five cases on each native API (10/10), with no regression verdicts; see `target/backend-comparison/criterion-verdicts/`. Its 80 images per route also matched. The earlier sampler trial reported a 1.8% unchanged-frame fluctuation, which did not recur in the frozen three-round result (native Vulkan median 0.196 microseconds); that diagnostic log is preserved.

## Coverage follow-up

The five-case result above is not certification of every repository benchmark.
Cargo declares 39 benchmark targets: 18 CPU/general targets, 15 wgpu-only targets,
5 native-only targets and the four-route comparison target. GPU timings from old
profiled suites include timestamp readback and cannot be substituted for the
completed-frame contract used here. Multi-frame throughput, window presentation,
cold process startup and other resource replacement scenarios require separate
measurements. Image replacement and the existing wgpu dirty-ratio/filter/scale
runs are now documented in the [expanded results](backend-performance-followup.md);
native comparisons for those wgpu-only matrices remain additional work.

Running CPU benchmarks with `dx12,bench-internals` exposed three feature-gate
errors: the uniform benchmark export referenced a disabled module, the tile-bin
helper inherited a wgpu-only HashSet import, and dirty-list transfer was hidden
from native dependency builds. Their gates/imports now match the helpers' actual
availability. `tests/benchmark_helpers.rs` exercises a normal dependency build so
`cfg(test)` cannot hide this regression. This fixes benchmark compilation, not a
renderer performance bottleneck; production backend paths are unchanged.

Follow-up native DX12 full-update profiling separated transactions, materialization,
recording, queue submission and retirement. Observed spikes were predominantly in
CPU submission/materialization rather than GPU completion waits. Fixed-core trials
lowered some p95 observations but increased maxima, and were removed. There is no
production affinity policy or claim that scheduler outliers have been eliminated.

### Expanded workloads and fragmented uploads

The first comparison expansion added retained text, 384 unique image resources, generic
circular clips, eight-cubic paths, and a 4,096-leaf scene. That initial ten-case matrix used
16-phase cycles and required all 160 raw RGBA images to match. The current
eleven-case matrix adds continuous image replacement and checks 176 images. Text uses the
bundled Noto Sans font. Text and images measure warm retained translations, not
cold shaping, decoding or resource replacement.

Three baseline rounds exposed native DX12 paths at 1.275 times the corresponding
wgpu mean, and clips at 1.033 times. Native Vulkan means were lower in all ten
cases. This invalidates any extension of the earlier five-case claim to arbitrary
workloads. The evidence is `target/backend-followup/extended-three-rounds/`.

The path upload contained 1,542 CopyBufferRegion calls for only 144,128 bytes per
frame. Three instrumented runs spent 341-371 microseconds recording these copies;
map/copy/unmap staging work took 13-18 microseconds. Native DX12 now coalesces
adjacent ranges and uses the existing GPU range-scatter kernel for fragmented
uploads with at least 16 disjoint copies. This removes the CPU API-call bottleneck
without overwriting GPU-owned gaps. Small or unaddressable uploads retain ordinary
copies. Vulkan keeps its native batched-copy path.

Uploads precede previously recorded commands, and submission receipts own both
source and destination until retirement. Tests cover packet validation, ordering,
non-overlapping ranges, preserved gaps and queued readbacks after dropping the
public buffer. The header/descriptor strides are defined in
`shared/range_scatter_constants.hlsli`; HLSL, generated WGSL and host packing and
validation consume that source. No ABI JSON or alternate shader fallback is used.

The isolated Criterion trial reduced paths from 1,088 to 717 microseconds (34.1%)
and clips from 24.80 to 24.37 milliseconds (1.75%), with exact pixels. These trials
are not the final multi-round parity result. Adjacent-copy merging alone and
batched DX12 resource barriers showed no significant improvement; the barrier
experiment was removed. Temporary profiling instrumentation is also removed.

### Persistent benchmark sessions

The old retained-scale main series reconstructed and warmed its scene on every
Criterion sample. For 100,000-node fragmentation this requested roughly 986
seconds for ten samples, largely repeating work outside the reported interval.
The main paired-cycle series now keeps a PersistentSession per scenario/size:
renderer, scene and mutation cursor survive between samples. Warmup and complete
paired mutations remain explicit. The standalone benchmark helper delegates to
the same session while preserving its previous one-call behavior.

A GPU regression checks warmup followed by consecutive and zero-frame samples;
the cursor must advance continuously and each requested frame must be measured.
This fixes harness overhead rather than production rendering. The new series
measures sustained history instead of fresh-per-sample history; compare it under
a new baseline name. Single-phase insert/remove and profiled stage series retain
their existing setup policy. Profile wall excludes transactions and includes
instrumentation; production wall includes transactions, rendering and completion
without profile readback. These metrics are not interchangeable.
