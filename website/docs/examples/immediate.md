---
title: Immediate 示例
---

# Immediate 示例

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

直接输出到宿主窗口时，参考 `examples/native_window/`。

该示例在进入事件循环前完成初始化，关闭时显式等待渲染和呈现结束，并向调用者报告错误。
不支持的后端配置会以失败状态退出；`metal --smoke` 验证八帧呈现和一次真实窗口缩放。
