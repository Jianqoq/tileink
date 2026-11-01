---
sidebar_position: 2
title: Text
---

# Text

```rust
let mut fonts = tileink::TextFontSystem::new();
let mut text = tileink::TextContext::new();
let layout = text.layout(&mut fonts, tileink::TextLayoutOptions::new("Hello", 28.0));
canvas.push_text_layout(&layout, peniko::kurbo::Point::new(24.0, 52.0), peniko::Color::WHITE);
renderer.render_with_text(&canvas, &mut fonts, &mut text);
```

`push_text_layout` records glyph/run data and supports bitmap/color glyphs through the text renderer. `push_text_layout_as_path` converts scalable outlines to ordinary paths and does not require a text-aware render call. `TextRasterOptions` selects subpixel and compositing behavior.

Bitmap-text translation and DPI conversion use wide precision. Glyph anchors and final bounds
are limited to the signed `i32` pixel domain without wrapping; bounds are limited only after
scaling and padding. `push_text_layout` and `push_text_layout_clipped` return `None` for a
nonfinite origin or an empty range outside that domain, without recording empty glyph/run data.

Ordinary Windows glyphs use DirectWrite coverage masks; Tileink still owns layout, caching,
atlas upload and GPU composition. Other platforms use Swash/Harmony, which also handles explicit
outline expansion and unsupported native fonts. RGB/BGR have three channels; `None` has one.
The default Linear mode corrects coverage from the foreground color, then uses the same
encoded premultiplied RGBA source-over as intermediate layers. It replaces empirical exponent,
axis and stem-core boosts. Correct each texel before fractional interpolation so motion and
opacity-group insertion preserve ink without recoloring or rerasterizing cached font masks.
Ordinary Windows masks have zero default outline expansion. Explicit Srgb mode keeps encoded-color
source-over. Color fonts retain their color outlines or bitmap strikes in every
subpixel mode, so changing the mode does not turn color emoji into monochrome glyphs.
Each channel's pixel-area integration already provides antialiasing. The merged coverage is
preserved without another smoothing pass; the removed convolution spread glyph edges twice
and softened small UI text.

Independent LCD channel coverage requires an opaque destination pixel. Transparent targets,
including offscreen opacity groups used by animations, use the channel mean as scalar coverage.
One RGBA alpha cannot retain three independent transmittances; using their maximum made glyph
edges heavier when the layer was composited. This fixes the compositing cause. Group insertion,
opacity changes and fractional motion must preserve coverage without changing the font weight.

If the application already retains a `cosmic_text::Buffer` for measurement or editing, call
`TextContext::layout_buffer(&mut fonts, &mut buffer)`. It shapes pending changes and reuses the
buffer's existing layout state, avoiding the allocation and shaping cost of `layout`'s owned
temporary buffer. The buffer must have been created with the same font system.

For retained text, insert the text Canvas as a leaf, call `replace_scene` for changed content, and use `set_transform` for movement only.

The text-aware renderer also retains prepared glyph images across successive flat `Canvas` frames.
Position-only changes reconcile glyph/run records without cloning cached bitmap data or rebuilding
the atlas lookup. Contiguous glyph/run dirty ranges flow into GPU text upload, so unchanged records
and blobs are not rebuilt. Changing raster options or calling `TextContext::clear_glyph_caches`
invalidates that prepared atlas. The renderer rebuilds from the live frame after retained image
data exceeds 32 MiB, discarding stale images. Recreating the renderer for a device reset starts
with an empty prepared-text cache.

Affine-transformed glyph bitmaps use bilinear sampling with transparent texels outside the glyph.
Integer translations preserve exact texels; fractional translation no longer snaps animated labels
to physical pixels. Coarse glyph bounds include the interpolation footprint on all native backends.

## Shared UI line geometry

`text_layout_runs(&buffer)` is the common line-placement view used by both `layout` and
`layout_buffer`. It rounds font ascent/descent before flooring the upper half-leading, while
preserving fractional glyph advances, line heights and scrolling. Empty paragraphs keep their
line boxes. Raster and outline glyphs use the same resolved baseline plus their shaping offsets;
there is no later Y truncation. DPI conversion and physical-pixel fitting remain separate.

Use this view for underlines, selection and caret geometry, and `text_hit(&buffer, x, y)` for
pointer-to-cursor conversion. Both borrow the retained shaped buffer without cloning or reshaping
it. Explicitly positioned Canvas and SVG glyphs keep their authored coordinates. See the
[implementation contract](https://github.com/Jianqoq/tileink/blob/main/docs/native/ui-text-metrics.md).
