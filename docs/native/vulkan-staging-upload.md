# Vulkan staging uploads during resize

## Completed compute resource reuse

Profiling also exposed repeated device-local buffer, command pool,
descriptor pool and fence allocation. `vulkan/frame_cache.rs` retains these owners
after a successful fence wait and readback attempt. Each new recording takes exclusive
ownership of a free slot; unretired and unknown-completion submissions cannot enter
the cache. Empty submissions preserve useful storage and descriptor slots. Teardown
releases completed cache entries before destroying the device.

Device-local buffers and descriptor capacities grow geometrically. Descriptor growth
preserves previous capacity in every descriptor class and the set count, so alternating
workloads do not repeatedly shrink and recreate the pool. Command buffers, descriptor
pools and fences are reset only after confirmed retirement. Existing queue/memory
barriers remain required when persistent scratch storage crosses frame boundaries.
The cache retains the historical number of simultaneously unretired slots and their
high-water capacities. This removes allocation churn rather than moving waits elsewhere.

Immutable samplers are the exception to exclusive slot ownership: nearest and linear
each have one shared context owner, referenced by every in-flight frame that uses it.
They do not need retirement before reuse because their state cannot change. This
removes repeated sampler creation/destruction, which remained a significant driver
cost after command and storage pooling. Compute command buffers declare
`ONE_TIME_SUBMIT`: each recording is submitted once and reset only after retirement.

Focused GPU regressions cover overlapping submissions, out-of-order retirement,
empty batches, growth, alternating descriptor requirements, exact readbacks and
failed/unknown submission quarantine.

Native Vulkan previously concatenated the packed uniforms, all resource payloads and dispatch
grids into a growing frame-sized Vec, then copied that Vec into coherent staging memory. Replay
resize uploads about 16 MB per frame, making the intermediate allocation and memory copies visible
in CPU submission time.

`vulkan/upload.rs` now records borrowed payload spans and checked aligned offsets. The recorder
allocates the final staging arena once, maps it once and copies each payload directly to its final
offset. Only alignment gaps are zero-filled. Uniform placement, source offsets, little-endian grid
words, GPU transfers and synchronization remain unchanged. This removes redundant CPU staging
work at its source; it does not defer uploads or reduce rendered content.

Borrowed spans are consumed synchronously before command submission. They never become GPU
owners. The frame retains the same staging allocation until completion. The unsafe copy requires
nonoverlapping source data and a destination writable for the checked plan length; production
maps exactly that length within an exclusively owned staging arena. Empty plans are never mapped.

Unit tests compare the complete output bytes with concatenation, including alignment gaps and
empty payloads, and reject invalid alignment/overflow without mutating the plan. The Criterion
workload includes 16 MB of resource payloads, packed uniforms and dispatch-grid padding:

```powershell
$env:TILEINK_BENCH_CONCATENATED_UPLOAD = '1'
cargo bench --no-default-features --features vulkan --bench vulkan_frame_upload -- --save-baseline concatenation
Remove-Item Env:TILEINK_BENCH_CONCATENATED_UPLOAD
cargo bench --no-default-features --features vulkan --bench vulkan_frame_upload -- --baseline concatenation
```

Both microbenchmark paths prepare the same uniform prefix. This benchmark measures CPU upload
packing into ordinary memory, not GPU execution or window frame rate. The application's Replay
resize benchmark provides the separate end-to-end evidence.

Validation on Windows / RTX 4090: release tests passed (710 unit tests plus integrations),
strict Vulkan library/benchmark Clippy passed, and validation-enabled corpus comparison against
the approved M6 native Vulkan baseline found zero changed pixels in 1,712 SVGs, 45 examples and
174 retained outputs. Shader/resource/font inputs were unchanged. This is a Vulkan-only change;
no new DX12 or Metal performance result is claimed.

Criterion's concatenation reference measured 5.007 ms versus 0.423 ms for direct writes (91.8%
reduction). The actual Replay three-pair comparison reduced median per-run submission mean from
4.434 to 3.203 ms (27.8%). These are different workloads and memory destinations, so the synthetic
percentage must not be used as a frame-rate claim. Evidence is under the application checkout's
`target/agent-work/resize-deep/` (`bench-*.log`, `pair-*.txt`, `corpus/receipt.json`).

## Completed upload storage reuse

Replay resize still incurred `vkCreateBuffer` / `vkAllocateMemory` / destruction on every
frame after the CPU concatenation fix. `vulkan/staging.rs` now keeps the upload allocation
with its frame and returns it to a single context cache only after a successful fence wait
and the readback attempt (even if mapping the completed readback reports an error). The next frame takes exclusive ownership. Capacity grows to the next power
of two when needed; the cache retains at most one completed allocation, in addition to
in-flight allocations. Thus resize within capacity avoids allocation churn at its source.
Capacity is below twice the largest request for that allocation, except exact powers of two.
The last retired nonempty upload replaces the cached allocation; this is not an unbounded pool.

Empty uploads neither allocate nor consume the cache. Rejected/unconfirmed submissions,
timeouts, and frame destruction never recycle storage. Unknown completion still quarantines
in-flight resources. Normal teardown releases cached storage before destroying the device.
Every logical upload byte, including padding, is rewritten; descriptors and transfers continue
to use logical lengths. Command pools, descriptors, device-local arenas and shaders are unchanged.

The GPU regression test submits two distinct payloads concurrently, retires one, reuses its
buffer for a smaller payload while preserving the other, checks out-of-order readback, growth,
shrinkage, empty submissions and an allocation-error submission rejection followed by retry.
Run it with a pinned GPU and validation layer:

```powershell
$env:TILEINK_NATIVE_GPU = '0f42010000000000'
cargo test --release --no-default-features --features vulkan staging_reuse_preserves --lib -- --ignored --test-threads=1
cargo bench --no-default-features --features vulkan --bench vulkan_staging_reuse
```

Criterion compares exact-size fresh allocation + mapped write against completed-storage reuse
for 15/17/16 MiB uploads. It excludes queue execution and presentation. GPU resources are scoped
to the benchmark device and no buffer is in flight. Real Replay evidence is in the application
checkout under `target/agent-work/gpui-resize/`; those accepted-present intervals must not be
interpreted as physical scanout frame rate.

Criterion on the pinned RTX 4090 measured 3.8077 ms for fresh exact-size allocations and
1.9909 ms for reuse (47.7% reduction) per 15/17/16 MiB upload sequence. See
`target/agent-work/gpui-resize/criterion.log` in the application checkout.

Final validation: Vulkan release suite and the pinned validation-enabled staging reuse test
passed; strict library/benchmark Clippy passed. The full Vulkan corpus matched the approved M6
baseline byte-for-byte: 1,712 SVGs, 45 examples and 174 retained outputs (1,931 total, zero diffs).
gfx_ui passed 779 unit tests, two integration tests and its doctest, including the real-window
backend acceptance test. This optimization and its performance evidence are Vulkan-only.
