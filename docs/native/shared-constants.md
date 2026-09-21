# Shared GPU constants

`src/shaders/hlsl/constants.hlsli` is the sole source of algorithm constants.
Maintained HLSL entries include it directly. The build reads its uint declarations
and emits `OUT_DIR/tileink_gpu_constants.rs` for the host and WGSL constant
preludes for existing wgpu shaders. `src/shared/gpu_constants.rs` only includes
that generated file and asserts algorithm invariants; it contains no values.
CPU-only builds generate the same host definitions without requiring DXC.

Supported declarations use decimal uint literals, earlier constant names and
multiplication, with standard `#ifndef/#define/#endif` guards for shared headers. Unknown names, duplicate definitions, overflow and unsupported
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
literal includes. Include-guard semantics avoid duplicate declarations through
multiple helpers while preserving dependency tracking and rejecting unguarded cycles.

Direct DXC compilation of repository scan prefix/count/emit and coarse prefix
sources succeeds without build-script preprocessing or constant injection.

This refactor does not complete M4: the accepted native kernel inventory remains
18/179 and production NativeRenderer/Canvas integration is still outstanding.

## Earlier constants checkpoint

[Receipt](shared-constants-verification.json): full release suite (964 library
tests), strict all-target release Clippy and both review axes pass. All 44 native
runtime tests pass with the explicit includes and again with nonexistent DXC
executable paths (zero pipeline compiles). Five parser/source tests cover invalid
constants, dependency changes, guarded diamonds/self-includes and rejected
unguarded cycles. The filter GPU row regression passes at 32/64/256 lanes.
Existing wgpu SVG/examples preserve all 3471 PNG hashes; full native renderer
SVG acceptance remains part of the unfinished M4 work.

## Explicit helper dependencies

Reusable HLSLI headers are self-contained and declare no register-bound resource.
Resource bindings live in entry HLSL files. Geometry receives line/path buffers;
clipping receives the segment output buffer; active-index helpers receive their
buffer/mode; packed coarse addressing receives its layout config; probe sampling
receives source/texture/parameters. Dispatch linearization receives grid dimensions.
Headers are included before entry resources, so compile success cannot depend on
caller declaration order. Scan config/index and probe/active-tile helpers have
separate focused headers.

The coarse prefix helper owns its workgroup scratch internally and returns both
prefix and total explicitly. Callers do not read that scratch or a hidden total;
the total is captured before the final barrier. All lanes must participate in a
scan call uniformly, as required by its workgroup barriers.

The compiler regression discovers every HLSLI automatically and compiles each
with no caller declarations for both DXIL and SPIR-V. A second check rejects
register bindings hidden in headers. These prevent the original external-global
and include-order dependency from returning; this is a structural fix.

Standard guards are used instead of `#pragma once` so editor preprocessors and DXC can resolve diamond includes consistently. Reusing a guard name across different files is rejected instead of silently hiding a dependency.

Shader Tools compatibility is checked separately from DXC: its parser treats
`matrix` as a type keyword, producing cascading errors at the following `asfloat`
expression when used as a local name. The traversal now uses `affine_linear`;
texture arguments use `image_texture`. This fixes the source naming conflict,
without changing arithmetic. A declaration regression checks these reserved type
names while still permitting their use as types. Config helper arguments retain
explicit `ConstantBuffer<T>` types instead of relying on conversion to `T`.


The remaining twelve shader ABI JSON files have been removed. Typed host contracts
live in `build/native/abi.rs` and the modular `build/native/interfaces.rs` catalog
(with scan/coarse submodules). They do not generate HLSL declarations or constants.
Both DXIL and SPIR-V reflection validate the same resource kinds, register slots,
uniform field names/types/offsets, and workgroup dimensions. Probe and compute
interfaces share this path rather than separate schema versions. Interface cache
keys use a versioned deterministic binary encoding; shader source and compiler
identity remain part of the key. JSON verification receipts and cache reports are
output records, not interface inputs. Historical receipts retain their original
file hashes, including paths deleted by this migration.

## Explicit interface verification (2026-09-13)

- Full release suite: 1,072 tests passed, including 964 library tests.
- Focused interface/reflection/source/compiler/inventory checks: 20 passed,
  including independent DXIL and SPIR-V compilation of all 14 HLSLI headers.
- Raw repository HLSL files: all 22 entries compiled directly for both targets
  (44 checks), without source expansion or ABI metadata injection.
- Native runtime: 44 tests passed, including four-API exact-output regressions;
  the same 44 passed with missing DXC executable paths and zero native pipeline
  compiles after warming the cache.
- Shader Tools 1.1.303: reproduced the `matrix` parse failure, then confirmed
  those diagnostics disappear after renaming. Declaration-name regression added.
- Strict all-target release Clippy, formatting, and both review axes passed.
- Full wgpu SVG and examples checks passed; all 3,471 PNG hashes are unchanged.

This verifies the explicit-interface refactor; it does not add M4 kernels or
claim production native renderer completion.
### Prefix scan loop scopes

Cumsum and coarse `exclusive_prefix` use distinct `upsweep_step` and
`downsweep_step` names. Shader Tools applies an older for-loop scope rule and
reported HLSL0047 for a second `step` declaration in the same function. This fixes
the source/editor naming conflict; no scan arithmetic or barriers changed.
All 44 directly compiled DXIL/SPIR-V artifacts are byte-identical to the previous
revision. The real-editor regression verifies both files and requires an explicit
empty diagnostic publication after a deliberately invalid document is corrected:

```powershell
$env:TILEINK_HLSL_LANGUAGE_SERVER = '<path>/ShaderTools.LanguageServer.exe'
python tests/hlsl_editor_test.py
```

The compiler suite remains authoritative for executable shaders; this additional
check exercises the separate editor parser which DXC cannot validate.
Validation: the editor regression fails with HLSL0047 in both prior sources and passes after renaming; 1,072 release tests, 44 native runtime tests, strict Clippy and full SVG/examples checks pass. All 3,471 PNG hashes remain unchanged.

Component-transfer table size, channel count and length also originate in
`src/shaders/hlsl/constants.hlsli`. Public host usize constants are aliases of the
generated values; the filter WGSL assembly injects the same size and length.
The upload contract requires 0..255 table values and checked logical indices.

Range-scatter header and descriptor strides are declared in
`shared/range_scatter_constants.hlsli`. Native and wgpu packet writers, the native
validator, HLSL and WGSL consume these definitions. This prevents
fragmented-upload optimization from introducing a third independent wire layout.

Metal retains its existing fixed four-word range-scatter layout in this Windows-only
change. Any future layout change also requires updating and validating Metal; its
shader has not been compiled or benchmarked in this run.
