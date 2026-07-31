---
sidebar_position: 2
title: 快速开始
---

# 快速开始

下面创建一个 640×360 logical-pixel Canvas，画圆角矩形并由 renderer 输出到 CPU `Image`。

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

    let mut renderer = WgpuRenderer::new_default_device(
        640,
        360,
        Color::TRANSPARENT,
    );
    renderer.render(&canvas);
    renderer.image().save("out.png")?;
    Ok(())
}
```

`Canvas` 坐标是 logical pixels；`scale_factor` 决定 physical output。`Canvas::new(640, 360, 2.0)` 的物理目标是 1280×720。

## 选择场景模型

| 场景 | 建议 |
|---|---|
| 每帧完整重新生成、内容较少 | `Canvas` |
| 大型 UI、局部移动或内容变化 | `RetainedScene` |
| 一次性 SVG 导出 | `Canvas::push_svg*` |
| 已有 WGPU texture/swapchain | `render_*_to_wgpu_texture` |
| 需要文本 | `TextContext` + `render_with_text*` |

下一步阅读 [架构总览](../architecture/overview.md) 或直接查看 [Canvas API](../api/canvas.md)。
