---
sidebar_position: 6
title: Text and SVG API
---

# Text

`TextLayoutOptions::new` can be extended with `with_size`, `with_line_height`, `with_attrs`, and `with_alignment`. Its text, size, constraints, attrs, and alignment fields are public.

`TextContext::new`, `layout`, `layout_buffer`, `layout_outline_path`, `raster_options`, `set_raster_options`, and `clear_glyph_caches` own layout/raster state. `layout_buffer` shapes pending changes in a retained `cosmic_text::Buffer` and extracts Tileink glyph data without allocating and shaping a second buffer. `TextLayout` exposes `is_empty` and `bounds`. `TextRasterOptions` builds with `new`, `with_subpixel_mode`, `with_composite_mode`, `with_alpha_mask_embolden`, and `with_subpixel_mask_embolden`; modes include None/Rgb/Bgr subpixel and Srgb/Linear composition. The embolden values add outline weight in raster pixels to grayscale and RGB/BGR masks, respectively, with defaults `0.11944195` and `0.03273267`. Native shaders own contrast-dependent coverage compensation; the unused runtime `TextCoverageParams` settings have been removed.

`TextContext::ink_bounds_at_scale(font_system, layout, origin, scale)` returns nontransparent
raster coverage in logical coordinates at the actual DPI/origin. Pixels and visible bounds share
one cache entry, computed after final raster transforms and channel-area integration. Measurement unions
cached bounds without rescanning pixels or keeping a second bounds cache. Reuse the same
`TextContext` and matching font database for recording/measurement and atlas preparation to avoid
duplicate rasterization; independently created contexts remain independent. `clear_glyph_caches`
invalidates pixels, bounds and prepared text. The API does not change ordinary baseline/line-height
layout, glyph sampling, damage bounds or SVG output. Blank text and invalid coordinates/scale
return `None`.

# SVG

`SvgOptions` exposes `tolerance` and base `transform`. `Canvas::push_svg` uses defaults; `push_svg_with_options` applies custom options. Lowering is atomic. `SvgError::unsupported` constructs an error and `feature()` returns the unsupported feature name.
