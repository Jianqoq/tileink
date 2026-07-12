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

For retained text, insert the text Canvas as a leaf, call `replace_scene` for changed content, and use `set_transform` for movement only.
