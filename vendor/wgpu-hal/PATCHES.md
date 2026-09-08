# Local wgpu-hal patch

This directory contains the crates.io `wgpu-hal` 30.0.0 source, from upstream commit
`8bf3e5ff4ab45e2c150e0d6c70d01d25f5b126c1` (`wgpu-hal/`). The MIT and Apache-2.0 licenses
are retained. Cargo's registry cache marker and the package's standalone lockfile are omitted;
the root consumer workspace's lockfile owns dependency resolution. Upstream example targets and
their dev-only dependencies are omitted from `Cargo.toml`; `Cargo.toml.orig` preserves the
original manifest. The source library, build script and licenses are retained.

The Tileink workspace patches only the compatible 30.x dependency. The separate 29.x
dependency remains upstream. This package is a Tileink workspace member, so its regression
tests run there directly. gfx_ui gallery and component examples select the same copy from
their root patch. External
application workspaces select this same directory from their own root patch; they do not
keep another copy. Omitting the unused example graph also keeps workspace-wide offline Cargo metadata
from requiring unrelated window-system packages during installed-strategy validation. The real
offline Supervisor regression caught this packaging issue before the example graph was removed.

## Fresh surface-format enumeration

The enumeration delta is limited to `src/vulkan/swapchain/{mod,native,surface_formats}.rs`.
Two redundant logging-argument borrows in `src/gles/{adapter,device}.rs` are also removed to
pass this patch's strict Clippy validation; these formatting changes preserve behavior.

- `NativeSurface` remembers an atomic format-count hint, not capability values.
- The first call obtains a count and enumerates formats. Later calls first enumerate into a
  buffer of the previous size, avoiding the otherwise redundant count-only driver call.
- `VK_INCOMPLETE` discards the partial result and retries the count/data sequence, including
  when the list grows again between those calls. Shrinking lists return only written entries.
- Every successful result contains fresh format/color-space pairs. Errors propagate exactly
  as before. A shared hint can be stale across concurrent queries or adapters without changing
  the returned data: it only changes buffer sizing and the need for a retry.
- Support, extent, present-mode queries and swapchain configuration remain unchanged.

This fixes redundant enumeration on the resize path. It does not eliminate native driver
heap contention inside a remaining Vulkan call. See
[the backend maintenance guide](../../WGPU_PATCHES.md) for integration and validation.
The Vulkan count/data and incomplete-result contract is documented by
[Khronos](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetPhysicalDeviceSurfaceFormatsKHR.html).

Run `cargo test --release -p wgpu-hal@30.0.0 --lib --features vulkan` from the Tileink workspace root.
The repeated-query regression failed against the original count/data algorithm before the fix.
Tests also cover changed formats, growing/shrinking lists, repeated incomplete results and errors.

## Deferred swapchain image allocation

`src/vulkan/{instance,adapter,mod}.rs` enable `VK_EXT_swapchain_maintenance1` only when
its complete instance dependencies and physical-device feature are available. Headless instances,
missing dependencies, and devices with an absent/false feature retain ordinary allocation.
The chosen feature is included in `VkDeviceCreateInfo`; the logical device records its enabled
state. Merely advertising an extension does not authorize using its swapchain flags.

`src/vulkan/swapchain/native.rs` then requests `DEFERRED_MEMORY_ALLOCATION_EXT`. Resize can retire
a swapchain before all of its images are used. Allocating image memory on first acquisition avoids
work for these unused images. This addresses eager allocation during resize; acquisition may now
include some allocation cost, so performance comparisons must include acquisition and total frame
time. It is not a guarantee against unrelated driver heap contention.

Images are used only after successful acquisition. Every swapchain still has the current client
extent, every capability query returns fresh data, and existing idle waits, old-swapchain handoff,
semaphore/fence lifetime rules, color spaces and presentation mode remain in force.
The feature contract is documented by
[Khronos](https://docs.vulkan.org/refpages/latest/refpages/source/VkSwapchainCreateFlagBitsKHR.html).

Release regressions inspect the actual device-create feature chain and cover incomplete instance
dependencies, a headless instance, missing/disabled device features, and a supported feature whose
extension was not enabled. They failed before enabling the feature/dependency chain.

On a wgpu upgrade, compare these source deltas against upstream enumeration and swapchain allocation,
rerun these tests and the native Replay resize benchmark, and remove the patch if upstream
provides equivalent behavior. Do not silently replace fresh enumeration with cached formats.

## DX12 write-only texture ordering

`src/dx12/command.rs` now emits a UAV barrier whenever a texture dependency supplied
by wgpu-core keeps the resource in `D3D12_RESOURCE_STATE_UNORDERED_ACCESS`. The old
condition only recognized `TextureUses::STORAGE_READ_WRITE`. Write-only and read-only
storage usages map to that same D3D12 state, so the old check could discard required
write-after-write, read-after-write or write-after-read dependencies without a state
transition. State-changing dependencies still use ordinary transition barriers.

This fixes the synchronization root cause. It adds no CPU wait, extra queue submission,
shader variant or Tileink-specific behavior. Vulkan and the buffer barrier path are unchanged.
The required ordering is defined by Microsoft's
[UAV barrier contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/ns-d3d12-d3d12_resource_uav_barrier).
The published upstream 30.0.1 source still has the old condition; upgrading to that version
alone is insufficient. Recheck this delta against upstream on every HAL upgrade.

The GPU regression lives in `tests/dx12_texture_order.rs` at the Tileink workspace root,
with shared test/benchmark support in `tests/support/dx12_texture_order.rs`. It drives
wgpu through the patched HAL on an explicit DX12 hardware adapter, submits consecutive
write-only compute passes, and checks every final RGBA byte. Normal submissions retain
UAV state before readback: reading after every submission introduces COPY_SOURCE
transitions that can mask the missing dependency. The unpatched condition failed with
2480 nonzero pixels in the first checked frame on the recorded RTX 4090.

Run the opt-in regression explicitly; an ordinary test run reports it as ignored:

```powershell
$env:TILEINK_PARITY_DXCOMPILER = '<absolute path to dxcompiler.dll>'
cargo test --release --locked -p tileink --test dx12_texture_order -- --ignored --test-threads=1
cargo bench --locked -p tileink --bench dx12_texture_order
```

The Criterion workload measures encoding, submission and GPU completion of one or eight
write pairs, excluding shader compilation and readback. Its final pixel assertion also
checks correctness after sustained submissions. The upstream performance baseline is an
incorrect renderer, so its timings do not justify dropping required barriers.
Keep the GPU regression and workload with this patch during upgrades. Tileink's
`wgpu_backend_parity` filter-sequence test additionally covers the original SVG failure
across both APIs and texture modes.

The ownership move preserved all 83 existing files. Apart from this record and the DX12
barrier fix, rustfmt only changed whitespace in `src/vulkan/descriptor.rs` and
`src/vulkan/swapchain/native.rs`; the remaining 79 files are byte-identical to the gfx_ui
source revision recorded in the maintenance guide.
