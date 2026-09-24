---
title: 渐进模糊
---

`ProgressiveBlur` 让模糊强度随位置变化。起点前保持清晰，起终点之间平滑增强，终点后保持最大模糊。

```rust
use peniko::kurbo::{Point, Rect};
use tileink::{Filter, ProgressiveBlur, Radius, Region};

canvas.push_backdrop_layer(
    Filter::ProgressiveBlur(ProgressiveBlur::new(
        Point::new(0.0, 100.0),
        Point::new(0.0, 260.0),
        24.0,
    )),
    Region::rect(Rect::new(0.0, 0.0, 640.0, 360.0), Radius::ZERO),
);
canvas.pop_layer();
```

先绘制背景，再添加 backdrop layer。要模糊图层本身的内容，使用 `push_filter_layer`。
位置和标准差均使用逻辑像素，并随 canvas 的缩放比例变化。交换两个点可反转方向；两个点相同时整个区域采用最大模糊。最大标准差为零时原图不变。

该效果采用 GPU 多尺度近似，细节随位置逐渐消失。它不等于清晰图与一张固定模糊图的透明度混合，也不保证与精确的可变高斯卷积完全一致。透明边缘按预乘 alpha 处理。
参数必须有限，标准差不得为负；设备像素下的标准差上限为 65536。

运行 `cargo run --release --example progressive_blur`，可生成 `target/progressive-blur.png` 示例。
