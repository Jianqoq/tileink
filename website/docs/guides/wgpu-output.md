---
sidebar_position: 1
title: WGPU 输出
---

# WGPU 输出

## 让 Tileink 创建默认设备

`WgpuRenderer::new_default_device` 适合测试、离屏工具和快速原型：

```rust
let mut renderer = tileink::WgpuRenderer::new_default_device(
    width,
    height,
    peniko::Color::TRANSPARENT,
);
renderer.render(&canvas);
let image = renderer.image();
```

## 使用应用已有的 device/queue

```rust
let mut renderer = tileink::WgpuRenderer::new_with_options(
    &device,
    &queue,
    width,
    height,
    peniko::Color::TRANSPARENT,
    tileink::WgpuRendererOptions::default(),
);
```

Device、Queue 必须比 renderer 活得更久。目标尺寸是 physical pixels，应与 Canvas 的 `physical_size()` 一致。

## 输出到 texture

一次性目标：

```rust
renderer.render_to_wgpu_texture(&canvas, &texture)?;
```

可保留像素的目标：

```rust
let history = tileink::ExternalTextureHistoryId::new(texture_generation);
renderer.render_retained_to_persistent_wgpu_texture(&scene, &texture, history)?;
```

Texture 必须是 `Rgba8Unorm`、2D、sample count 1，并包含所需 usages。具体错误由 `WgpuTextureRenderError` 返回。

## Swapchain 注意事项

普通 swapchain image 每帧不同，不能自然作为 persistent history。使用 transient 方法，或先渲染到应用持有的 offscreen texture 再 blit 到 surface。resize 后重新创建 offscreen texture并推进 history ID。
