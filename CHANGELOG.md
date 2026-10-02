# Changelog

## Unreleased

- SVG `feComposite` and `feConvolveMatrix` now honor each primitive's filter color space. Convolution processes premultiplied colors by default and applies bias using the unclamped result alpha, matching the reference output.
- SVG pattern brushes now account for the path and viewBox transforms when sampling pattern tiles, fixing their scale and phase in native renders.
- Improved progressive blur shallow-edge quality with direct small Gaussian kernels, denser full-resolution levels, and delayed downsampling. Added independent `Balanced` (default) and `High` quality policies, edge/motion regression tests, and quality-specific benchmarks.

- Added GPU progressive blur for content and backdrop layers, with smooth directional strength, a calibrated multiscale pyramid, and retained-cache integration. Includes DX12/Vulkan HLSL, Metal shaders, semantic/GPU tests, a visual example, and Criterion benchmarks.

- SVG `feBlend` and `feComponentTransfer` now honor each primitive's `color-interpolation-filters` setting. The renderer previously dropped the setting and processed default linearRGB filters in sRGB; it now converts their premultiplied inputs and outputs at the primitive boundary while preserving explicit sRGB filters.
- Native DX12, Vulkan, and Metal renderer paths use shared scene semantics and backend-specific GPU resources.
- Immediate and retained rendering support renderer-owned images, host textures, and host presentation targets.
- SVG fixture rendering and release-mode test runners use the selected native backend.

## 0.1.2

- Added transactional retained scenes, incremental materialization, text preparation, and SVG lowering.
