# Four-API immediate corpus runner

Build `wgpu_backend_parity` in release mode with `--features native`. The optional
`--native` flag adds owned native DX12 and Vulkan routes to the existing wgpu
comparison. The standalone process activates the DX12 debug layer before any device
is created, enables native API validation, and pins every route to one physical LUID.
It rejects native retained certification, which remains M5.

```powershell
cargo build --release --features native --example wgpu_backend_parity
$parity = "target/release/examples/wgpu_backend_parity.exe"
& $parity --native --textures native --luid <16-digit-LUID> --dxc <dxcompiler.dll> --input src/svg/tests --output target/native-svg-new
& $parity --native --textures native --luid <16-digit-LUID> --dxc <dxcompiler.dll> --suite examples --output target/native-examples-new
```

Use the actual `CARGO_TARGET_DIR` when set. Configure `TILEINK_NATIVE_DXC_PATH`
for the native build and `VK_LAYER_PATH` for the installed Vulkan validation layer.
The output directory must be new. `--textures native` selects wgpu's native texture
table mode; `--native` independently adds the native API implementations. `--textures
both` also includes the two wgpu portable texture routes.

Every SVG uses the same prepared tree on all routes. The complete example catalog
uses one backend-neutral scene callback and captured font/SVG inputs; scene-building
code and text layout are not reimplemented for native verification. Callback errors,
duplicate/missing outputs and nested captures fail. The next capture starts clean
after a failed callback. A native capture cannot silently create a wgpu renderer.

The existing report compares all four premultiplied RGBA8 channels without tolerance,
records every expected frame, saves raw outputs and differences, and checks API
validation before successful completion. The manifest hashes the binary, sources
(including dirty/new files), resources and compiler provenance. These are test evidence
files, not shader ABI inputs. Successful smoke tests alone do not close M4: both full
immediate corpora must complete with zero different pixels.
