# Native Metal backend

Native Metal renders the shared Canvas/RetainedScene execution plan with maintained
MSL and direct Metal commands. It does not execute WGSL or use wgpu/wgpu-hal at
runtime. Select exactly one backend feature:

```sh
cargo build --release --no-default-features --features metal
```

A full Xcode installation must supply `metal`, `metallib` and the macOS SDK via
`xcrun`; select it with `xcode-select` or `DEVELOPER_DIR`. Build artifacts are
cached by compiler/SDK contents, flags, included MSL sources and binding ABI.
The deployment target is macOS 12.0 and the shader language is Metal 2.4.
Device creation requires Apple7 or Mac2, argument-buffer tier 2 and 256-thread
workgroups. These capability checks are not certification of every matching GPU.
Actual verification currently covers **Apple M2, Xcode 16.2 / Metal 32023.404,
macOS 15.0.1 (24A348), macOS 15.2 SDK**. Intel/AMD Macs and other OS/driver versions remain unverified.

```rust,ignore
let context = tileink::NativeContext::new(
    tileink::NativeBackend::Metal,
    &tileink::NativeContextOptions::default(),
)?;
let mut renderer = tileink::NativeRenderer::with_context(&context, 640, 480)?;
let canvas = tileink::Canvas::new(640, 480, 1.0);
let image = renderer.render_to_image(&canvas)?.readback()?;
```

`physical_adapter` accepts the device's 16-digit lowercase hexadecimal registry
ID. `None` selects the system default device. Validation is opt-in and requires
`MTL_DEBUG_LAYER=1` **before process startup**. Shader pipelines are created lazily;
reflection checks active binding slots/types, uniform sizes and every scalar
field offset/type before dispatch.

## Execution and ownership

One shared compute batch becomes one command buffer. Tracked-resource encoder
boundaries establish compute/blit visibility. Ordinary render submissions and
resizes do not wait for GPU completion. Owning receipts retain buffers, textures,
argument tables and upload/readback storage until completion. Readback removes
Metal's row padding. Explicit completion/readback has a bounded wait; failed or
uncertain submissions cannot recycle in-flight resources.

`native_interop::metal` exposes typed device/queue, RGBA8Unorm texture and shared
event descriptors. Imports retain Objective-C ownership. The host must fulfill
the unsafe API's queue/synchronization contract. Textures must use tracked hazards
and support shader read/write; incompatible devices, usages or layouts fail before
recording. Shared-event waits/signals cover host queue handoff without blocking
submission. Tileink does not own a window, drawable acquisition or presentation.

## Shader coverage and numerical contract

The production catalog covers scan, cumsum, coarse allocation/emission, fine
paths/SDF/text/images/gradients/clipping/blending, and all filter families,
including SVG lighting/turbulence and liquid glass. Raw scene reads use explicit
buffer-length metadata where optional records require guarded access. The Metal
fine interpreter consumes the existing particle format, including spill stacks
and sparse active tiles.

Offline fast-math with preserved invariance matches the current wgpu-Metal
compiler policy. This is part of the semantic cache key, not a performance claim.
Safe-math compilation changed SDF half-alpha rounding and was rejected by exact
comparison. Glass square/rounded distance branches remain separate: merging them
allowed cancellation across neighboring distance samples, changing a finite-
difference normal and one refracted RGBA8 byte. The full glass example now serves
as a regression test; no pixel exception or tolerance is used.

## Reproducible verification

```sh
bash scripts/mac/run_metal_probes.sh
bash scripts/mac/run_native_metal_tests.sh
bash scripts/mac/run_native_metal_tests.sh --svg
bash scripts/mac/run_native_metal_tests.sh --examples
bash scripts/mac/run_native_metal_tests.sh --retained
bash scripts/mac/run_native_metal_tests.sh --present
```

All tests run in release mode with one test thread. Reference and native renderers
are separate, mutually exclusive feature builds pinned to the same registry ID.
Raw premultiplied RGBA8 is compared, including transparent pixels. The scripts
write evidence beneath `target/metal-validation`, never update tracked goldens,
and reject missing references. The shader inventory links maintained MSL entries
rather than declaring unported WGSL variants to be separate Metal programs.

The checked-in [verification receipt](metal-verification.json) records toolchain,
shader artifact and output hashes for this run. Raw reports remain in the local
evidence directory and can be regenerated with the commands above.

Verified corpus coverage on this M2 includes 1,712 SVGs and 45 example images,
with zero differing bytes. Focused checks cover all 19 SDF shapes, affine and
phase cases (375,732 samples repeated three times), CPU morphology/transfer
oracles, coarse work records, scan prefixes, texture arrays and row alignment,
all compiled entry-point reflection contracts, retained output/history,
persistent allocation reuse and imported-target shared-event synchronization.
This evidence does not imply cross-GPU/cross-OS pixel identity or a measured
performance improvement. The older cross-platform checked-in PNG differences
remain separate from same-device certification; no golden files were accepted
or replaced by this change.

Mac M1–M5 acceptance is complete on the recorded device. The [M5 closeout](m5-metal.md)
adds 29 consecutive retained states across 18 variants (522 exact outputs), failure
and import edge cases, and eight real host presentation frames with resize. The
broader M6 hardware/platform matrix is still open.
