---
title: NativeRenderer API
---

# NativeRenderer API

Construct with `NativeRenderer::new(NativeBackend::Dx12, width, height)` or `NativeRenderer::with_context(&context, width, height)`. Immediate methods include `render`, `render_with_text`, `render_to_image`, `render_to_texture`, and `render_to_target`. Retained equivalents begin with `render_retained`. Image methods return a submission whose `readback()` waits and produces an `Image`. GPU-only submissions return a receipt for explicit synchronization. `insert_image`, `remove_image`, and `clear_images` manage image resources. `set_clear_color` and `invalidate_retained_history` update renderer state.
