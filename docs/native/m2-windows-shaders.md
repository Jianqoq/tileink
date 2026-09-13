# Windows native shader toolchain and probes

> M2 toolchain receipt; current Windows runtime is M3. See [current M3 closeout](m3-completion.md) for current module paths, commands and completed scope.

M0/M1 delivery `90151cbdc7047e8b1eb14e41f62b4690b416ede9` was confirmed on the
configured Git remote after the user's push on 2026-09-13. This document records
the next Windows build/ABI/probe slice, not completion of the native renderer.
The user has no Mac available and requested Windows work first. Independent MSL
source is included; Metal compilation, reflection and GPU acceptance remain open.
Performance comparisons remain stopped by the user's instruction.

## Build and cache contract

Default `wgpu` builds do not invoke the new native compiler path. Windows
`native-dx12` builds DXIL; `native-vulkan` builds SPIR-V; `native` builds both.
An explicit absolute `TILEINK_NATIVE_DXC_PATH` is required, with `TILEINK_DXC_PATH`
as a fallback. The tested native tool is DXC 1.8.2502.8 (b4711839e). The Windows
SDK DXC used by the existing wgpu reference does not support SPIR-V; configure
these two compiler paths separately. Missing/unsupported tools fail explicitly.

```powershell
$env:TILEINK_NATIVE_DXC_PATH = 'G:\Code\tileink\target\toolchains\dxc-v1.8.2502\bin\x64\dxc.exe'
cargo build --release --no-default-features --features native
```

`build/native/` separates source expansion, storage, DXC execution, ABI checking,
DXIL reflection and SPIR-V reflection. This fixes repeated native shader
compilation at its source: successful binary artifacts are content addressed,
then embedded in the library. Starting the built executable does not run DXC.
A Cargo rebuild can still run compiler identity discovery and DXIL reflection;
these are not shader compilation. GPU drivers still create pipelines separately.

The shader cache defaults to `$CARGO_TARGET_DIR/tileink-native-shaders`, or
`target/tileink-native-shaders` when the target directory is not configured.
`TILEINK_NATIVE_SHADER_CACHE_DIR` overrides it. Relative overrides resolve from
the package root. Its SHA-256 key includes source language, all source/include
bytes, expanded input, entry/variant, target/triple, ABI bytes, exact compiler
flags, compiler version and compiler executable/library digests. Changed inputs
produce new keys. Old entries are disposable and are not silently reused.

The supported source format uses literal local `#include "file"` directives.
Includes are recursively expanded inside the canonical source root. Cycles,
escapes, missing includes, macro directives and alternate preprocessor syntax
are rejected rather than allowing DXC to read untracked files. This first format
does not support conditional compilation or include guards. Metal's standard
library is reserved for the later SDK-identified Metal compiler module.

Cache records contain a format version, key, payload length and SHA-256 digest.
An OS file lock per key covers load, validation, compilation and publication,
including across processes. A crashed/truncated write or bad checksum rebuilds;
compiler failures and empty outputs are never published. I/O errors are reported.
This is corruption detection, not authentication of an untrusted cache directory.

DXIL uses `cs_6_0`, HLSL 2021 and strict IEEE settings; SPIR-V additionally targets
Vulkan 1.1. Unnecessary Google reflection extensions are not emitted. Each build
checks the generated/cached artifact's entry, workgroup, bindings and byte-buffer
layout. DXIL resource arrays cannot disappear from resource-count validation.
The narrow SPIR-V ABI reader is supplemented by real validation-layer execution;
it is not a general SPIR-V validator. The four current artifacts also passed
SPIRV-Tools `spirv-val --target-env vulkan1.1`.

## Probe ABI and exact output

`src/shaders/probe-abi.json` defines the minimum ABI: a 32-byte, 16-byte-aligned
parameter structure, four scalar fields at offsets 0/4/8/12 and a uint4 at 16.
Byte-buffer offsets are multiples of four. Set/register space is zero;
destination/source/params use bindings 0/1/2. Dispatch size is 64×1×1.

Four authored HLSL entries produce both DXIL and SPIR-V. Independent WGSL provides
the wgpu reference; independent `.metal` source follows the same contract:

- `clear_words`: packed RGBA/integer clear and destination guards.
- `copy_words`: offset source reads and exact word transfer.
- `layout_words`: uint4 sentinel arithmetic, wrapping overflow and strided output.
- `sample_words`: manual linear interpolation of RGBA8 texels held in a byte
  buffer, floor coordinates, clamp-to-edge and round-half-up quantization.

The sampling contract requires nonzero width and finite coordinates whose floor
and neighbor are representable. Current cases use exactly representable dyadic
coordinates, one/17 texels, positive/negative steps and independent f64 reference
arithmetic. It does not establish arbitrary floating-point equality, texture
format behavior, hardware filtering, NaN/denormal behavior or full renderer parity.
Those risks remain explicit M3/M4 acceptance work.

