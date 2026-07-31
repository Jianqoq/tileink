---
sidebar_position: 2
title: Quick start
---

# Quick start

```rust
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius, WgpuRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = Canvas::new(640, 360, 1.0);
    canvas.push_rect(
        Rect::new(48.0, 48.0, 280.0, 180.0),
        Radius::all(24.0),
        Color::from_rgb8(91, 83, 255),
    );
    let mut renderer = WgpuRenderer::new_default_device(640, 360, Color::TRANSPARENT);
    renderer.render(&canvas);
    renderer.image().save("out.png")?;
    Ok(())
}
```

Canvas coordinates are logical pixels; the scale factor determines physical output size. Prefer `Canvas` for one-shot or fully rebuilt scenes and `RetainedScene` for large UIs, editors, and mostly local updates.
