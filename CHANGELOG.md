# Changelog

## Unreleased

- Native DX12, Vulkan, and Metal renderer paths use shared scene semantics and backend-specific GPU resources.
- Immediate and retained rendering support renderer-owned images, host textures, and host presentation targets.
- SVG fixture rendering and release-mode test runners use the selected native backend.

## 0.1.2

- Added transactional retained scenes, incremental materialization, text preparation, and SVG lowering.
