# DX12 upload resource reuse

Replay resize previously called `CreateCommittedResource` for every constant/data upload,
even after the previous frame's GPU work had completed. The DX12 compute recorder now consumes
an exclusively owned list of completed upload resources, reuses each sufficiently large buffer,
and grows undersized storage to the next power of two. This removes repeated upload heap
allocation at its source; it does not change shader work, scene fidelity, or presentation policy.

The cache matches upload slots in recording order, not resource identities. Every logical payload
is rewritten before submission and every descriptor/copy retains its logical offset and length.
The resources stay in `GENERIC_READ`. Uniform padding comes from the existing packed uniform
layout. Device-local resources and texture upload footprints retain their existing allocation
and synchronization paths.

Each in-flight frame owns its upload list. Only a confirmed fence wait followed by the readback
attempt transfers it back to the context, even if completed readback mapping itself fails.
Drop, failed Signal, invalid completion and timeouts never recycle uploads; the complete COM-owner
aggregate still quarantines unknown in-flight work. No new waits are introduced. Cache size is
bounded to one retired frame's upload list, plus separately owned in-flight frames; replacement
releases the former list and recording discards unused slots. Geometric capacities are less than
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
