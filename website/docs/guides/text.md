---
sidebar_position: 2
title: 文本
---

# 文本

Tileink 使用 cosmic-text 做 shaping/layout，使用 `TextContext` 管理 layout 与 glyph preparation。

```rust
use peniko::{Color, kurbo::Point};
use tileink::{Canvas, TextContext, TextFontSystem, TextLayoutOptions, WgpuRenderer};

let mut fonts = TextFontSystem::new();
let mut text = TextContext::new();
let layout = text.layout(
    &mut fonts,
    TextLayoutOptions::new("Hello, Tileink", 28.0),
);

let mut canvas = Canvas::new(640, 160, 1.0);
canvas.push_text_layout(&layout, Point::new(24.0, 52.0), Color::WHITE);

renderer.render_with_text(&canvas, &mut fonts, &mut text);
```

如果应用已经为测量或编辑保留了 `cosmic_text::Buffer`，使用
`TextContext::layout_buffer(&mut fonts, &mut buffer)`。它会 shape pending changes 并复用
buffer 已有的 layout state，避免 `layout` 再分配并 shaping 一个 temporary buffer。
buffer 必须由同一个 font system 创建。

## 两种记录方式

- `push_text_layout`：保留 glyph/run records，由 GPU text path 渲染；必须调用 `render_with_text*`。
- `push_text_layout_as_path`：把 glyph outline 记录为普通 path；不依赖 text renderer，但失去 bitmap/color emoji 与 text-specific caching。

## Raster options

`TextRasterOptions` 控制 composite/subpixel 策略。`TextSubpixelMode` 可选择 none、RGB/BGR 等模式；`TextCompositeMode` 控制 coverage 如何应用到颜色。窗口目标与字体 atlas 参数变化会使相关资源重新准备。

## 扁平 Canvas 的 prepared text 复用

连续调用 `render_with_text*` 渲染新记录的扁平 `Canvas` 时，renderer 会复用已准备的
glyph 图片和 atlas。相同 cache key 的位置变化只更新坐标；glyph/run 的连续 dirty
range 会直接传给 GPU text upload，因此无需重建未变化的 text records 和 blobs。

修改 raster options 或调用 `TextContext::clear_glyph_caches` 会使 prepared atlas
失效。长期不再可见的 glyph 图片允许暂时保留以便复用；当累计图片数据超过 32 MiB
时，renderer 会从当前帧重建缓存并丢弃旧图片。设备重建会创建新的 renderer，因此
不会沿用旧设备的 prepared text。

## Retained text

把包含 text runs 的 Canvas 放入 `RetainedScene`，使用 `render_retained_with_text*`。替换文本时创建新的 `Rc<Canvas>` 并 `replace_scene`；仅移动文本时只调用 `set_transform`。
