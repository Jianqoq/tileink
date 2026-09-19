# M5: retained frames and host targets

Status: complete for Windows DX12/Vulkan (2026-09-19). Baseline: `79e6153e` (Windows M4 complete).
The user requested M5 before connecting gfx_ui to native DX12/Vulkan. No gfx_ui
native features are claimed as implemented. Performance comparisons remain waived;
exact pixels, lifetime correctness and actual reuse are required.

## Implementation order and acceptance

1. Submission progress and persistent GPU allocations. Hosts can inspect a fence
   without a CPU wait or consuming image readback. Persistent resources carry
   logical-device identity and remain leased until confirmed GPU completion.
2. Renderer-owned targets and uploads survive successive submissions. Dirty ranges
   update existing buffers; immutable uploads are not resent. Scratch leases and
   offscreen history use the shared policies rather than backend-specific scheduling.
3. Native retained recording uses shared materialization, journal recovery, damage,
   partial filter/backdrop and frame execution. Compare Auto with an independent
   ForceFull renderer on every frame, including removal, empty damage and resize.
4. Host-created devices/queues and transient/persistent targets have explicit
   ownership, state/layout, queue synchronization and history contracts. Validate
   foreign devices, extent/origin, history changes, rejected work and device loss.
5. DX12/Vulkan examples demonstrate target acquisition, rendering, completion and
   presentation. The host owns swapchain and window policy. Run the complete retained
   scenario corpus on both native and both wgpu APIs with zero byte differences.

## Invariants

- No CPU readback/reupload presentation path and no implicit wgpu fallback.
- API-neutral materialization, damage, cache identity and reuse policies stay shared.
- A rejected recording/submission cannot publish new history or resource state.
  Unconfirmed work quarantines resources and prevents unsafe reuse.
- Dropping a renderer or target cannot destroy allocations still referenced by a
  submitted frame. Imported Vulkan ownership must outlive command completion.
- External targets retain only the history explicitly promised by their identity;
  transient swapchain images must not inherit unrelated image contents.
- Polling completion is observational: no wait, map, readback consumption or premature
  resource retirement. The explicit receipt completion/readback path still retires work.
- Native DX12 and Vulkan source stays separated; new Rust modules use named `.rs`
  roots. Shader constants stay in HLSLI; no ABI JSON input or hidden shader includes.

## Evidence

### Persistent target foundation

Implemented: nonblocking `NativeSubmission::is_complete`, same-device owned
`NativeTexture`, explicit texture readback, direct `NativeRenderer` target output,
and reuse of the renderer-owned root across equal-sized frames. Rendering clears
the root at the shared frame boundary before blending; it no longer creates an
intermediate root and copies the entire frame into the persistent output. A resize
publishes its replacement only after the queue accepts the submission.

Native frames retain raw allocation owners without retaining their context through
a cycle. DX12 persistent allocations finish in COMMON; Vulkan allocations finish
in GENERAL. Initialization is selected when native commands are recorded, so CPU
batches prepared before an earlier submission cannot clear that submission's pixels.

Focused GPU evidence: `m5-persistent-copy.log` verifies initial partial writes,
unchanged pixels, queued batches, repeated readback, early handle drop and
copy-source/copy-destination transitions followed by compute writes on both APIs.
`m5-public-renderer.log` verifies exact four-route public rendering, clear-color
changes, replacement with an empty frame, wrong-size/foreign-device rejection and
observational completion polling. Both focused suites pass. Independent standards
and spec reviews of this foundation found no outstanding issue.

Full foundation verification passes: 1,049 ordinary release library tests,
integration suites, default/native feature checks, formatting and strict native
all-target Clippy (`m5-foundation-*.log`). The complete six-route corpora report
1,712 SVGs and 45 example images with zero different pixels. All generated PNG
hashes also match the corresponding M4 outputs; there are no new visual changes.
Reports: `m5-foundation-svg/report.json`, `m5-foundation-examples/report.json`.

### Scratch allocation reuse

Native public recording now uses the shared `SceneResourcePool` for scratch and
local offscreen allocations. Sibling scenes cannot acquire pending storage from
the same batch. The next submitted/discarded batch boundary enables reuse without
a CPU completion wait, and each new lease explicitly clears its GPU pixels.
These are scratch leases, not retained pixel history; cached retained surfaces
must keep separate leases rather than returning their contents to this pool.

`m5-surface-pool-recovery.log` passes on both APIs: physical allocation reuse,
nonaliasing siblings, ordered in-flight reuse, reverse readback, size changes and
discard-after-recording-error followed by reuse with correct initialization.
`m5-surface-pool-public.log` verifies repeated ordinary/filtered/empty frames on
all four APIs. Release tests, independent native feature checks, formatting and
strict native all-target Clippy pass (`m5-pool-*.log`). Standards and spec reviews
have no outstanding findings after the discard/retry regression was added.

The full six-route SVG (1,712) and example-image (45) corpora also pass with zero
different pixels and unchanged PNG hashes relative to the foundation and M4.
Evidence: `m5-pool-svg/report.json`, `m5-pool-examples/report.json`. No performance
comparison was run, as requested.

Evidence is stored under `G:/Code/northstar-trading-app/target/agent-work/m5-*`.
All four Windows M5 plan checkboxes and the host-interop exit condition are verified.
Mac compilation and GPU validation remain deferred for lack of hardware and are
not replaced with a Windows result.

