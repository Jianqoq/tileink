---
sidebar_position: 3
title: WgpuRenderer API
---

# `WgpuRenderer` / `Renderer`

Both names identify the same type.

## Construction

`new(device, queue, width, height, clear)`, `new_with_options(..., RendererOptions)`, and `new_default_device(width, height, clear)` cover application-owned and convenience devices. `RendererOptions::pipeline_cache` must originate from the same device.

## Rendering

Immediate calls are `render`, `render_native`, `render_with_text`, `render_native_with_text`, `render_profiled`, `render_with_text_profiled`, and `render_with_options`. The two `*_profiled` methods return a `WgpuRenderProfile`. Retained equivalents are `render_retained`, `render_retained_with_text`, `render_retained_profiled`, and `render_retained_with_text_profiled`.

The complete texture-output matrix is:

| Scene | Transient texture | Persistent external history |
|---|---|---|
| Canvas | `render_to_wgpu_texture` | `render_to_persistent_wgpu_texture` |
| Canvas + text | `render_with_text_to_wgpu_texture` | `render_with_text_to_persistent_wgpu_texture` |
| RetainedScene | `render_retained_to_wgpu_texture` | `render_retained_to_persistent_wgpu_texture` |
| RetainedScene + text | `render_retained_with_text_to_wgpu_texture` | `render_retained_with_text_to_persistent_wgpu_texture` |

All eight methods return `WgpuTextureRenderError` on size, format, or usage mismatch. Persistent variants accept an `ExternalTextureHistoryId`; reuse it only while the target's preserved contents still have the same identity.

## State and resources

`insert_image`, `remove_image`, `clear_images`, and `image_resource` manage `ImageKey` resources. `incremental_render_config`, `set_incremental_render_config`, `incremental_render_stats`, and `invalidate_retained_history` control retained behavior. `device`, `queue`, `image`, `target_rgba8_byte_len`, `set_clear_color`, `last_frame_used_native_gpu`, and `pipeline_compilation_epoch` expose renderer state.

Profiling uses `start_profile`, `end_profile`, `poll_profile`, `has_pending_profile_readbacks`, and `profile`.
