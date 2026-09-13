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

Texture 必须是 `Rgba8Unorm`、2D、sample count 1，尺寸足够且包含 `STORAGE_BINDING`。Portable 纹理路径还要求 `COPY_SRC | COPY_DST`。

直接向外部目标绘制时，根级 offscreen layer/mask 需要 `COPY_SRC` 读取背景，根级 backdrop 还需要 `TEXTURE_BINDING`。嵌套效果读取自己的 scratch 目标，不额外要求外部目标提供这些权限。Retained 选择内部 history 后再拷贝输出时，根级效果同样读取内部纹理，因此外部目标不因这些效果额外需要读权限。

权限不足通过 `WgpuTextureRenderError` 返回。场景准备和资源上传可能已经发生，但权限错误不会向目标执行绘制或拷贝，也不会提交该帧的输出历史。要求的读权限随准备好的执行计划缓存，静态帧不会为了校验重新编译或遍历计划。

## Swapchain 注意事项

普通 swapchain image 每帧不同，不能自然作为 persistent history。使用 transient 方法，或先渲染到应用持有的 offscreen texture 再 blit 到 surface。resize 后重新创建 offscreen texture并推进 history ID。

同一个 Renderer 从 persistent 外部目标切回 `render_retained` 或 `render_retained_with_text` 时，会重新建立内部输出历史并按需调整内部纹理尺寸；未变化的场景准备仍可复用。
