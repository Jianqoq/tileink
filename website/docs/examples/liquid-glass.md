---
sidebar_position: 3
title: Liquid Glass
---

# Liquid Glass 示例

```rust
use peniko::{Color, kurbo::Rect};
use tileink::{BlurSampling, Canvas, Filter, Radius, RectLiquidGlass, Region};

let bounds = Rect::new(40.0, 40.0, 340.0, 260.0);
canvas.push_backdrop_layer(
    Filter::RectLiquidGlass(RectLiquidGlass {
        blur_radius: 5,
        blur_sampling: BlurSampling::downsampled(4),
        blur_edge: true,
        tint: Color::from_rgba8(255, 255, 255, 20),
        refraction_thickness: 28.0,
        refraction_factor: 2.5,
        refraction_dispersion: 10.0,
        glare_factor: 100.0,
        ..Default::default()
    }),
    Region::rect(bounds, Radius::all(34.0)),
);
canvas.push_rect(bounds, Radius::all(34.0), Color::from_rgba8(255, 255, 255, 26));
canvas.pop_layer();
```

Backdrop 依赖之前已经绘制的内容，因此 painter order 是语义的一部分。Retained 模式移动 glass node 时会损伤旧/新 bounds 和受影响的 backdrop dependency tiles。
