---
sidebar_position: 2
title: Canvas API
---

# `Canvas`

`Canvas` 记录 draw records、geometry、paint blobs、text runs 和 layer command tree。除读取方法外，大多数 API 都要求 `&mut self`。

## 创建与尺寸

| 方法 | 说明 |
|---|---|
| `Canvas::new(width, height, scale_factor)` | 创建 logical-size scene；尺寸和 scale 必须有效 |
| `scale_factor()` | 返回 logical→physical scale |
| `logical_size()` | `(logical_width, logical_height)` |
| `physical_size()` / `physical_width()` / `physical_height()` | scale 后的物理输出尺寸 |
| `is_closed_for_append()` | 所有 layer 是否已 pop，可否 append/retain |
| `reset()` | 清空 scene 数据并保留 Canvas 对象供复用 |
| `reset_for_surface(width, height, scale_factor)` | 校验正数有限 scale 后，清空内容、保留分配容量并切换到新的 logical surface；校验失败不改变 Canvas |

## Scene composition

```rust
pub fn append(&mut self, other: &Canvas, pos: impl Into<Point>);
pub fn append_transformed(&mut self, other: &Canvas, transform: Affine);
```

`append` 是 translation convenience；`append_transformed` 支持旋转、缩放、skew 和 translation。纯平移会自动使用 `append` 的单次合并路径，不创建中间 Canvas。Source Canvas 必须已关闭 layers 且 scale compatible。

## Draw identity 与更新

| 方法 | 说明 |
|---|---|
| `draw_count()` | 物理 draw record 数量 |
| `draw_id_at(index)` | 返回有效 `DrawId` |
| `DrawId::index()` | 转回 record index |
| `draw_brush(draw)` | 解码 draw brush |
| `set_draw_brush(draw, brush)` | 更新 paint，成功返回 `true` |
| `set_draw_color(draw, color)` | solid-color convenience |
| `draw_solid_color(draw)` | brush 为 solid 时返回颜色 |

`DrawId` 只在拥有它的 Canvas 当前内容中有效；`reset` 后不要复用旧 ID。

## Layer stack

每次 push layer 后必须 `pop_layer()`；返回的 `LayerKind` 可用于检查关闭顺序。

| 方法 | 语义 |
|---|---|
| `push_clip_layer(path, transform, rule, tolerance)` | path clip |
| `push_clip_sdf_rect_layer(rect, radius)` | rounded-rect analytic clip |
| `push_clip_sdf_circle_layer(circle)` | circle clip |
| `push_clip_sdf_arc_layer(arc)` | arc clip |
| `push_clip_sdf_line_layer(line)` | line clip |
| `push_clip_sdf_layer(sdf)` | 任意 `Sdf` clip |
| `push_clip_sdf_layer_transformed(sdf, affine)` | affine SDF clip；不可逆 transform 返回 `false` |
| `push_isolate_layer(path, transform, tolerance)` | 隔离 compositing group |
| `push_opacity_layer(path, transform, tolerance, opacity)` | group opacity |
| `push_blend_layer(path, transform, tolerance, mix, compose)` | peniko blend/compose |
| `push_mask_layer(mask_scene, mask)` | 独立 mask scene |
| `push_filter_layer(filter, region)` | 对 children 应用 filter |
| `push_backdrop_layer(filter, region)` | 对之前已绘制内容采样 |
| `pop_layer()` | 结束最近 layer |

## Analytic primitives

所有接受 `impl Into<Brush>` 的方法可传 `Color` 或 `Brush`。

| 方法 | 返回 | 说明 |
|---|---|---|
| `push_rect(rect, radius, brush)` | `DrawId` | fill rounded rect |
| `push_checkerboard(rect, cell_size, first, second)` | `Option<(DrawId, DrawId)>` | constant two-draw analytic checkerboard |
| `push_rect_stroke(rect, radius, kurbo::Stroke, brush)` | `Option<DrawId>` | uniform/dashed stroke |
| `push_rect_stroke_widths(rect, radius, widths, brush)` | `DrawId` | 四边独立宽度 |
| `push_rect_shadow(rect, radius, options, brush)` | `DrawId` | analytic rect shadow |
| `push_circle(circle, brush)` | `DrawId` | fill circle |
| `push_circle_stroke(circle, kurbo::Stroke, brush)` | `Option<DrawId>` | circle stroke |
| `push_circle_shadow(circle, options, brush)` | `DrawId` | circle shadow |
| `push_sdf_arc(arc, brush)` | `Option<DrawId>` | arc；无效 geometry 返回 `None` |
| `push_triangle(triangle, brush)` | `Option<DrawId>` | 解析式三角形，可选圆角顶点 |
| `push_arc_shadow(arc, options, brush)` | `Option<DrawId>` | arc shadow |
| `push_candlestick(candle, brush)` | `DrawId` | candlestick SDF |
| `push_line(line, brush)` | `Option<DrawId>` | solid analytic line |
| `push_dash_line(line, brush)` | `Option<DrawId>` | dashed line |
| `push_line_shadow(line, options, brush)` | `Option<DrawId>` | line shadow |

## Kurbo paths

```rust
pub fn push_path(
    &mut self,
    path: BezPath,
    brush: impl Into<Brush>,
    transform: Affine,
    fill_rule: FillRule,
    tolerance: f64,
);
```

`push_arc`/`push_stroke` 是 kurbo stroke helpers；它们把 path/stroke style flatten 到 Tileink path records。Tolerance 越小曲线越精确，line records 也越多。

## Images

| 方法 | 用途 |
|---|---|
| `push_image(rect, image, peniko::Extend, sampling)` | 直接把共享 Image 放入 scene，返回 `Option<DrawId>` |
| `push_image_key(rect, key, peniko::Extend, sampling)` | 引用 renderer image registry，返回 `Option<DrawId>` |

Image-key draw 必须在 renderer 中存在相同 `ImageKey`，否则无法采样预期资源。

## Text

| 方法 | 用途 |
|---|---|
| `push_text_layout(layout, origin, brush)` | 记录 glyph/run；用 `render_with_text*` |
| `push_text_layout_as_path(context, font_system, layout, origin, brush)` | 转为 path，不依赖 GPU text atlas |

## SVG extension methods

| 方法 | 说明 |
|---|---|
| `push_svg(&usvg::Tree)` | 默认 `SvgOptions`，事务式 append |
| `push_svg_with_options(tree, options)` | 自定义 transform/tolerance |
