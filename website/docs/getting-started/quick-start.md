---
title: 快速开始
---

# 快速开始

创建 `Canvas`，记录绘图命令，再用 `NativeRenderer` 提交。以下代码使用 Windows 默认 DX12；其他平台应选择对应后端和 Cargo feature。

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
