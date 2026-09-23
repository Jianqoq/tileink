# Windows backend benchmark rerun, 2026-09-22

The native backends do **not** yet beat wgpu in every workload on this machine. All
four routes produced identical captured pixels in the comparable suites, but some
native cases remain slower after three runs with alternating route order.

## Environment and coverage

- Source commit: `74d9e4f8` (`codex/png-pixel-check`); uncommitted source files were
  unchanged throughout the benchmark runs.
- Windows, NVIDIA GeForce RTX 4090, driver 610.62, physical GPU identity
  `fe3e010000000000`; pinned DXC v1.8.2502.
- All 42 declared Cargo benchmark targets ran in release mode: four four-route
  comparison suites and 38 specialized targets. The specialized targets completed
  970/970 expected Criterion cases. The optional clip matrix added 16 cases.
- Every comparable case ran on wgpu-DX12, native DX12, wgpu-Vulkan, and native
  Vulkan. The comparison receipts passed their exact-pixel checks.
- Ratios below are native time divided by wgpu time for the same API and case;
  a value below 1 means native is faster. “Broad sweep” is one full pass. Selected
  cases were repeated three times, reversing route order on the second pass;
  their reported ratio uses the median of each route's three means.

## Results

| Suite | Cases | Native slower in broad sweep, DX12 / Vulkan | Repeated finding |
| --- | ---: | ---: | --- |
| Basic | 11 | 1 / 2 | DX12 `image_replace` 1.136×; Vulkan `large` 1.212× |
| Retained | 181 | 51 / 36 | `scale-many-layer-update-20000`: DX12 8.203×, Vulkan 7.315× |
| Immediate | 62 | 54 / 52 | Three selected apparent regressions reversed on isolated rerun |
| Pipelined | 44 | 6 / 21 | Vulkan `large-burst-2` 1.211× and `sparse-burst-2` 1.527× |
| Clip count/depth/area matrix | 16 | 0 / 0 | All native cases faster in the broad matrix |

The retained multi-layer case builds 20,000 opacity layers and changes one layer
between frames. It is the strongest repeatable regression in this run. The
selected pipelined Vulkan regressions also repeated; DX12 ratios for those same
two cases were 0.966× and 0.413×.
For the retained multi-layer case, medians of the three reported p95 values
were 8.270× (DX12) and 6.576× (Vulkan), while medians of the three per-run
maximum latencies were 8.486× and 6.825× respectively.

The broad-sweep results are order-sensitive. For example,
`scale-cropped-filter-child-revision` appeared 7–11× slower in the full retained
sweep, but isolated reruns at 100, 1,000, and 100,000 nodes found native faster
on both APIs (DX12 0.824×, 0.744×, 0.703×; Vulkan 0.660×, 0.627×, 0.558×).
Likewise, three immediate cases that appeared much slower in the full sweep
were faster in isolated reruns: DX12 0.615–0.700× and Vulkan 0.739–0.931×.
The full-sweep counts therefore identify cases needing investigation, not a
reliable count of steady-state regressions. Changing the selected case inventory
also changes preceding GPU/cache work, so the discrepancy may reflect workload
history rather than ordinary measurement noise; this run did not isolate that
cause.

## Local receipts

- `target/full-benchmark-2026-09-22-micro/receipt.json` — all 38 specialized
  targets and 970 cases. The receipt's case counts were reconciled from full
  Criterion logs because some Criterion outputs put `time:` on a continuation
  line.
- `target/full-benchmark-2026-09-22-basic/receipt.json` and
  `target/full-benchmark-2026-09-22-basic-repeats/receipt.json`.
- `target/full-benchmark-2026-09-22-retained/receipt.json` and
  `target/full-benchmark-2026-09-22-retained-repeats/receipt.json`.
- `target/full-benchmark-2026-09-22-immediate/receipt.json` and
  `target/full-benchmark-2026-09-22-immediate-repeats/receipt.json`.
- `target/full-benchmark-2026-09-22-pipelined/receipt.json` and
  `target/full-benchmark-2026-09-22-pipelined-repeats/receipt.json`.
- `target/full-benchmark-2026-09-22-clips/receipt.json`.

These results cover one Windows NVIDIA GPU. They do not establish AMD/Intel,
Linux/macOS, or real-window resize p95/PMax performance. The specialized
microbenchmarks are diagnostics and do not independently compare native with
wgpu under the same workload.
