---
title: NativeRenderer API
---

# NativeRenderer API

Construct with `NativeRenderer::new(NativeBackend::Dx12, width, height)` or `NativeRenderer::with_context(&context, width, height)`. Immediate methods include `render`, `render_with_text`, `render_to_image`, `render_to_texture`, and `render_to_target`. Retained equivalents begin with `render_retained`. Image methods return a submission whose `readback()` waits and produces an `Image`. GPU-only submissions return a receipt for explicit synchronization. `insert_image`, `remove_image`, and `clear_images` manage image resources. `set_clear_color` and `invalidate_retained_history` update renderer state.

Metal requires an Apple7 or newer Apple GPU, Tier 2 argument buffers, and at least 256 threads per compute threadgroup. Final drawing uses hardware TBDR render passes. Full-target fine passes dispatch tile shaders that read and write the on-chip imageblock. Clip-local and incremental draws rasterize only active tiles and fetch destination colors from the attachment. Analytic path coverage, clipping, text, and blending retain their shared semantics. Geometry preparation and neighborhood filters remain compute operations.

Metal also supports conservative clip-local tile selection. Non-text pure-clip plans reuse preallocated particle slots, avoiding repeated counting, prefix scans, and full-viewport drawing. Retained updates still account for damage from removal or reparenting; mixed group and text schedules retain regular allocation.

Metal compute dispatches, texture copies, and rendering retain separate encoder boundaries to preserve resource dependencies between clip emission and drawing.

Imported textures used as drawing targets require `ShaderRead | ShaderWrite | RenderTarget` usage. Sparse updates, background blending, and targets larger than the viewport load existing contents to preserve untouched pixels. Full target replacement can discard old colors.
