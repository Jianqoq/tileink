---
sidebar_position: 1
title: WGPU output
---

# WGPU output

Use `WgpuRenderer::new_default_device` for tools and tests, or `new`/`new_with_options` with an application's device and queue. Dimensions are physical pixels and should match `Canvas::physical_size()`.

Transient outputs use `render_to_wgpu_texture` or `render_retained_to_wgpu_texture`. If the caller preserves a texture's pixels, use the corresponding `*_to_persistent_wgpu_texture` method and an `ExternalTextureHistoryId`. Advance that ID whenever the texture is recreated or modified externally.

Targets must be 2D `Rgba8Unorm`, sample count 1, large enough, and include `STORAGE_BINDING`. The portable texture path also requires `COPY_SRC | COPY_DST`.

When rendering directly to an external target, root offscreen layers/masks need `COPY_SRC` to read the background; a root backdrop also needs `TEXTURE_BINDING`. Nested effects read their own scratch targets. If retained output renders into internal history and then copies the result, those effects read the internal texture and impose no additional destination read usages.

Missing usages return `WgpuTextureRenderError`. Scene preparation and resource uploads may already have occurred, but the error does not dispatch or copy into the target or commit that frame's output history. Required read usages are cached with prepared plan metadata, without recompilation or plan scans on static frames.

For swapchains, either use transient output or render into a persistent offscreen texture and blit to the current surface image.

Switching the same renderer from persistent external output to `render_retained` or `render_retained_with_text` regenerates internal output history and ensures its texture size, while reusing unchanged scene preparation.
