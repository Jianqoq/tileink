# Native retained tile-bin delta uploads

In a retained scene, changing one draw dirties only the tile records and fixed-size
index pages that contain it. The native scene recorder previously transferred the
complete tile index arena on every frame. In the 100,000-draw move workload that
was 4,210,688 index bytes per frame, although the changed draw touched only a
few pages. CPU staging and GPU upload submission, rather than GPU execution,
caused the native performance gap.

The native recorder now consumes the same sorted dirty record/page journal used
by wgpu. It coalesces adjacent IDs into upload ranges and writes only those
ranges into queue-ordered persistent work storage. A full CPU snapshot is still
used when the tile layout changes, GPU storage is new, or the previous recording
was not accepted. Thus a failed or abandoned batch cannot make a later delta
depend on missing GPU contents. The scene's other CPU-owned work regions retain
their existing upload behavior. This fixes the redundant transfer at its source;
it is not a temporary workload-specific bypass.

`sparse_tile_bin_uploads_only_changed_records_and_pages` checks coalescing,
offsets and transferred bytes. The retained journal test covers repeated
changes and abandoned recordings. Native DX12 and Vulkan release unit suites,
the wgpu release unit suite, and strict all-target Clippy pass. The complete
native DX12 and Vulkan SVG/examples/retained corpora each match the approved
reference exactly: 1,712 + 45 + 174 outputs per route, with zero changed pixels.
The two measured workloads also match byte-for-byte across wgpu DX12, native
DX12, wgpu Vulkan and native Vulkan for all captured phases.

## Windows measurements (2026-09-22)

RTX 4090, LUID `fe3e010000000000`. Isolated Criterion measurements with ten
samples per route. Each iteration includes two alternating mutations, render,
submit and completion; the table divides Criterion mean by two. Initial render
and image capture are outside timing.

| Workload, 100,000 draws | wgpu DX12 | Native DX12 | wgpu Vulkan | Native Vulkan |
|---|---:|---:|---:|---:|
| Move one draw | 0.3329 ms | 0.1347 ms | 0.2261 ms | 0.1061 ms |
| Update affine clip | 0.4506 ms | 0.2630 ms | 0.2829 ms | 0.1867 ms |

The earlier repeated native measurements were 1.4980/1.4828 ms for the move
case and 2.2476/1.7528 ms for the affine-clip case (DX12/Vulkan). They came
from a different case sequence, so the table above is the contemporaneous
native-versus-wgpu comparison. A nine-case native retained sweep also matched
all 27 captured phases per route. A separate three-run DX12 check of the cropped
filter 1,000-draw case measured 0.3600, 0.3603 and 0.3609 ms/frame.

Criterion logs and raw captures are under `target/move-fixed-*` and
`target/move-matrix-*`; full-corpus receipts are under
`target/remaining-corpus/move-final-native-*`. These are completed-frame
benchmarks, not window frame-rate or tail-latency guarantees.
