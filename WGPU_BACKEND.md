# Upstream wgpu backend

Tileink, gfx_ui and the trading application use the crates.io wgpu HAL. No vendored
HAL, path patch, fork or HAL workspace member is required. The lockfiles select
upstream `wgpu-hal` 30.0.1 for wgpu 30; other major versions remain separate.
Verify the selected registry source with `cargo tree --locked -i wgpu-hal@30.0.1`.

## DX12 texture ordering

Upstream 30.0.1 omits same-UAV-state dependencies when the previous tracked texture
use is write-only. Removing the old HAL fix reproduced 2,048 incorrect pixels in
`dx12_texture_order`. Tileink now uses the public `CommandEncoder::transition_resources`
API before write-only fine/filter passes on DX12. It establishes STORAGE_READ_WRITE
as the tracked UAV state so the following pass receives an upstream UAV barrier.
This requires no optional read/write shader feature, CPU wait, extra submission,
shader modification or HAL code. Vulkan and read/write pipelines keep ordinary tracking.

This is an isolated consumer workaround in `src/wgpu/texture_order.rs`, not an
upstream root-cause fix. Remove it only after the regression passes against a HAL
that correctly orders write-only storage accesses without it. The regression and
Criterion workload share this exact synchronization helper.

```powershell
cargo test --release --locked -p tileink --test dx12_texture_order -- --ignored --test-threads=1
```

Set `TILEINK_PARITY_DXCOMPILER` to the explicit dxcompiler.dll path. Full renderer
validation must include DX12 portable filters and cross-API RGBA comparisons.
Upstream's ordinary sampler comparison-field warning is no longer locally patched;
no validation messages are suppressed here.

## Resize ownership

The previous Vulkan HAL resize patches have been removed. gfx_ui owns native
Vulkan deferred swapchain allocation, oldSwapchain handoff, frame-fence retirement
and target capacity reuse. wgpu uses the upstream swapchain behavior. Framework-level
wgpu configuration/layout overlap and offscreen capacity reuse remain independent
of the HAL and are retained.
