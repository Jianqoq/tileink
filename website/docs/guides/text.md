---
sidebar_position: 2
title: 文本
---

# 文本

Tileink 使用 cosmic-text 做 shaping/layout，使用 `TextContext` 管理 layout 与 glyph preparation。

```rust
use peniko::{Color, kurbo::Point};
use tileink::{Canvas, TextContext, TextFontSystem, TextLayoutOptions, NativeRenderer};

let mut fonts = TextFontSystem::new();
let mut text = TextContext::new();
let layout = text.layout(
    &mut fonts,
    TextLayoutOptions::new("Hello, Tileink", 28.0),
);

let mut canvas = Canvas::new(640, 160, 1.0);
canvas.push_text_layout(&layout, Point::new(24.0, 52.0), Color::WHITE);

renderer.render_with_text(&canvas, &mut fonts, &mut text)?.wait()?;
```

如果应用已经为测量或编辑保留了 `cosmic_text::Buffer`，使用
`TextContext::layout_buffer(&mut fonts, &mut buffer)`。它会 shape pending changes 并复用
buffer 已有的 layout state，避免 `layout` 再分配并 shaping 一个 temporary buffer。
buffer 必须由同一个 font system 创建。

## 两种记录方式

- `push_text_layout`：保留 glyph/run records，由 GPU text path 渲染；必须调用 `render_with_text*`。
- `push_text_layout_as_path`：把 glyph outline 记录为普通 path；不依赖 text renderer，但失去 bitmap/color emoji 与 text-specific caching。

位图文本的平移与 DPI 换算使用宽精度，字形锚点和最终范围限制在 `i32` 像素坐标域内。
大坐标不会回绕到另一侧，范围计算在完成缩放和 padding 后才限制端点。
`push_text_layout` 和 `push_text_layout_clipped` 对非有限原点或坐标域外的空范围返回 `None`，不记录空 glyph/run。

## Raster options

`TextRasterOptions` 控制 composite/subpixel 策略。`TextSubpixelMode` 可选择 none、RGB/BGR 等模式；`TextCompositeMode` 控制 coverage 如何应用到颜色。窗口目标与字体 atlas 参数变化会使相关资源重新准备。

Windows 的普通字形使用 DirectWrite 生成覆盖率掩码，布局、缓存、atlas 与 GPU 合成仍由 Tileink 管理。
其他平台使用 Swash/Harmony；显式轮廓扩张和不受支持的原生字体也保留 portable 路径。
RGB/BGR 掩码使用三个通道，`None` 使用单通道 alpha；不再追加平滑卷积。
默认 Linear 模式使用基于前景颜色的覆盖率预补偿，再使用与中间图层一致的 RGBA source-over，
替换原有的经验指数、轴线和笔画中心补偿。先补偿字形像素，再做小数位移插值，
避免进入透明度图层或移动时文字突然加重。Windows 默认不扩张普通字形轮廓。
Srgb 模式仍表示显式的编码颜色 source-over。
彩色字体仍优先使用其彩色轮廓或位图，切换 subpixel 模式不会将彩色 emoji 转成单色。

LCD 的三个颜色通道覆盖率只在不透明目标像素上分别合成。透明目标（包括动画的离屏透明图层）
使用通道平均覆盖率表示一个 alpha；单个 RGBA alpha 无法保留三个独立的透射率。
原先使用最大通道作为 alpha 会在图层覆盖到背景时加重字形边缘，这里修复的是合成根因。
图层插入、透明度变化与小数位移都必须保持字形覆盖一致，不能靠改变字重补偿。

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

经过仿射变换的字形位图采用双线性采样，字形外部按透明像素处理。整数位移保持原始像素，
小数位移不会将动画文字吸附到物理整数像素。各原生后端的粗粒度字形边界均包含插值范围。

## Shared UI line geometry

`text_layout_runs(&buffer)` is the common line-placement view used by both `layout` and
`layout_buffer`. It rounds font ascent/descent before flooring the upper half-leading, while
preserving fractional glyph advances, line heights and scrolling. Empty paragraphs keep their
line boxes. Raster and outline glyphs use the same resolved baseline plus their shaping offsets;
there is no later Y truncation. DPI conversion and physical-pixel fitting remain separate.

Use this view for underlines, selection and caret geometry, and `text_hit(&buffer, x, y)` for
pointer-to-cursor conversion. Both borrow the retained shaped buffer without cloning or reshaping
it. Explicitly positioned Canvas and SVG glyphs keep their authored coordinates. See the
[implementation contract](https://github.com/Jianqoq/tileink/blob/main/docs/native/ui-text-metrics.md).
