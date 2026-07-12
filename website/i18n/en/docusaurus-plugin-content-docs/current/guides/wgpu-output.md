---
sidebar_position: 1
title: WGPU output
---

# WGPU output

Use `WgpuRenderer::new_default_device` for tools and tests, or `new`/`new_with_options` with an application's device and queue. Dimensions are physical pixels and should match `Canvas::physical_size()`.

Transient outputs use `render_to_wgpu_texture` or `render_retained_to_wgpu_texture`. If the caller preserves a texture's pixels, use the corresponding `*_to_persistent_wgpu_texture` method and an `ExternalTextureHistoryId`. Advance that ID whenever the texture is recreated or modified externally.

Targets must be 2D `Rgba8Unorm`, sample count 1, large enough, and carry required storage/copy usages. For swapchains, either use transient output or render into a persistent offscreen texture and blit to the current surface image.
