# Backend feature contract

As of 2026-09-19, exactly one of `wgpu`, `dx12`, `vulkan`, and `metal` must be enabled.
`wgpu` is the default. The old `native`, `native-dx12`, and `native-vulkan`
features have been removed, without aliases. Both the build script and library
enforce exclusivity, so invalid combinations fail before shader compilation.

```powershell
cargo check --release --all-targets
cargo check --release --no-default-features --features dx12 --all-targets
cargo check --release --no-default-features --features vulkan --all-targets
# macOS / Xcode
cargo check --release --no-default-features --features metal --all-targets
cargo test --release --test backend_features -- --test-threads=1
```

Consumers disable dependency defaults and forward one selected feature. Cargo
unifies features, so enabling wgpu through one dependency and dx12 through
another is an error. Native builds do not bring in the wgpu renderer.

The HLSL compiler configuration (`TILEINK_NATIVE_DXC_PATH`) and the native Rust
module names are unchanged: they describe the implementation, not Cargo features.
DX12 embeds cached DXIL; Vulkan embeds cached SPIR-V. Feature renaming does not
change shader source or invalidate shader content hashes.

Pixel parity must use separately compiled executables for wgpu-DX12,
wgpu-Vulkan, DX12, and Vulkan, with matching GPU, inputs, fonts, and frame sequence.
There is no test-only exception to feature exclusivity. The maintained
[Windows corpus runner](m6-windows.md) builds each backend separately and compares
all SVGs, examples and retained variants across processes. Previous multi-feature
invocations are not valid verification commands.
The independent lifetime suite is already migrated to
`native::runtime::lifecycle_gpu_tests`: run it in each native-only build with
`--ignored --test-threads=1` and the pinned GPU/validation environment. It covers
receipts, staged batches, persistent resources, imported targets and backend-specific
queue synchronization. It complements the cross-build corpus acceptance.
Historical M0–M5 reports retain the feature names and commands actually used at
their recorded commits. Those reports are evidence of those commits, not new
verification of this migration.

The macOS `metal` feature embeds independently maintained MSL compiled with the
Apple toolchain. See [Metal setup, ownership and exact acceptance](metal.md).
The Metal corpus and lifetime scripts use legal separate backend builds.
