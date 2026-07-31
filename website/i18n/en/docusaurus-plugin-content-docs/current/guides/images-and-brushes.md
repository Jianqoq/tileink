---
sidebar_position: 4
title: Images and brushes
---

# Images and brushes

Create images with `Image::new`, `from_rgba8`, or `from_premultiplied_rgba8`. Direct draws use `Canvas::push_image`; shared retained resources use `ImageKey`, `renderer.insert_image`, and `push_image_key`.

APIs accepting `impl Into<Brush>` accept a `Color` directly. Other brushes include linear, radial, sweep, four-corner, and image patterns. `PatternSampling` controls sampling and `PatternBrush::from_image_key*` selects explicit or natural sizing.

Replacing an image registry entry invalidates only the required resource tables and retained history; unrelated scene chunks remain intact.
