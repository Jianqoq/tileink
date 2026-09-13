# Changelog

All notable changes to Tileink are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Tileink uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add opt-in native HLSL builds with persistent content-addressed DXIL/SPIR-V
  artifacts, include/toolchain invalidation, strict minimum ABI reflection and
  independent MSL probe source. Default wgpu builds require no new tools.
- Add exact four-API clear/copy/layout/manual RGBA8 sampling probes, native driver
  pipeline caches and validation-layer checks. Production native rendering and
  Metal compiler/GPU acceptance remain unfinished.

- Split the default WGPU feature from CPU scene/materializer code and reserved
  opt-in DX12/Vulkan feature selections. Native constructors explicitly report
  unavailable backends; actual native rendering remains planned.
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
