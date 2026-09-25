# DX12 upload resource reuse

## Completed command and descriptor reuse

Profiling found that creating descriptor heaps, command allocators and
command lists could cost more CPU time than scene recording. The context now retains
each completed frame's descriptor tables and command owners. A successful fence wait
is required before resetting the allocator/list or overwriting descriptors; failed
Signal and unknown completion continue to quarantine owners. Empty batches do not
evict useful descriptor heaps. Heap capacities grow geometrically within API limits.
The cache retains the historical peak of simultaneously unretired slots.

The GPU regression verifies concurrent isolation, reuse after retirement, empty
submissions, heap growth, exact readback and failed-Signal quarantine. This is a
root-cause allocation fix with no new waits or changes to shader/presentation behavior.

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
```

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

Buffer state rule: [Microsoft resource state decay documentation](https://learn.microsoft.com/en-us/windows/win32/direct3d12/using-resource-barriers-to-synchronize-resource-states-in-direct3d-12#state-decay-to-common).

The multi-frame regression retires two in-flight submissions together and verifies that both
subsequent submissions reuse their original upload and DEFAULT resources. It failed with the
single-list cache and passed with the free-list pool.
## Final multi-frame validation

Release tests passed together with the pinned GPU regression and strict DX12
library Clippy. gfx_ui real-window resize/capture and its release suite passed.
All 1,931 DX12 corpus outputs remained byte-identical to approved M6 references.
