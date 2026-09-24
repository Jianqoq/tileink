---
title: NativeRenderer API
---

# NativeRenderer API

使用 `NativeRenderer::new(NativeBackend::Dx12, width, height)` 或 `NativeRenderer::with_context(&context, width, height)` 构造。Immediate 方法包括 `render`、`render_with_text`、`render_to_image`、`render_to_texture`、`render_to_target`。Retained 对应方法以 `render_retained` 开头。图像提交的 `readback()` 等待并返回 `Image`；GPU 提交返回同步凭据。`insert_image`、`remove_image`、`clear_images` 管理图像资源；`set_clear_color` 和 `invalidate_retained_history` 更新渲染状态。
