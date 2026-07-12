---
sidebar_position: 1
title: Immediate 完整示例
---

# Immediate Canvas 示例

```rust
use peniko::{Color, kurbo::{Affine, Rect}};
use tileink::{Canvas, FillRule, Radius, WgpuRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut canvas = Canvas::new(800, 480, 1.0);

    canvas.push_rect(
        Rect::new(0.0, 0.0, 800.0, 480.0),
        Radius::ZERO,
        Color::from_rgb8(16, 22, 38),
    );

    canvas.push_clip_layer(
        Rect::new(40.0, 40.0, 760.0, 440.0).to_path(0.1),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let card = canvas.push_rect(
        Rect::new(90.0, 90.0, 360.0, 280.0),
        Radius::all(28.0),
        Color::from_rgb8(98, 82, 238),
    );
    canvas.set_draw_color(card, Color::from_rgb8(80, 190, 220));
    canvas.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(800, 480, Color::TRANSPARENT);
    renderer.render(&canvas);
    renderer.image().save("immediate.png")?;
    Ok(())
}
```

对于窗口输出，参考仓库 `examples/winit_svg_tiger.rs` 的 WGPU surface 配置与 resize 处理。
