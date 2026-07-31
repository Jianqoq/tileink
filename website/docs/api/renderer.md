---
sidebar_position: 3
title: WgpuRenderer API
---

# `WgpuRenderer` / `Renderer`

两个名字是同一类型。Renderer 持有 device/queue clone、pipelines、GPU buffers、image registry、retained cursors、history 和 profiler。

## 构造

```rust
pub fn new(device: &wgpu::Device, queue: &wgpu::Queue,
           width: u32, height: u32, clear: Color) -> Self;
pub fn new_with_options(device: &wgpu::Device, queue: &wgpu::Queue,
                        width: u32, height: u32, clear: Color,
                        options: RendererOptions) -> Self;
pub fn new_default_device(width: u32, height: u32, clear: Color) -> Self;
```

`RendererOptions::pipeline_cache` 接收由相同 device 创建的 WGPU pipeline cache。应用负责磁盘持久化。

## 基本 render

| 方法 | 说明 |
|---|---|
| `render(&Canvas)` | 普通 Canvas 到 renderer-owned target |
| `render_native(&Canvas) -> bool` | 显式 native backend，返回是否成功 |
| `render_with_text(...)` | 带 font/text context |
| `render_native_with_text(...) -> bool` | native text variant |
| `render_profiled(&Canvas)` | immediate render，并返回 CPU/GPU profile |
| `render_with_text_profiled(...)` | text-aware immediate render profile |
| `render_with_options(canvas, options)` | debug/capture options |
| `image()` | 同步读回 renderer target 为 `Image` |

## Retained render

| 方法 | 说明 |
|---|---|
| `render_retained(&RetainedScene)` | persistent scene |
| `render_retained_with_text(scene, fonts, text)` | text variant |
| `render_retained_profiled(scene)` | 自动 start/end profile |
| `render_retained_with_text_profiled(...)` | retained + text + profile |

## Texture output 对称矩阵

| Scene | Transient texture | Persistent external history |
|---|---|---|
| Canvas | `render_to_wgpu_texture` | `render_to_persistent_wgpu_texture` |
| Canvas + text | `render_with_text_to_wgpu_texture` | `render_with_text_to_persistent_wgpu_texture` |
| Retained | `render_retained_to_wgpu_texture` | `render_retained_to_persistent_wgpu_texture` |
| Retained + text | `render_retained_with_text_to_wgpu_texture` | `render_retained_with_text_to_persistent_wgpu_texture` |

全部 texture 方法返回 `Result<(), WgpuTextureRenderError>`。

## Image registry

| 方法 | 说明 |
|---|---|
| `insert_image(key, image) -> bool` | 新增/替换；是否改变 registry |
| `remove_image(key) -> bool` | 删除 |
| `clear_images() -> bool` | 清空 |
| `image_resource(key)` | 只读查询 |

资源变化会失效必要的 retained history 和 GPU tables。

## Incremental 配置与状态

| 方法 | 说明 |
|---|---|
| `incremental_render_config()` | 复制当前 config |
| `set_incremental_render_config(config)` | validate 并应用 |
| `incremental_render_stats()` | 最近一帧详细 counters |
| `invalidate_retained_history()` | 外部状态使历史不可信时调用 |
| `set_clear_color(color)` | 改 clear color，并触发必要失效 |

`IncrementalRenderConfig` 包含 `mode`、full-redraw ratio、direct-render hysteresis、active-tile capture 等字段。调用 `validate()` 会检查比例关系。

## Device 与 pipeline

| 方法 | 说明 |
|---|---|
| `device()` / `queue()` | renderer 使用的 WGPU handles |
| `target_rgba8_byte_len()` | 当前目标读回字节数 |
| `last_frame_used_native_gpu()` | 最近一次是否 native path |
| `pipeline_compilation_epoch()` | pipelines 新编译时推进，可用于持久 cache |
