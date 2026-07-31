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
