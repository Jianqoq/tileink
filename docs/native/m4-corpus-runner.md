# Windows immediate corpus runner

Use the [M6 separate-build runner](m6-windows.md) for current acceptance. The
former combined-process `--features native` / `--native` commands are obsolete:
renderer features are mutually exclusive. Historical M4 receipts retain their
original commands and source hashes.

```powershell
.\scripts\ps1\run_native_acceptance.ps1 -Output target/native-svg-new -Gpu '<16-digit-LUID>' -Suite svg
.\scripts\ps1\run_native_acceptance.ps1 -Output target/native-examples-new -Gpu '<16-digit-LUID>' -Suite examples
```

Configure the pinned wgpu DXC DLL, native DXC and Vulkan validation layer as
documented in M6. The output directory must be new. All seven current routes are
required, including portable textures and the wgpu-DX12 embedded-DXIL variant.

Every route loads the same hashed SVG/resource inputs. The complete example catalog
uses one backend-neutral scene callback and captured font/SVG inputs; scene-building
code and text layout are not reimplemented for native verification. Callback errors,
duplicate/missing outputs and nested captures fail. The next capture starts clean
after a failed callback. A native capture cannot silently create a wgpu renderer.

The report compares all four premultiplied RGBA8 channels without tolerance,
records every expected frame, saves raw outputs and differences, and checks API
validation before successful completion. The manifest hashes the binary, sources
(including dirty/new files), resources and compiler provenance. These are test evidence
files, not shader ABI inputs. Successful smoke tests alone do not close M4: both full
immediate corpora must complete with zero different pixels.
