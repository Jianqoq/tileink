---
sidebar_position: 1
title: Immediate example
---

# Immediate Canvas example

```rust
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius, WgpuRenderer};

let mut canvas = Canvas::new(800, 480, 1.0);
canvas.push_rect(Rect::new(0.0, 0.0, 800.0, 480.0), Radius::ZERO, Color::from_rgb8(16, 22, 38));
let card = canvas.push_rect(Rect::new(90.0, 90.0, 360.0, 280.0), Radius::all(28.0), Color::from_rgb8(98, 82, 238));
canvas.set_draw_color(card, Color::from_rgb8(80, 190, 220));
let mut renderer = WgpuRenderer::new_default_device(800, 480, Color::TRANSPARENT);
renderer.render(&canvas);
renderer.image().save("immediate.png")?;
```

See `examples/winit_svg_tiger.rs` for a complete WGPU surface and resize loop.