There are 42 cases: 18 clear/copy/layout and 24 sampling cases, each covering
counts 0, 1, 63, 64, 65 and 129. Entire destination bytes are compared, including
prefix/tail/stride guards. On the recorded RTX 4090 LUID `9f3f010000000000`, real
wgpu-DX12, wgpu-Vulkan, native DX12 and native Vulkan outputs matched one another
and the independent CPU expected bytes with zero differences. This is a minimum
probe contract, not a full SVG/example/RetainedScene acceptance run.

## GPU pipeline cache and verification lifetime

The permanent GPU harness uses actual DX12 root signatures/PSOs/resources/fences
and Vulkan descriptor layouts/pipelines/buffers/barriers. Both native paths load
the embedded shader products. DX12 debug layer and Vulkan Khronos validation are
required. Diagnostics are checked after context destruction as well as execution.

DX12 PSO blobs and Vulkan pipeline cache data persist under
`target/native-probe-pipelines`, overridden by `TILEINK_NATIVE_PIPELINE_CACHE_DIR`.
Keys additionally contain API/device/driver identity, pipeline layout version and
shader artifact key. Vulkan cache headers must match the adapter and cache UUID.
DX12 retries known cache incompatibility errors; unrelated device-loss/allocation
errors propagate. Only cache-specific messages from that rejected creation attempt
are classified as expected, preserving all other validation diagnostics. Corrupt
storage is rejected before calling the driver. A fresh process hit all eight
native pipeline entries after the initial compilation run.

This driver cache is currently used by the verification adapters. It is not yet
wired into a production `NativeRenderer`, whose constructors still return explicit
unavailable errors. Production adapters must retain this cache contract when added.
Readback waits in these disposable tests are not a production frame submit policy.
If a DX12 test submission's completion becomes unknown, every GPU owner including
the fence is retained until process exit; it never frees possibly in-flight work
or waits on a signal that failed. This is test failure containment, not a device
loss recovery implementation for a shipping renderer. Failure tests run in
isolated child processes, checking Idle/Unfenced/Failed COM ownership counts and
InfoQueue diagnostic boundaries without polluting normal four-API validation.

## Run correctness checks

Run all tests single-threaded in release mode. The GPU test is deliberately
ignored by a generic test run; invoke it explicitly and pin one physical device.

```powershell
$env:TILEINK_NATIVE_GPU = '9f3f010000000000' # use the current machine's actual LUID
$env:TILEINK_PARITY_DXCOMPILER = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\dxcompiler.dll'
$env:VK_LAYER_PATH = 'G:\Code\tileink\target\toolchains\vulkan-validation-1.4.357.0\Bin'
cargo test --release --features native --test native_shader_gpu -- --ignored --test-threads=1 --nocapture
cargo test --release --features native -- --test-threads=1
cargo test --release --test native_shader_compiler -- --ignored --test-threads=1
cargo clippy --release --all-targets --features native -- -D warnings
```

The migration inventory `native-program-inventory.json` covers all 179 reference
entry/texture-table variants, bound to the reference inventory digest after UTF-8/LF normalization (so Git
checkout line endings do not invalidate the cross-platform inventory). It inherits
complete resource/type/stride/workgroup and filter remap metadata by reference.
All full renderer HLSL/MSL entries remain marked unported. Probe completion never
changes these statuses. M3 production adapter integration, hardware texture and
numerical probes, M4 complete pipeline porting, M5 retained/external-target work,
and M6 full correctness acceptance remain open. macOS needs a real Mac and its
own compiler/SDK identity, Metal artifact/reflection and wgpu-Metal GPU comparison.

Compiler/background documentation: [DXC SPIR-V](https://github.com/microsoft/DirectXShaderCompiler/blob/main/docs/SPIR-V.rst),
[DX12 cached PSO contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/ns-d3d12-d3d12_cached_pipeline_state),
[Apple Metal libraries](https://developer.apple.com/documentation/metal/metal-libraries).

## Verified Windows build slice

Release library suites passed: default wgpu 938, native aggregate 941 and CPU-only
620 tests. Full default/native test commands also passed their integration suites;
ignored GPU cases were executed separately. All three native-only feature builds
embedded their expected artifacts. Missing-tool/invalid-DXC tests, source graph,
cache concurrency/corruption, reflection mutation, ABI and migration inventory
checks passed. Final formatting and native all-target strict Clippy passed.

The actual `.crate` archive was extracted outside any workspace and built with
native-only features and with default wgpu without native compiler variables.
The native normal/build dependency tree contains no wgpu, wgpu-hal or naga.
The independently maintained Metal source is present in the package but remains
uncompiled and unverified on Apple hardware. No production wgpu shader/rendering
algorithm was changed, and no new performance comparison was run.

Machine-readable local verification receipts and source digests are recorded in
[m2-windows-verification.json](m2-windows-verification.json). Local absolute log
paths identify this machine's records; they are not portable CI artifact URLs.
