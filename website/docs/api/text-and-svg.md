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
| `layout_buffer_runs(font_system, &buffer)` | 只读提取已排版的分段 `TextLayoutRun`；保留视觉行、metadata 和 RGBA8 颜色，不重新 shaping |
| `layout_runs_filtered(font_system, runs, include)` | 从调用方准备好的 UI 行视图提取绘制片段，保留原文字索引、位置、metadata 和颜色 |
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

## Shared UI line metrics

`text_layout_runs(&buffer)` exposes the same resolved UI baselines used by `TextContext`, while
retaining original glyph advances, line boxes and shaped-buffer identity. `text_hit(&buffer, x, y)`
uses those line boxes and direction-aware grapheme positions for editing. Both require an already
shaped buffer. See [the line-metric contract](https://github.com/Jianqoq/tileink/blob/main/docs/native/ui-text-metrics.md).

## 自定义原生换行布局

`text_layout_runs_from(&buffer, layouts)` 复用统一的 UI 基线、滚动与可见性规则，借用调用方准备好的
段落 `LayoutLine`。回调返回 `None` 时沿用原缓存，返回空切片时省略该段落。逻辑文字、段落方向、
字形的原文 byte index 和字体库身份保持不变；它不会复制或重新 shaping 源 buffer。

把同一行视图传给 `TextContext::layout_runs_filtered` 与 `text_hit_runs`，即可让绘制和鼠标命中使用
同一份布局。默认 buffer 接口也委托给这条共用路径。这个接口用于接入原生换行策略，本身不执行
均衡换行，也不修改字体栅格化、颜色混合或 GPU 采样规则。


`text_layout_runs_by(&buffer, |paragraph, row| ...)` resolves individual borrowed `LayoutLine`
values. `None` ends that paragraph; an empty line remains a real row. A native policy may combine
an owned short prefix with original cached tail rows without copying their glyphs. The array
and ordinary buffer views delegate to this same baseline/visibility path.

## 已排版的 span 绘制

`TextLayoutRun` 提供 `layout`、`line`、`metadata`、`color`。`line` 为视觉行序号，空段落也计数；
`color` 是可选的非预乘 sRGB RGBA8，None 使用调用方的默认画刷。只有同一行、相邻且颜色与 metadata
一致的字形才合并，绘制顺序不变。先按同一字体库准备 buffer，再调用 `layout_buffer_runs`；提取不会
修改或重新排版 buffer。应用可以为每段选择画刷、裁切行或更改交互颜色，复用同一份字形数据。


### 筛选已准备的字形

`TextContext::layout_buffer_runs_filtered` 使用原有排版和字形位置，回调会收到原始字形及
视觉行编号（包含空段落）。空绘制组会被省略，其余组的元数据、颜色、基线及顺序保持不变。
普通 `layout_buffer_runs` 仍包含全部字形；两个接口都不会重新排版或修改源缓冲区。

这是块级行截断的共享基础：单纯裁剪像素会切断连字、组合重音或斜体轮廓。调用方先按字形簇
决定保留范围，完整源文本仍可用于选择、复制以及后续的焦点滚动。
