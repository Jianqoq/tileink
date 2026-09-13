# M4 range scatter — Windows kernel slice

This slice ports the real `src/wgpu/shaders/range_scatter.wgsl` algorithm to
`src/shaders/hlsl/range_scatter.hlsl`. It does not complete M4 or enable the public
NativeRenderer: scan/cumsum, coarse, fine and effects still require migration,
shared GPU resources and full-Canvas four-route validation. The standalone stage
currently uses the existing batch adapter's owned upload/readback frames.

## Contract and structure

- The upload is little-endian u32 words: four header words (payload base, range
  count, two reserved words), followed by four-word descriptors (destination,
  source relative to payload, length, reserved). Payload base equals
  `4 + 4 * range_count`. All indices and lengths are word-based.
- One 256-thread workgroup processes each descriptor, looping in steps of 256.
  Zero ranges emit no compute dispatch and preserve the initial destination.
- `program/scatter.rs` owns and validates the packed upload and destination.
  Buffers must be nonempty, word-aligned and fit u32 byte addressing; ranges must
  fit both buffers and nonempty destinations must be sorted and non-overlapping.
  Overlap rejection prevents cross-workgroup write races at the root cause.
  At most 65,535 descriptors are accepted. Reserved words are not interpreted.
- `program.rs` dispatches typed Probe/Scatter commands. Scatter has no uniform
  block, and cannot borrow probe parameters or the probes' 64-thread launch rule.
  `program/probe.rs` retains the M3 numerical probes separately.
- Native source/destination use binding 1/0 (t1/u0); the original WGSL uses 0/1.
  `range-scatter-abi.json` records the explicit remapping. No source layout or
  shader arithmetic is changed to make tests pass.
- Existing DX12/Vulkan modules record the stage; allocation and synchronization
  remain in their own API folders. Shared batch ownership and receipts preserve
  asynchronous submission and reverse-order readback. No runtime DXC process is
  introduced. Build cache keys cover source, ABI, compiler, flags and target.
- Artifact metadata carries the reflected workgroup dimensions. Vulkan checks
  both axis limits and total invocation limits before pipeline creation; its
  minimum supported workgroup size is not assumed to be 256.
- `build/native.rs` replaces the build module's old mod.rs entry point.

## Verification

Host tests cover malformed/truncated headers, alignment, empty work, bounds,
u32 index overflow, unsorted/overlapping destinations, adjacent ranges and
multi-iteration workgroups. ABI tests check resources, lack of uniforms, native
binding remapping and reflected workgroup size.

The GPU test uses the unmodified production WGSL on both wgpu APIs, and native
HLSL DXIL/SPIR-V on the same pinned physical GPU. It compares every destination
byte with an independent CPU oracle, including untouched guards. Cases cover
range counts around 64/128/256, lengths 0/1/255/256/257/513 and mixed ranges.
All native submissions are queued before reverse-order readback; three
repetitions exercise retained frame ownership. Optional
`TILEINK_NATIVE_SCATTER_REPORT` writes per-case/per-route SHA-256 results.

Enable native validation before creating wgpu devices in this disposable test
process: enabling the DX12 Debug Layer after a non-debug device exists can reset
that device. Host-owned device/debug-layer policy remains part of M5 interop;
this slice does not claim that public integration is implemented.

Windows GPU validation passed on RTX 4090, LUID `9f3f010000000000`: 57 cases × 3 repetitions × 4 APIs = 684 byte-exact outputs. All 25 native runtime tests passed, including the existing M3 probes and failure/lifetime cases. Broader closeout results are recorded below. MSL remains unported for
this stage and real Mac compilation/GPU acceptance remains unavailable. No
performance comparison was run, as requested by the user.

## Closeout results (2026-09-13)

- Complete release test run passed (956 library tests; the ten opt-in GPU tests
  are reported separately, and relevant integration/example tests passed).
- All 25 native runtime tests passed again with both DXC executable paths set to
  nonexistent files: 120 native pipeline cache hits, zero pipeline compilations.
  wgpu's independent DX12 reference still uses its explicitly pinned DXC DLL.
- Native/portable SVG and example runs passed. All 3,471 pre-existing PNG files
  retained identical SHA-256 values; no files were added or removed.
- Default (without DXC environment), CPU, DX12-only, Vulkan-only, native-only and
  wgpu+native release checks passed. The combined feature build passed strict
  all-target Clippy and formatting. Native-only retains existing shared-code
  unused warnings; it is not claimed to pass strict warning-free Clippy.
- Five source-matched SPIR-V artifacts passed the official Vulkan 1.1 validator.
- The 288-file actual crate package contains the new named build root and shader
  inputs. Its extracted native-only and default-without-DXC builds both passed.
- Standards and Spec reviews found no blocking issue in this stage's scope.

The machine-readable receipt is [m4-range-scatter-verification.json](m4-range-scatter-verification.json).
