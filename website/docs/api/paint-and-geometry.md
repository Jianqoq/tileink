---
sidebar_position: 5
title: Paint、Geometry 与 Filter
---

# Paint、Geometry 与 Filter

## Brush

`Brush` variants：solid、linear、radial、sweep、four-corner 与 pattern。常用 constructors：

| 方法 | 说明 |
|---|---|
| `Brush::from_image_key(...)` | image-key pattern，显式目标 rect 与 sampling |
| `Brush::from_image_key_with_options(...)` | 再指定 extend、sampling 与 opacity |
| `Brush::from_image_key_natural(...)` | 按图片自然尺寸从指定 origin 绘制 |
| `PatternBrush::for_origin_resource(...)` | 构造 1:1 canvas-pixel pattern；零尺寸返回 `None` |
| `Brush::from_gradient(&Gradient)` | 默认 ramp size |
| `Brush::from_gradient_with_ramp_size(...)` | 显式 ramp resolution |
| `Brush::four_corner(bounds, [Color; 4])` | 四角插值 |
| `Brush::for_origin_resource(...)` | 调整 resource brush 到 local origin |

`DEFAULT_GRADIENT_RAMP_SIZE` 为 4096。`PatternSampling` 决定 nearest/linear 等采样策略。

## Bounds

`Bounds::new(x0, y0, x1, y1)` 创建整数像素范围，`Bounds::canvas(width, height)` 创建完整画布范围。`intersect`、`union` 与 `outset` 分别计算交集、并集与向外扩张；`is_empty` 检查空范围。Bounds 使用半开区间，`x1`/`y1` 不属于范围。

## Image

| 方法 | 说明 |
|---|---|
| `Image::new(width, height, clear)` | 创建 premultiplied image |
| `from_rgba8(width, height, bytes)` | straight RGBA8 输入 |
| `from_premultiplied_rgba8(width, height, Vec<u32>)` | 已预乘输入 |
| `rgba8_at(x, y)` | straight RGBA8 pixel |
| `rgba8_bytes()` | 完整 straight RGBA8 copy |
| `save(path)` | PNG 输出 |
| `ImageKey::new(id)` | registry identity |

## SDF

`Sdf` 包含 Rect、Circle、RectStroke、CircleStroke、CandleStick、Line、DashLine、Arc；`SdfShadow` 包含相应 shadow shapes。两者都有 `bounds()`。

| 类型/constructor | 说明 |
|---|---|
| `Radius::all(r)` | 四角相同半径 |
| `StrokeWidths::all(w)` | 四边相同宽度 |
| `ShadowOptions::new(dx, dy, expand, intensity)` | analytic shadow 参数 |
| `SdfLine::new(start, end, width, cap)` | line |
| `SdfDashLine::new(...)` / `with_offset(...)` | dashed line |
| `SdfArc::new(...)` | arc geometry |
| `SdfTriangle::new(a, b, c, corner_radius)` | 统一圆角的解析式三角形 |
| `CandleStick::new(...)` | candlestick；width 可用 validation helpers 检查 |

Circle/Rect/stroke/shadow structs 同时公开字段，适合 struct literal；详细字段运行 `cargo doc --open` 查看。

## Region 与 Mask

`Region::rect(rect, radius)` 创建 analytic rounded-rect region；`Region::path(path, transform, tolerance)` 创建 path region。`Mask` 包含 region 和 `MaskKind`（alpha/luminance 语义）。

## Filter

`Filter` 覆盖 blur、color transforms、drop shadow、opacity、component transfer、convolution、morphology、displacement、turbulence、diffuse/specular lighting、composite、blend、RectLiquidGlass 和 filter primitive graph。

辅助类型包括：

- `BlurSampling::downsampled(factor)`、`BlurDownsampleFilter`、`BlurUpsampleFilter`；
- `FilterPrimitive` / `FilterInput` / `FilterPrimitiveKind`；
- `CompositeOperator`、`MorphologyOperator`、`ConvolveMatrix` / `ConvolveEdgeMode`；
- `DiffuseLighting`、`SpecularLighting`、`LightSource`；
- `RectLiquidGlass`。

Filter region 不只是裁剪：它还决定 dependency bounds、offscreen allocation 与 damage propagation，应该尽量紧致但不能遗漏采样范围。
