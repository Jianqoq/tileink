# M4 Windows cumsum and multi-buffer compute batches

Status: cumsum's three HLSL entries are validated on the pinned RTX 4090 LUID
`9f3f010000000000`. Together with range scatter, 4 of 179 inventory entries are
validated. The full native renderer, scan/coarse/fine/effects and the M4 exit gate
remain unfinished. No performance comparison was run, as requested by the user.

## Implementation and invariants

- `runtime/compute.rs` owns logical buffers and an ordered pass list. Handles carry
  batch identity; foreign handles, wrong/missing bindings, writable aliases,
  undersized buffers and invalid grids are rejected before recording. Trusted
  stage encoders must additionally establish data-dependent memory bounds.
- `program/cumsum.rs` validates non-overlapping backdrop intervals, chunk capacity,
  and exact row ownership. Three passes retain totals/carries on the GPU in one
  submission. One-chunk rows omit the carry passes, matching production WGSL.
- `shared/gpu_constants.rs` is the algorithm constant source. Rust planning and
  generated HLSL/WGSL preludes use `CUMSUM_CHUNK_SIZE`; scratch lengths, tree bounds,
  last-element indices and workgroup attributes derive from it. The ABI reflection
  contract is checked against it and it participates in native shader cache keys.
  API alignments are separate: DX12 uses its SDK constant and Vulkan queries the
  physical device's uniform-buffer alignment.
- Native and WGSL configs share the logical `chunk_count` in the second word.
  Prefix/apply stages reject padded groups before metadata reads and barriers.
  Buffer capacity is not a logical bound: retained allocations can hold stale
  valid chunks. Two four-route regressions reproduce and fix this root cause for
  cumsum and sparse scan, preserving all inactive records.
- DX12 and Vulkan own separate allocation, descriptors, pipelines and command
  implementations. Both upload once per batch and keep intermediates device-local.
  Vulkan binds logical buffers into one device-memory arena plus upload/readback
  blocks. Allocation reuse across batches is still pending full M4 integration.
- Per-entry pipelines are lazy and persist driver cache blobs. The compute layout
  has a distinct cache identity from the old probe layout. Vulkan teardown clears
  compute pipelines before destroying the device; unconfirmed work is quarantined.
- A read-only buffer may be both CBV and SRV. DX12 now unions all per-pass read
  states before transitioning the resource. The previous last-binding-wins path
  lost one required state. A direct semantic regression reproduced `64 != 65`
  before this root-cause fix; the GPU debug layer alone did not catch that error.

## Evidence

[Machine-readable receipt](m4-cumsum-verification.json) records 12 input cases,
three repetitions, four actual API routes and each output buffer's SHA-256.
All 144 outputs match an independent wrapping-i32 CPU prefix oracle exactly,
including chunk totals, carries, untouched gaps and guards. Cases cover empty
inputs/chunks, one/multiple rows, 255/256/257 boundaries, multiple chunks, signed
overflow, padded two-dimensional grids and reverse observation of queued tickets.

The production WGSL algorithm is independently executed through wgpu-DX12 and
wgpu-Vulkan. The original receipt below predates the subsequent logical chunk-count
guard fix; algorithm constants are supplied by the common build prelude. The original PNG baseline is independently retained: full SVG and
example native/portable runs produced 3471 PNG files with identical file hashes,
no additions and no missing files.

All 33 native runtime tests ran, including opt-in GPU/lifetime/error paths. Running
the actual compiled test executable with both DXC executable paths set to missing
files also passed all 33 tests with zero pipeline compilations. The wgpu reference
still uses its explicitly configured DXC DLL; this does not claim that wgpu itself
is compiler-free. Native DXIL/SPIR-V are embedded build artifacts.

Reproduce cumsum with the environment documented in the range-scatter record:

```powershell
cargo test --release --features native --lib four_api_cumsum -- --ignored --nocapture --test-threads=1
cargo test --release --features native --lib native::runtime -- --include-ignored --test-threads=1
cargo test --release --features native --test native_shader_abi --test native_shader_artifacts --test native_shader_reflection -- --test-threads=1
```

Set `TILEINK_NATIVE_CUMSUM_REPORT` to save the per-route buffer receipt. Mac/MSL
validation remains deferred because no Mac is available. This slice does not make
`NativeRenderer::new` available or claim full Canvas output through native APIs.

Final checkpoint checks: `cargo test --release --features native` passed 962 library
cases (12 opt-in cases were also exercised by the separate native runtime run)
and the integration suites. CPU-only, native-DX12, native-Vulkan, native aggregate
and combined wgpu/native builds passed. `cargo fmt --all --check` and strict
all-target Clippy passed. All three actual cumsum SPIR-V artifacts passed
`spirv-val --target-env vulkan1.1`. The 304-file Cargo archive contained the new
modules/constants/shaders and built native-only and default-without-DXC after
extraction. Package verification refreshed extracted input mtimes to avoid
reusing an older build-script output from the shared Cargo target directory.