---
sidebar_position: 3
title: SVG
---

# SVG

Tileink 接收已经由 `usvg` 解析的 tree：

```rust
use tileink::{Canvas, SvgOptions};

let svg = std::fs::read("icon.svg")?;
let tree = usvg::Tree::from_data(&svg, &usvg::Options::default())?;
let size = tree.size();
let mut canvas = Canvas::new(size.width().ceil() as u32, size.height().ceil() as u32, 1.0);
canvas.push_svg_with_options(&tree, SvgOptions::default())?;
```

`push_svg` 使用默认 options；`push_svg_with_options` 可设置 transform、tolerance 等转换行为。无法表达的 SVG feature 返回 `SvgError`，`feature()` 给出名称。

SVG 会转换成 Tileink 的 path、brush、image、text/layer/filter 语义，之后与手写 Canvas 走相同 pipeline。仓库 `scripts/ps1/run_svg_tests.ps1` 按 filters、masking、painting、paint servers、shapes、structure、text 分类做像素回归。

小尺寸 UI 图标可通过 `SvgOptions { pixel_grid: PixelGrid::new(effective_dpi),
..Default::default() }` 开启像素拟合。`effective_dpi` 包含最终祖先节点的等比缩放，最终 Canvas
原点应位于物理像素边界。SVG 变换后的水平/垂直填充边缘和普通描边中心线会对齐，描边宽度取整
为物理像素且至少一个像素；曲线和斜线保留抗锯齿。旋转、倾斜和非等比缩放保持原始几何。
Pattern tile、嵌入图像和 filter image buffer 保留各自的坐标系统。插画、数据图和 SVG
参考渲染应保持默认的 `pixel_grid: None`。这是 UI 图标模糊覆盖的根因修复，不改变通用 SVG 语义。
