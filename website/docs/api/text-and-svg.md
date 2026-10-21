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
| `layout_buffer(font_system, buffer)` | 复用 retained `cosmic_text::Buffer`，只 shape pending changes |
| `ink_bounds_at_scale(font_system, layout, origin, scale)` | 当前 DPI／origin 下的非透明字形像素边界，返回逻辑坐标 `Option<Rect>`；按 glyph／raster 配置缓存，空白文本或无效输入返回 None |
| `layout_outline_path(font_system, layout, origin)` | 收集 glyph outlines |
| `clear_glyph_caches()` | fonts/resources 发生外部重大变化时清理 |

`TextLayout::is_empty()` 与 `bounds()` 用于跳过空文本和计算 placement/damage。
可视标签居中应使用 `ink_bounds_at_scale`，排除透明栅格 padding，不能把 1× 缓冲区边界直接当作
Retina／分数 DPI 的可见字形边界。该只读测量接口不改变 layout、damage bounds、字形采样或 SVG
输出。最终字形光栅化（含 LCD 过滤）完成后，像素与非透明边界存入同一个缓存项；测量只合并
缓存边界，不再次扫描像素或维护第二份边界缓存。测量和绘制应复用同一个 `TextContext` 与相同
face ID 的字体库，否则各自的上下文仍会独立生成字形。`clear_glyph_caches()` 同时失效像素、边界
和已准备的文字图集。普通文字仍按基线／行高排版，不会自动改成可见区域居中。

`TextRasterOptions::new()` 可链 `with_subpixel_mode`、`with_composite_mode`、`with_alpha_mask_embolden`、`with_subpixel_mask_embolden`。`TextSubpixelMode` 为 `None/Rgb/Bgr`，`TextCompositeMode` 为 `Srgb/Linear`。两个 embolden 参数以光栅像素指定额外字重，分别用于灰度和 RGB/BGR mask；默认值为 `0.11944195` 和 `0.03273267`。对比度覆盖率补偿由原生 shader 固定实现，已删除没有运行时消费者的 `TextCoverageParams` 配置。

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
