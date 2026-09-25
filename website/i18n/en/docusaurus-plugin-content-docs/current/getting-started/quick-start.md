---
title: Quick start
---

# Quick start

Create a `Canvas`, record draw operations, then submit it through `NativeRenderer`. This example uses the Windows DX12 default; select the matching `NativeBackend` and Cargo feature on other platforms.

```rust
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, NativeBackend, NativeRenderer, Radius};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = Canvas::new(640, 360, 1.0);
    canvas.push_rect(Rect::new(48.0, 48.0, 280.0, 180.0), Radius::all(24.0), Color::from_rgb8(91, 83, 255));
    let mut renderer = NativeRenderer::new(NativeBackend::Dx12, 640, 360)?;
    renderer.render_to_image(&canvas)?.readback()?.save("out.png")?;
    Ok(())
}
```
