# Shared UI text metrics

High-level `TextContext::layout` and `layout_buffer` use one line-placement rule exposed by
`text_layout_runs`. It borrows the existing shaped buffer and cached paragraph layouts. It does
not allocate another buffer, reshape text, round glyph advances, change line heights, or modify
the caller's Cosmic Text buffer.

For each visual line:

- Round the maximum font ascent and descent in logical UI units.
- Floor `(line_height - ascent - descent) / 2` for the leading above the baseline. The remainder
  stays below, including for fractional or negative leading.
- Add that leading and ascent to the exact line top. Preserve fractional scrolling and spacing.
- Add each glyph's shaping offsets once. Use that same origin for raster and outline placement;
  the existing bounded CanvasGlyph conversion owns subpixel bins and integer carry.

This fixes the root cause of vertically biased UI text: previously fractional font metrics fed a
baseline that was truncated before rasterization, while outline text retained a different origin.
Noto Sans 14px/21px now places both at baseline 16, rather than raster baseline 15. Browser-style
font-metric rounding and upper-leading placement are defined in Chromium's font_metrics.cc and
core/layout/inline/line_utils.cc; no per-font or component compensation is used.

Visible-line filtering uses the resolved font bounds together with the original line box.
Partly visible empty paragraphs remain available to editing geometry. `text_hit` uses those
same lines and shaped glyph advances. Aligned and mixed-direction text select the nearest
physical glyph edge; within a ligature, logical grapheme order follows the glyph direction.
Nonfinite pointer positions have no hit. This replaces the old UI placement/hit path; there
is no compatibility switch retaining the former baseline calculation.

Consumers that need UI baseline coordinates must use `text_layout_runs`, rather than the raw
shaper's `LayoutRun::line_y`. Raw buffers remain the content/shaping source; their fractional
advances and line boxes are valid measurement data. Carets and selection cover those line boxes,
not the font ink bounds. Fractional intrinsic sizes therefore do not accumulate integer errors.

DPI and affine conversion happen later in the renderer. A baseline is not truncated in logical
space before scaling. Raster cache keys retain font identity, size, weight and flags; position
changes reuse images whenever their subpixel phase is unchanged. Explicitly positioned Canvas
and SVG glyphs retain their authored coordinates and do not acquire UI half-leading.

Regression coverage includes actual raster/outline layout, fractional spacing and scrolling,
negative leading, empty paragraphs, wrapping, font fallback, combining offsets, aligned/RTL
hit testing, grapheme clusters, and the retained-buffer/owned-layout equivalence contract.

## Validation

The normal single-thread release suite passed (915 library tests, 195 opt-in tests ignored,
plus integration/example tests). The explicitly run native GPU fractional-glyph-motion and
clipped-text fade tests passed, including 1x/2x DPI and both LCD channel orders. Strict release
Clippy and formatting passed. The complete SVG fixture renderer and all example builds passed;
all 1,712 generated SVG PNGs were byte-identical to the pre-change run on this machine.
The independent gfx_ui_shadcn fixture matches 48 installed-browser baseline/line-height pairs.
