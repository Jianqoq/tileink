# Vulkan staging uploads during resize

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
maps exactly that length in a newly allocated staging arena. Empty plans are never mapped.

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
