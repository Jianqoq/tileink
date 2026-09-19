# Changelog

All notable changes to Tileink are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Tileink uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add Windows native retained rendering with dirty-range buffer uploads, persistent
  image/offscreen caches, journal recovery, independent renderer history and
  transient/persistent targets with explicit origins.
- Import host DX12/Vulkan devices and images with ownership pins and consumed
  per-use state/synchronization descriptors, including cross-queue fences and
  Vulkan binary/timeline semaphore ownership transfers. Rejected or unconfirmed
  work cannot publish target state/history. Add the `native_present` window example.

- Expose owned Windows native contexts and immediate Canvas/text renderers with
  explicit submission completion, premultiplied image readback and configurable
  root backgrounds. Native pipelines use persistent caches; conformance-only
  pipelines initialize lazily. Reject late DX12 debug-layer activation and preserve
  resource quarantine when no debug queue exists.
- Extend the existing SVG/example parity runner with `--native`, sharing scene
  callbacks and frozen fonts/resources across wgpu and native DX12/Vulkan routes.
  Complete Windows M4 acceptance: 1,712 SVGs and 45 example images match exactly
  across six routes on the recorded GPU/driver. Native retained/interop is M5.

- Complete the Windows M3 minimum native adapter: shared command batches/uniforms,
  context-owning receipts, DX12/Vulkan texture modules and strict input validation.
  Native module roots use named `.rs` files. Repeated four-API exact probes cover
  hardware RGBA8 loads and canonical Q16 interpolation, fixing the reproduced
  floating contraction mismatch. Vulkan OOM retries preserve confirmed prefixes;
  unknown submissions retain leases and stop safely. Complete immediate corpus acceptance remains M4.

- Extend native verification adapters with queued per-frame ownership, explicit
  completion/readback tickets and bounded teardown. Exact reverse-order results,
  foreign-ticket rejection and failure cleanup have permanent regressions.

- Add opt-in native HLSL builds with persistent content-addressed DXIL/SPIR-V
  artifacts, include/toolchain invalidation, strict minimum ABI reflection and
  independent MSL probe source. Default wgpu builds require no new tools.
- Add exact four-API clear/copy/layout/manual RGBA8 sampling probes, native driver
  pipeline caches and validation-layer checks. Native retained/interop and
  Metal compiler/GPU acceptance remain unfinished.

- Split the default WGPU feature from CPU scene/materializer code and opt-in
  DX12/Vulkan feature selections. Native constructors explicitly report unavailable
  backends or unsupported capabilities.
- Share frame/layer/filter scheduling, incremental state and resource-lifetime
  contracts with WGPU as the first executing adapter.
- Plan independently maintained MSL source and Apple toolchain validation for M2
  on macOS. DX12/Vulkan continue to share HLSL; native Metal rendering remains a
  later milestone.

- Added a same-GPU WGPU DX12/Vulkan reference runner with exact raw RGBA comparison,
  immutable run manifests, input-resource hashes and failure artifacts. This is M0 of the
  native HLSL backend plan; native API backends are not implemented yet.
- Added a Criterion benchmark for rotated pattern sampling at two render sizes and both
  WGPU texture paths.

### Fixed

- Keep pooled filter sampling in logical texel space so capacity reuse after resize preserves
  exact channel values; remove the unused filter sampler binding and cover degenerate axes.
- Share renderer plan metadata reuse and text preparation decisions across backend adapters.
- Reuse the existing retained chunk lookup to classify old batch membership, preserving
  plain/backdrop transitions while removing a repeated walk over changed nodes.
- Remove the second changed-node classification walk when existing batch eligibility
  and the clean-plan invariant already establish the result; cover resize, compaction
  and mixed position-patch updates against fresh materialization.
- Skip root-layer insertion scans for content-only changes; retain journal-gap
  reconciliation and immutable published-frame semantics.
- Reuse unchanged empty/single-element retained patch indexes without sharing nonempty patch
  payloads or introducing an additional equality scan for larger indexes.
- Reduce repeated uniform indexing, filter pipeline creation and retained damage/order work
  while preserving buffer identity, device ownership, history and painter-order semantics.

- Share filter graph, blur and glass pass scheduling with typed GPU kernel encoding.
  Reject failed graph clears and pointwise recording, and release Merge outputs
  when input resolution fails. Preserve downsample/partial worklists on every exit.

- Preserve unvisited Backdrop input under same-frame cache pressure, and retain
  complete filter input domains and painter-order damage for scoped updates.
- Resolve structural deletion/reparenting damage in both old and new local
  command trees, avoiding unnecessary full redraws from missing damage history.

- Bound filter source/auxiliary textures only as sampled resources, avoiding an illegal DX12
  UAV/SRV state combination that could invalidate the device during rendering.
- Compensated pattern transform product rounding so exact cancellation does not select a texel
  from the opposite repeat edge. Pixel changes require the repository's human PNG review.

## [0.1.2] - 2026-08-31

### Added

- Documented how to render directly into an acquired WGPU surface texture and present it, with a
  complete `winit` example covering setup, resize, surface recovery, and presentation.

### Fixed

- Made the direct-surface example require the actual `Rgba8Unorm` format and texture usages needed
  by both native and portable WGPU paths instead of accepting an unsupported sRGB target.
- Corrected WGPU destination validation messages and updated code for Rust 1.98 Clippy.

## [0.1.1] - 2026-08-31

### Added

- Added DirectWrite baseline, matrix, metrics, and reference validation for LCD text rendering.

### Changed

- Improved LCD glyph coverage generation and GPU compositing to more closely match DirectWrite on
  both light and dark backgrounds.
- Updated WGPU example reference renders for the revised LCD rasterization.

## [0.1.0] - 2026-08-01

### Added

- Initial public release of the tile-based GPU-compute renderer.
- Added immediate `Canvas` and transactional, incremental `RetainedScene` APIs.
- Added paths, analytic SDF primitives, text, images, gradients, layers, masks, filters, backdrops,
  SVG rendering, and native WGPU output.

[Unreleased]: https://github.com/Jianqoq/tileink/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/Jianqoq/tileink/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/Jianqoq/tileink/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Jianqoq/tileink/tree/v0.1.0
