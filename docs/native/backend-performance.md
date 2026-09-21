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
raw samples, and fails if sources change or any of the 80 output images differ.
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

All routes render the same 384 colored retained leaves at 1280x800: unchanged;
one translated leaf; all translated leaves; a triangular 1280x800 to 1216x760
resize cycle; and all translated leaves with eight local Gaussian blur layers.
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

Excluding unchanged frames, native DX12 reduced median means by 16.8?55.3%, and native Vulkan by 13.6?56.9%. These are completed offscreen frame costs, including retained transactions, rather than pure GPU timestamps or presented-window FPS.

Per-frame latency in milliseconds: median of each run's p95, followed by the **worst observed maximum across all three runs**. Each run sampled 400 warmed frames per case; maxima are not hard upper bounds.

| Case | wgpu DX12 p95 / max | Native DX12 p95 / max | wgpu Vulkan p95 / max | Native Vulkan p95 / max |
|---|---:|---:|---:|---:|
| unchanged | 0.0004 / 0.0015 | 0.0003 / 0.0004 | 0.0016 / 0.0267 | 0.0003 / 0.0009 |
| sparse | 0.4964 / 1.2094 | 0.2177 / 0.5953 | 0.2973 / 1.8456 | 0.1517 / 1.7955 |
| full | 0.8549 / 1.3642 | 0.7465 / 1.6343 | 0.8925 / 2.1801 | 0.6623 / 1.1269 |
| resize | 0.5487 / 1.9928 | 0.2475 / 0.5601 | 0.2795 / 0.6273 | 0.1932 / 0.3894 |
| blur | 3.3838 / 4.8382 | 1.5836 / 2.4694 | 2.6813 / 3.4078 | 1.1375 / 1.9631 |

All native p95 medians are lower. Worst observed maxima do not all win: native DX12 full-update reached 1.6343 ms versus wgpu-DX12 1.3642 ms. Native Vulkan sparse updates still showed roughly 1.7?1.8 ms outliers, also present in the wgpu-Vulkan comparison. These observations are retained; no samples were removed to satisfy parity.

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
