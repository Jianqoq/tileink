# Shared GPU constants

`src/shaders/hlsl/constants.hlsli` is the sole source of algorithm constants.
Maintained HLSL entries include it directly. The build reads its uint declarations
and emits `OUT_DIR/tileink_gpu_constants.rs` for the host and WGSL constant
preludes for existing wgpu shaders. `src/shared/gpu_constants.rs` only includes
that generated file and asserts algorithm invariants; it contains no values.
CPU-only builds generate the same host definitions without requiring DXC.

Supported declarations use decimal uint literals, earlier constant names and
multiplication, with `#pragma once` for shared headers. Unknown names, duplicate definitions, overflow and unsupported
syntax fail the build, preventing host/shader interpretation from diverging.
The file is a Cargo rebuild input and a native HLSL source/cache dependency.
Native ABI workgroup numbers remain independently checked reflection contracts.

The constants cover tile size, range scatter, cumsum, scan, coarse, fine and
linear filters. Fine lanes derive from tile area; blur dimensions follow damage
tiles, and blur shared-memory capacity derives from dimensions and halo radius.
Fine DXIL metadata and host dispatch/spill sizing use generated host constants.
API alignment and unrelated numeric values keep their own semantic definitions.

The filter row-index helper also uses the lane constant: a literal row stride
would skip pixels when workgroup width changes. A GPU regression executes the
production helper at 32, 64 and the configured width across three dispatch rows.
This fixes the source of that drift, not a default-width-specific workaround.

Raw scene and coarse layout facts are defined in `scene_records.hlsli` and
`coarse_records.hlsli`. Each user explicitly includes its dependencies, including
scan prefix/geometry/clip and coarse allocation helpers. No ABI JSON-to-HLSL
constant prelude is injected. Rust size/offset tests parse these same HLSLI
values and verify the binary layout; native source/cache tracking follows the
literal includes. Include-once semantics avoid duplicate declarations through
multiple helpers while preserving dependency tracking and rejecting unguarded cycles.

Direct DXC compilation of repository scan prefix/count/emit and coarse prefix
sources succeeds without build-script preprocessing or constant injection.

This refactor does not complete M4: the accepted native kernel inventory remains
18/179 and production NativeRenderer/Canvas integration is still outstanding.

## Verification

[Receipt](shared-constants-verification.json): full release suite (964 library
tests), strict all-target release Clippy and both review axes pass. All 44 native
runtime tests pass with the explicit includes and again with nonexistent DXC
executable paths (zero pipeline compiles). Five parser/source tests cover invalid
constants, dependency changes, guarded diamonds/self-includes and rejected
unguarded cycles. The filter GPU row regression passes at 32/64/256 lanes.
Existing wgpu SVG/examples preserve all 3471 PNG hashes; full native renderer
SVG acceptance remains part of the unfinished M4 work.
