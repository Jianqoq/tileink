# DX12 upload resource reuse

Replay resize previously called `CreateCommittedResource` for every constant/data upload,
even after the previous frame's GPU work had completed. The DX12 compute recorder now consumes
an exclusively owned list of completed upload resources, reuses each sufficiently large buffer,
and grows undersized storage to the next power of two. This removes repeated upload heap
allocation at its source; it does not change shader work, scene fidelity, or presentation policy.

The cache selects the smallest available capacity that fits, independent of recording order.
Capacity is captured with the COM resource at creation; selecting a completed list sorts its
existing vector in place, without a per-frame description query or intermediate vector.
Previously, consuming cached resources in recording order let a small upload take a large
buffer, forcing a later large upload to allocate again when resource order changed. Every logical payload
is rewritten before submission and every descriptor/copy retains its logical offset and length.
The resources stay in `GENERIC_READ`. Uniform padding comes from the existing packed uniform
layout. Texture upload footprints retain their existing allocation and synchronization paths.
Device-local buffer reuse is described below.

Each in-flight frame owns its upload list. Only a confirmed fence wait followed by the readback
attempt transfers it back to the context, even if completed readback mapping itself fails.
Drop, failed Signal, invalid completion and timeouts never recycle uploads; the complete COM-owner
aggregate still quarantines unknown in-flight work. No new waits are introduced. Each retirement returns its own list to a free pool. Recording consumes one list when needed;
a new list is allocated only when none is free. The list count is bounded by the historical peak of simultaneously unretired
submissions needing that heap class, including GPU-complete submissions not yet read back.
Idle pools retain this high-water capacity. Recording discards unused resources within the chosen list. Geometric capacities are less than
twice their allocation request except exact powers of two. Empty batches and batches referencing
only persistent textures leave the cache intact, preventing a no-op from evicting reusable storage.

The GPU regression covers concurrent in-flight isolation, reuse after retirement, out-of-order
readback, shrinking/growing payloads, multiple resources, empty submissions and failed Signal
following Execute. The empty-cache preservation assertion first failed against the prototype,
then passed after the root-cause fix. Fault injection runs in an isolated child process.

```powershell
$env:TILEINK_NATIVE_GPU = '0f42010000000000'
cargo test --release --no-default-features --features dx12 staging_reuse_preserves --lib -- --ignored --test-threads=1
cargo bench --no-default-features --features dx12 --bench dx12_staging_reuse
```

Criterion compares exact-size fresh committed upload allocations and mapped writes with the
same writes to reusable resources for 15/17/16 MiB payloads. No GPU execution or presentation is
included; the Replay application benchmark is separate. DX12 Auto remains VSync in gfx_ui, while
Vulkan Auto selects supported Mailbox, so cross-backend results are not a policy-controlled API
comparison. Evidence is in the application checkout under `target/agent-work/dx12-resize/`.

Validation on RTX 4090: release suite passed (709 unit tests plus integrations), the isolated
staging reuse/failure regression passed, strict library/benchmark Clippy passed, and gfx_ui
passed 778 unit tests, two integration tests and its doctest including real-window acceptance.
The full native DX12 corpus matched the approved M6 reference byte-for-byte: 1,712 SVGs,
45 examples and 174 retained outputs, zero differences. See `dx12-resize/corpus/receipt.json`.

Criterion measured 4.8584 ms for fresh exact-size committed allocations versus 1.8479 ms for
reuse per three-upload sequence (62.0% lower). The application result is smaller: three alternating
400-frame Replay pairs reduced median per-run interval p95 from 38.209 to 26.149 ms, per-run
maximum from 46.179 to 29.499 ms, and submission mean from 14.282 to 10.682 ms. These are
accepted-present intervals, not physical scanout measurements. Fixed-count runs traverse different
amounts of the time-driven drag path; each validates actual HWND resize progress. The 144 Hz
6.944 ms target remains unmet. Remaining resize waits and device-local allocation costs are
separate optimization work, not resolved by this upload cache.


## Completed device-local buffers

After upload reuse, owned `Resource::Buffer` payloads still created a fresh DEFAULT-heap resource
per frame. The recorder now reuses a separate retired list of DEFAULT/UAV buffers by capacity. Only non-uniform owned buffers enter this list; externally owned persistent buffers,
packed uniforms, textures and readback resources do not. Logical contents are copied in full
before any shader access. Capacities grow geometrically but copies, readbacks and shader-visible
logical parameters retain their requested lengths.

D3D12 buffers decay to COMMON when ExecuteCommandLists completes, including after explicit
transitions and readback. Recycling requires the subsequent fence to complete, so no extra
end-of-frame COMMON barrier is needed. Immutable upload uniforms remain GENERIC_READ. A frame owns both
resource lists exclusively; successful fence completion and readback attempt return them to the
context independently. A batch without device storage leaves that pool intact. Every completed
slot survives a multi-frame drain; keeping only the last slot previously forced the next pipeline
refill to allocate again. Unknown submission completion quarantines both lists.
This addresses device allocation churn; it neither removes necessary GPU work nor skips resize.

The existing GPU regression now checks both upload and device resource identities, concurrent
isolation, growth/shrinkage, reordered resources, out-of-order retirement and empty preservation.
The reordered-resource identity assertions failed with sequential matching and pass with best fit. Its new reuse
assertion caught missing retirement wiring in the prototype before the fixed version passed.
The Criterion `dx12_device_buffer_reuse` group measures fresh versus reused DEFAULT allocations
for 32 buffers near 512 KiB each; it excludes GPU execution and upload copies.

Buffer state rule: [Microsoft resource state decay documentation](https://learn.microsoft.com/en-us/windows/win32/direct3d12/using-resource-barriers-to-synchronize-resource-states-in-direct3d-12#state-decay-to-common).

The multi-frame regression retires two in-flight submissions together and verifies that both
subsequent submissions reuse their original upload and DEFAULT resources. It failed with the
single-list cache and passed with the free-list pool. `dx12_resize_pipeline_refill` compares the
previous last-retired-only policy with retaining all free slots for two 9 MiB upload frames.
It measures CPU allocation/map/copy/retirement only, without GPU execution.


## Final multi-frame validation

Release tests passed (709 unit tests plus integrations), together with the pinned GPU regression
and strict DX12 library/benchmark Clippy. gfx_ui real-window resize/capture and its release suite
passed. All 1,931 DX12 corpus outputs remained byte-identical to approved M6 references.

Final Criterion: resize pipeline refill 1.8759 ms with the last-slot-only policy versus 0.68815 ms
with all completed slots (63.3% lower). Reusing 32 DEFAULT buffers took 0.563 us versus 8.5835 ms
for fresh allocations; this excludes GPU execution and upload copies. Compared with the prior
committed helper in the same session, single-frame uploads improved from 1.9073 to 1.8647 ms.
The intermediate capacity-index vector had shown a small regression and was replaced with
capacity stored directly beside its COM owner. Unchanged allocation controls varied between runs.

Three alternating final Replay pairs reduced median per-run mean from 18.299 to 8.788 ms, p95
from 25.713 to 18.006 ms, and submission mean from 10.607 to 3.276 ms. Maximum did not improve:
median per-run max rose from 31.641 to 32.661 ms. Queue/resize waiting still dominates the tail;
this cache change does not meet the 144 Hz tail target. Accepted-present intervals are not
physical scanout frame rates. See the application's `docs/replay-resize-native-performance.md`
and `target/agent-work/dx12-device-reuse/` for raw traces and verification receipts.
