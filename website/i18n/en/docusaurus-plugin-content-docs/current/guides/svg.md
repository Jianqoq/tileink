---
sidebar_position: 3
title: SVG
---

# SVG

Parse input with `usvg`, create a Canvas at the intended extent, and call `push_svg` or `push_svg_with_options`. `SvgOptions` controls the base affine and curve tolerance.

Lowering is transactional: unsupported semantics return `SvgError` before modifying the destination Canvas. SVG paths, paint servers, raster images, text, masks, and filters then use the same Tileink pipeline as manually recorded content. Run `scripts/ps1/run_svg_tests.ps1` for the complete native/portable pixel suite.

For small UI glyphs, opt into `SvgOptions { pixel_grid: PixelGrid::new(effective_dpi),
..Default::default() }`. The effective DPI includes any final uniform ancestor scale, and the
final canvas origin must be on a physical pixel boundary. Axis-aligned fill edges and ordinary
stroke centerlines are fitted after the SVG transform; stroke widths round to whole physical
pixels with a one-pixel minimum. Curves and diagonal segments remain antialiased. Rotation, skew
and nonuniform scaling preserve exact geometry. Pattern tiles, embedded images, and filter image
buffers retain their independent coordinate systems. Keep `pixel_grid: None` for illustrations,
plots, and exact SVG reference rendering. This opt-in addresses blurry UI glyph coverage without
changing general SVG semantics.
