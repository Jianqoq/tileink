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
