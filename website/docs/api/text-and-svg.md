---
sidebar_position: 6
title: Text 与 SVG API
---

# Text API

## `TextLayoutOptions`

公开字段：`text`、`font_size`、`line_height`、`width`、`height`、`attrs`、`alignment`。

| Builder | 说明 |
|---|---|
| `new(text, font_size)` | line height 默认 1.2× font size |
| `with_size(width, height)` | layout constraints |
| `with_line_height(value)` | 行高 |
| `with_attrs(TextAttrs)` | cosmic-text 字体属性 |
| `with_alignment(Option<TextAlign>)` | 对齐 |

## `TextContext`

| 方法 | 说明 |
|---|---|
| `new()` | 创建 layout/raster caches |
| `raster_options()` / `set_raster_options()` | 查询/修改 text raster 策略 |
| `layout(font_system, options)` | 返回 `TextLayout` |
| `layout_outline_path(font_system, layout, origin)` | 收集 glyph outlines |
| `clear_glyph_caches()` | fonts/resources 发生外部重大变化时清理 |

`TextLayout::is_empty()` 与 `bounds()` 用于跳过空文本和计算 placement/damage。

`TextRasterOptions::new()` 可链 `with_subpixel_mode`、`with_composite_mode`、`with_coverage_params`。`TextSubpixelMode` 为 `None/Rgb/Bgr`，`TextCompositeMode` 为 `Srgb/Linear`。

# SVG API

`SvgOptions { tolerance, transform }` 默认 tolerance 0.1、identity transform。

```rust
canvas.push_svg(&tree)?;
canvas.push_svg_with_options(&tree, SvgOptions {
    tolerance: 0.05,
    transform: Affine::scale(2.0),
})?;
```

转换是事务式的：任何 unsupported feature 都在目标 Canvas 被修改前返回 `SvgError`。`SvgError::unsupported(name)` 构造错误，`feature()` 读取 feature 名。
