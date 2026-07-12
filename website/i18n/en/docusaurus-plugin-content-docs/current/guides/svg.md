---
sidebar_position: 3
title: SVG
---

# SVG

Parse input with `usvg`, create a Canvas at the intended extent, and call `push_svg` or `push_svg_with_options`. `SvgOptions` controls the base affine and curve tolerance.

Lowering is transactional: unsupported semantics return `SvgError` before modifying the destination Canvas. SVG paths, paint servers, raster images, text, masks, and filters then use the same Tileink pipeline as manually recorded content. Run `scripts/ps1/run_svg_tests.ps1` for the complete native/portable pixel suite.