## Retained execution and incremental uploads

Native recording now uses shared retained materialization, journal recovery,
active tiles/batches, partial root clears, filter/backdrop execution and offscreen
history. Only accepted submissions publish history, target content versions,
image cache initialization and buffer-upload acceptance. Discarded CPU batches
force a complete retry upload before consuming newer dirty journals.

Persistent GPU buffers cover draw/paint/layer records, stable batch IDs, geometry
lines/paths/chunks/ranges, cumsum metadata and coarse/fine text data. Dirty ranges
update existing allocations; unchanged ranges are not resent. Immutable raster
and vector images persist as GPU textures. Offscreen cache leases prevent scratch
reuse from clearing history. Frame-local uniforms/work buffers still describe the
current commands and are not misrepresented as immutable scene uploads.

Three root-cause fixes have dedicated regressions: text/raster options and glyph
cache generations invalidate otherwise static retained history before begin_frame;
multiple image keys sharing one vector canvas reuse one child output per batch;
DX12 imported targets must support shader reads as well as UAV writes.

## Host integration

Typed DX12/Vulkan context and texture imports preserve host ownership. Initial and
final resource states/layouts, persistent/transient targets, origins, accepted
content versions and logical-device identities are explicit. Native frames retain
allocation leases without context cycles. See [host interop](host-interop.md).

`native_present` creates host devices and real Windows swapchains, imports them,
executes native retained frames and presents. DX12 uses a host-owned UAV target
and GPU copy into flip buffers; Vulkan renders acquired RGBA8 images directly.
Both exercise resize and bounded frames in hidden smoke mode. Fence completion
covers host operations, and unconfirmed teardown preserves resource owners.

Final validation results and machine scope are recorded in `m5-verification.json`.
The gfx_ui native feature integration is a subsequent task, not part of this
Tileink implementation. No performance comparison was requested or performed.


## Reproduction

Set `TILEINK_NATIVE_GPU` to the physical adapter LUID, `TILEINK_NATIVE_DXC_PATH`
to the pinned native DXC executable, and `TILEINK_PARITY_DXCOMPILER` to the wgpu
DXC DLL. The recorded machine uses DXC 1.8.2502 for native shaders and Windows
SDK 10.0.26100.0 for the wgpu DX12 compiler. Enable DX12 debug and Vulkan validation
layers; run GPU processes serially. Each parity output directory must be new.

```powershell
cargo test --release --features native -- --test-threads=1
cargo test --release --features native --lib gpu_tests::persistent -- --ignored --test-threads=1
cargo test --release --features native --lib gpu_tests::retained_renderer -- --ignored --test-threads=1
cargo test --release --features native --lib gpu_tests::interop -- --ignored --test-threads=1
cargo test --release --features native --lib target_use -- --ignored --test-threads=1
cargo run --release --features native --example wgpu_backend_parity -- --native --textures both --luid $env:TILEINK_NATIVE_GPU --dxc $env:TILEINK_PARITY_DXCOMPILER --suite retained --output target/m5-retained-review
cargo run --release --features native --example wgpu_backend_parity -- --native --textures both --luid $env:TILEINK_NATIVE_GPU --dxc $env:TILEINK_PARITY_DXCOMPILER --input src/svg/tests --output target/m5-svg-review
cargo run --release --features native --example wgpu_backend_parity -- --native --textures both --luid $env:TILEINK_NATIVE_GPU --dxc $env:TILEINK_PARITY_DXCOMPILER --suite examples --output target/m5-examples-review
cargo run --release --features native --example native_present -- dx12 --smoke
cargo run --release --features native --example native_present -- vulkan --smoke
cargo fmt --all --check
cargo clippy --release --features native --all-targets -- -D warnings
```

Native-only build checks use `--no-default-features --features native-dx12`,
`native-vulkan`, and `native`, each with `cargo check --release --all-targets`.
Default wgpu remains independently buildable without the native shader toolchain.


## Final acceptance ? 2026-09-19

M5 is complete on the recorded Windows RTX 4090/driver. The retained suite passes
29 consecutive states across 36 variants: wgpu DX12/Vulkan with native/portable
textures and native DX12/Vulkan, each using owned/transient/persistent output and
independent Auto/ForceFull renderers. Every comparison has zero different pixels
and zero channel delta. Full six-route acceptance passes 1,712 SVGs and 45 example
images. All 10,542 output PNG hashes match the preceding M5 pool/M4 baseline.
There are no new rendering changes requiring image approval.

The release suite passes 1,052 ordinary library tests plus integration/example
suites. Focused real-GPU tests cover persistent allocations/uploads/images,
retained text invalidation, imported ownership, rejected state declarations,
DX12 cross-queue fences and signal failure, Vulkan binary/timeline semaphores and
cross-family ownership, including empty-damage handoffs. Native validation passes;
the existing wgpu optimized-clear advisory does not report a correctness error.
Both Windows native presentation smoke runs complete eight frames and resize with
exit code 0. Default and native feature checks, formatting and strict all-target
Clippy pass. Standards and spec reviews have no outstanding findings.

[Verification manifest](m5-verification.json) records code/log/report hashes,
route counts, physical GPU and driver identity, baseline PNG hashes and limitations.
This is M5 acceptance on that device, not the wider M6 platform/package matrix.
