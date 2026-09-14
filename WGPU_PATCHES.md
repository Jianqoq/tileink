# Shared wgpu HAL maintenance

Tileink owns the single modified `wgpu-hal` 30.0.0 source in `vendor/wgpu-hal`.
It was moved from gfx_ui at `cfcdce30596cc77146de5c671554e43350a1d24f`, preserving
upstream provenance and both licenses. gfx_ui consumes this copy. Do not create a
second vendor directory in an application or framework.

[PATCHES.md](vendor/wgpu-hal/PATCHES.md) records the upstream commit, source deltas,
root causes and regressions. The three backend changes are fresh Vulkan surface-format
enumeration, supported deferred Vulkan swapchain allocation, and DX12 texture UAV ordering.
Relocation itself does not change rendering or performance. HAL 29.x dependencies elsewhere
in the graph remain upstream; this patch only replaces the compatible 30.x package.

## Consumer integration

Cargo only honors patches in the root workspace. A dependency on Tileink or gfx_ui does
not inherit Tileink's patch. For sibling checkouts, the consuming root workspace needs:

```toml
[patch.crates-io]
wgpu-hal = { path = "../tileink/vendor/wgpu-hal" }
```

The Tileink root already patches `vendor/wgpu-hal` and owns it as a workspace member.
gfx_ui points to the sibling path and does not list the HAL in its workspace members.
The registry Tileink library does not bundle or automatically apply a Cargo patch; downstream
DX12 users must select the corrected HAL in their root workspace until upstream includes it.
The public Tileink repository contains the complete source needed for that selection.

Commit consumer lockfiles and use `--locked`. On dependency updates, verify the active
`wgpu -> wgpu-core -> wgpu-hal` edge; a newer registry HAL can otherwise bypass an older patch:

```powershell
cargo tree --locked -p wgpu-core@30.0.0 --depth 1
cargo metadata --locked --offline --format-version 1
```

The selected 30.0.0 package must have a null registry source and resolve to
`tileink/vendor/wgpu-hal/Cargo.toml`. Merely appearing as a workspace member is insufficient.

## Verification

Run from Tileink, in release mode and with one test thread:

```powershell
cargo test --release --locked -p wgpu-hal@30.0.0 --lib --features vulkan,dx12 -- --test-threads=1
$env:TILEINK_PARITY_DXCOMPILER = '<absolute path to dxcompiler.dll>'
cargo test --release --locked -p tileink --test dx12_texture_order -- --ignored --test-threads=1
cargo test --release --locked -p tileink --example wgpu_backend_parity filter_sequence:: -- --ignored --test-threads=1
cargo bench --locked -p tileink --bench dx12_texture_order
```

The HAL tests cover fresh enumeration, growing/shrinking lists and incomplete results,
extension dependencies, headless instances, and the actual device-create feature chain.
The GPU test checks write-only pass ordering on explicit DX12 hardware; the filter sequence
checks repeated original SVG scenes through DX12/Vulkan and both WGPU texture modes.
Both GPU tests are opt-in and must be run explicitly; an ignored test is not a pass.
Rendering changes also require Tileink's ordinary tests, complete SVG/examples, PNG review,
formatting and lint. gfx_ui runs its framework/component/gallery regression suite against
the same selected dependency. No automatic backend fallback is allowed in exact-pixel tests.

The DX12 Criterion workload measures command encoding, submission and GPU completion,
without shader compilation or readback. On RTX 4090 / DX12 driver 32.0.16.1062, one pair
measured 90.28 to 87.66 microseconds (Criterion: improved), and eight pairs measured 169.01
to 211.46 microseconds (Criterion: **regressed**, about +25%). The old condition also failed
the final pixel assertion; it allowed dependent writes to overlap illegally. Retain this
cost as an explicit limitation, not as evidence that all performance gates pass. It is a
small dispatch workload, not a measurement of an application frame or resize PMax.

## Upgrades

Start from upstream source compatible with the selected wgpu/wgpu-core versions, compare
all three deltas, and remove each local change once upstream supplies equivalent semantics.
Port each remaining change with its regression, update provenance and consumer locks, then
verify active dependency edges again. A version-number edit alone is not an upgrade.
Upstream HAL 30.0.1 still omits write-only UAV ordering and is not a replacement for that fix.

Rerun hardware regressions and representative native resize workloads before adoption.
Vulkan deferred allocation can move work from configuration into acquisition: include
acquisition and whole-frame mean, P95 and maximum. Preserve fresh capabilities, exact client
extents, synchronization lifetimes and unsupported-device fallbacks. Previous Replay PMax
limitations remain; moving the source is not a new performance optimization.
