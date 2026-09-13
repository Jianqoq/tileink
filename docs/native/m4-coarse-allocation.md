# M4 Windows coarse allocation kernels

Eight maintained HLSL entries implement paired particle/glyph prefix allocation
and the five-stage emit-reference allocation chain. Both native APIs use the same
HLSL through DXC. Together with prior slices, 18/179 entries are kernel-validated;
coarse draw classification/particle emission, fine/effects, production stage
encoders, resource reuse and full NativeRenderer/Canvas remain unfinished.

## Implementation

- `coarse/prefix_scan.hlsli` shares the ordered wrapping-u32 two-channel scan;
  `prefix.hlsl` applies it to particle/glyph ranges, while `emit_allocation.hlsl`
  allocates per-tile emit references. No intermediate CPU readback is introduced.
- `emit_layout.hlsli` describes byte offsets through the packed coarse buffer.
  All 14 raw layout constants are checked against Rust `size_of!`/`offset_of!`.
  Only specified fields are written; unused particle/glyph/classification fields
  and regions before/after the outputs retain their contents.
- Emit reference writes explicitly respect `emit_chunk_capacity`. Logical-tail
  guards precede native raw reads; partial workgroups still participate uniformly
  in the shared-memory prefix scan.
- `src/shaders/hlsl/constants.hlsli::COARSE_WORKGROUP_SIZE` feeds HLSL directly
  and [generated Rust/WGSL](shared-constants.md) for host dispatch/planning. Tree bounds, scratch arrays, page size and page metadata
  length derive from that value. The host retains the bin-area invariant. Color
  alpha 255 and API alignment requirements are separate semantic constants.
- The native build validates each entry's workgroup/resources through DXIL/SPIR-V
  reflection. Record layouts are explicit HLSLI includes. The serial
  emit carry entry has one thread; the parallel allocation entries use the shared
  workgroup size. No unused scan tile constant is injected into coarse programs.

## Verification

[Receipt](m4-coarse-allocation-verification.json) includes source, artifact and log
hashes. Tests pin all four real routes to RTX 4090 LUID `9f3f010000000000`.

- Paired prefix/carry/apply: 144 exact CPU-oracle output executions, covering
  0/1/255/256/257/65537 items, sparse reversed tile indices, wrapping-u32 counts,
  untouched guards and carry across a second scan block.
- Emit count/prefix/carry/apply/fill: 144 exact CPU-oracle output executions,
  covering empty/partial/multiple workgroups, nonzero preceding packed regions,
  zero/truncated/sufficient reference capacities and untouched record fields.
- All 44 native runtime tests pass. The compiled executable also passes all 44
  with nonexistent DXC executable paths, zero pipeline compiles and cache hits;
  the wgpu reference retains its configured DXC DLL.
- All eight generated SPIR-V entries pass `spirv-val --target-env vulkan1.1`.
  Full release tests pass (964 library tests), as do 620 CPU-only library tests,
  independent native feature checks, strict all-target release Clippy, formatting
  and code review. Cargo's package list contains the added HLSL/include/ABI files.
- Full existing wgpu SVG and example native/portable texture comparisons pass.
  All 3471 PNG file hashes remain unchanged, with no added or missing files.
  This does not substitute for full native DX12/Vulkan SVG acceptance at M4 exit.

No performance comparison was run; the user waived that gate. Mac hardware
validation remains deferred because no Mac is available.
