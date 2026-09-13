# M4 Windows scan kernels

Six maintained HLSL entries now execute the full clear → count → prefix chunks →
chunk offsets → apply offsets → emit chain on native DX12 and Vulkan. Together
with range scatter and cumsum, 10/179 inventory entries are kernel-validated.
This is not full M4 completion: coarse/fine/effects, production stage encoders,
resource reuse, NativeRenderer/Canvas and four-route immediate SVG/examples remain.
Mac MSL hardware validation is deferred. Performance comparisons remain waived.

## Implementation and invariants

- `shared/gpu_constants.rs` owns scan/cumsum chunk sizes, range-scatter workgroup
  size and physical tile size. Rust planning and generated WGSL/HLSL preludes
  consume them. Scratch sizes and loop bounds derive from the algorithm constants.
  Native ABI workgroup declarations are independently checked during compilation.
  Equal-valued API alignment requirements remain API constants/device queries.
- `build/native/program.rs` owns the program catalog and source/ABI preparation.
  Each scan family references the common record ABI; all 13 raw offsets/strides
  are checked against Rust `size_of!`/`offset_of!`. Source includes, resolved ABI,
  constants and compiler identity participate in shader cache identity.
- `geometry.hlsli` shares DDA traversal between count and emit. `clip.hlsli` owns
  endpoint clipping and tile-local segment encoding. Float arithmetic is checked
  bit for bit against production WGSL on the pinned RTX 4090, not generalized to
  untested devices. Count uses atomics; emit reserves bounded segment slots.
- All six passes use one compute batch and keep intermediates on the GPU. Existing
  DX12/Vulkan allocation, binding, barriers and owning receipts stay separate.
- The logical count, not retained buffer capacity, bounds padded dispatch groups.
  Regression tests first reproduced WGSL scan/cumsum processing stale in-capacity
  metadata, then passed after adding guards before lookup/barriers. Cumsum host,
  WGSL and HLSL now share the second config word as `chunk_count`. This fixes the
  root cause rather than relying on robust out-of-bounds behavior.

## Verification

[Machine-readable receipt](m4-scan-verification.json) records source/artifact/log
hashes and test scope. Four actual routes use GPU LUID `9f3f010000000000`.

| Coverage | Exact output executions |
| --- | ---: |
| Dense/sparse clear, empty and partial workgroups, untouched guards | 144 |
| Prefix/carry/apply, CPU wrapping-u32 oracle, empty chunks and padded grids | 72 |
| Count, clipped analytic/random lines, both orientations, CPU tile count | 3072 |
| Full six-stage chain, float segment records and surrounding guards | 768 |
| 17/255/256/257 affine paths, reverse sparse mappings and padded grids | 40 |

The two stale-tail regressions additionally execute all four routes. The affine
test uses one wgpu-DX12 baseline per case and three executions of the other routes;
the other table rows repeat all routes three times. Full-chain fixtures give each
tile one segment writer, so byte-order determinism is meaningful; overlapping
multi-writer segment ordering still requires downstream renderer acceptance.

All 41 native runtime tests, including opt-in GPU tests, pass with validation
layers. Re-running the compiled executable with nonexistent DXC executable paths
also passes all 41, with zero pipeline compiles and cache hits. The wgpu reference
still uses its configured DXC DLL. All six generated SPIR-V modules pass
`spirv-val --target-env vulkan1.1`.

Full release tests pass (963 library tests; opt-in GPU cases exercised separately),
as do 620 CPU-only library tests and independent native feature checks. Combined
all-target strict release Clippy and formatting pass. Cargo's package list includes
the new build module, shared constants, HLSL and ABI files.

Full existing wgpu SVG/example native-texture versus portable-texture runs pass;
all 3471 baseline PNG file hashes are unchanged, with no additions or missing files.
These are existing-renderer regressions, not native DX12/Vulkan SVG acceptance.
