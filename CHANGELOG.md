# Changelog

All notable changes to Tileink are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Tileink uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/Jianqoq/tileink/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/Jianqoq/tileink/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Jianqoq/tileink/tree/v0.1.0
