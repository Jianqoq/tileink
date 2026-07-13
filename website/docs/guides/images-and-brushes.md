---
sidebar_position: 4
title: 图片与 Brush
---

# 图片与 Brush

## Image

```rust
let image = tileink::Image::from_rgba8(width, height, rgba_bytes);
renderer.insert_image(tileink::ImageKey::new(42), image);
```

`Image` 内部保存 premultiplied RGBA8。可用 `rgba8_at`/`rgba8_bytes` 读回，`save` 输出 PNG。

## Direct image draw

`Canvas::push_image` 把 `Rc<Image>` 直接记录进 scene；`push_image_key` 引用 renderer registry 中的 `ImageKey`。Key 方式更适合多个 retained nodes 共用大图和独立更新资源。

## Brush

接受 `impl Into<Brush>` 的 API 可以直接传 `peniko::Color`。其他 brush：

- `Brush::Linear` / `Radial` / `Sweep` / `FourCorner`；
- `Brush::Pattern(PatternBrush)`；
- `PatternBrush::from_image_key*` 引用 image registry；
- `Brush::from_gradient*` 从 peniko gradient 生成采样 ramp。

Pattern sampling 由 `PatternSampling` 控制；自然尺寸、显式 source/destination bounds 和 affine transform 应按所用 constructor 选择。

资源更新后 renderer 会失效相关 retained history；无需手动重建无关 scene chunks。
