# Shared native filter tile validation

`program/filter/region.rs` records filters for Metal, DX12 and Vulkan. Each clear,
blur/copy and composite pass previously rebuilt a `BTreeSet` and uploaded an
independent copy of the same active tile list. Maximized AAPL Replay on Apple M5
sampled 535 tree-insertion stacks out of 1080 inclusive submission stacks. This
change removes the repeated work at the shared recording layer; it is a root-cause
CPU/upload fix, independent of GPU coarse scheduling.

## Required invariants

- Each `ComputeBatch` owns a content-keyed cache. Exact list values **and order**
  determine identity; keys own their contents. Equal separately allocated lists
  may reuse storage. A reused pointer/length cannot establish identity.
- A cache miss validates every ID against the current logical tile count and
  checks uniqueness with an expected-linear hash set. Nonmonotonic sparse IDs
  remain legal. Hash lookups still read the list; reuse is not constant-time.
- Cache entries retain the maximum ID. Every cache hit rechecks that maximum
  against the current logical tile count, including smaller logical domains in
  larger pooled allocations. Surface resource ownership and pixel/region bounds
  continue to be checked independently for each pass.
- Validation returns a batch-owned proof borrowing the immutable source list.
  Upload is separate and occurs only after input and dispatch-geometry checks.
  Empty or invalid input does not allocate GPU resources or record a pass.
  A proof from another batch is rejected before allocation. Independently prepared
  proofs for equal lists must converge on one uploaded buffer.
- Uploaded tile buffers are immutable snapshots used only as read bindings by
  these filters. Later input mutation creates a different cache entry or fails
  validation; it cannot change bytes referenced by previous passes. List order,
  unique pixel writers, ordered pass dependencies and inactive history are preserved.
- Reuse ends with the compute batch; it does not share resource handles across
  submissions. `None` retains dense-region semantics, distinct from an explicit
  empty active list. Checked integer addressing and empty-list descriptors remain
  valid at the zero and `u32::MAX` boundaries.

Metal, DX12 and Vulkan adapters all upload each batch resource once and resolve
repeated `ResourceId` bindings to the same GPU allocation. No adapter or shader
change is needed for this optimization.

## Real workload validation, 2026-10-01

Same release benchmark driver and GPU coarse fix (base `70005fd1`), Apple M5,
macOS 27.0 (26A428), Rust 1.98.1, GPU `000000010000058b`. Native maximization gives
3420x1966 backing pixels, with real AAPL.POLYGON data (780 candles), 120 Hz timed
horizontal input and 400 accepted presents after two seconds of warmup. Three
alternating candidate/baseline pairs ran without compilation or profilers.

| Pair | Submission mean, ms | Render total mean, ms | Interval mean, ms | Interval p95, ms |
|---|---:|---:|---:|---:|
| 1 | 5.020 → 2.889 | 6.111 → 4.281 | 16.664 → 16.661 | 18.231 → 18.481 |
| 2 | 5.015 → 2.886 | 6.158 → 4.345 | 16.663 → 16.664 | 18.065 → 18.235 |
| 3 | 5.055 → 2.819 | 6.132 → 4.230 | 16.667 → 16.668 | 18.213 → 18.100 |

Median per-run means: submission falls **42.5%**, render total **30.2%**. Uploaded
bytes fall from 1,768,974 to 1,593,211 per frame (about 176 KB, 9.9%). Accepted-present
cadence remains about 60/s on this 60 Hz display. Interval tails do not consistently
improve; these results establish CPU headroom, not increased visible FPS or measured
input-to-photon latency.

A separate five-second CPU sample contains no tree-insertion stacks (previously
535 of 1080 inclusive submission samples). The new validator appears in 10 of
552 submission samples. Scene preparation and Metal command recording are now
larger branches. Sampling is diagnostic evidence, not an elapsed timer; no new
GPU trace or additional optimization gain is claimed.

Seven new release regressions cover content/order reuse, range/duplicate rejection,
in-place mutation, batch ownership, delayed upload, independent proofs, empty/error
atomicity and integer boundaries. The shared physical-GPU test executes ordered
clear/copy/clear passes over a reverse sparse list, preserving inactive pixels and
partial edge tiles, and reads back after dropping the CPU batch. It is wired into
each legal backend's GPU test suite; only Metal was run on this Mac.

Metal API-validated, serial release all-targets: **894 passed, six existing failures**.
The same five coarse glyph/clip oracle differences and 1600-pixel turbulence canonical
mismatch were already reproduced before this change. Two tests needing unavailable
DXC tools and one missing historical WGSL reference are explicitly excluded. All
1712 SVG fixtures plus tiger and both blur qualities match the frozen baseline
byte-for-byte. Native presentation completes eight frames and two sizes. Formatting
and release all-targets Clippy pass with existing warnings, no new warning.

The earlier [Metal scalar coarse schedule](metal-tbdr.md#large-sparse-regular-coarse-scheduling)
remains Metal-only. The HLSL scalar counter used by DX12/Vulkan currently derives IDs
from dense bin geometry; it does not map an incremental sparse active list as the
MSL counter now does. Enabling the Metal planner decision there requires corresponding
HLSL semantics and GPU regression/performance measurements on those platforms.
This shared CPU optimization applies to all three; DX12/Vulkan hardware performance
and a coarse-scheduling change are not certified by a Metal run.
