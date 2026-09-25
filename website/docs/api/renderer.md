---
title: NativeRenderer API
---

# NativeRenderer API

使用 `NativeRenderer::new(NativeBackend::Dx12, width, height)` 或 `NativeRenderer::with_context(&context, width, height)` 构造。Immediate 方法包括 `render`、`render_with_text`、`render_to_image`、`render_to_texture`、`render_to_target`。Retained 对应方法以 `render_retained` 开头。图像提交的 `readback()` 等待并返回 `Image`；GPU 提交返回同步凭据。`insert_image`、`remove_image`、`clear_images` 管理图像资源；`set_clear_color` 和 `invalidate_retained_history` 更新渲染状态。

Metal 后端要求 Apple7 或更新的 Apple GPU、Tier 2 argument buffers 和至少 256 线程的 compute threadgroup。最终绘制使用硬件 TBDR render pass：覆盖完整目标的 fine pass 由 tile shader 直接读写片上 imageblock，局部裁剪和增量绘制只栅格化活动 tile，并通过 framebuffer fetch 读取目标颜色。路径覆盖、裁剪、文本和混合保持原有解析语义，几何准备和邻域滤镜继续使用 compute。

Metal 支持局部裁剪调度：保守地选择裁剪范围内的 tile；无文本的纯裁剪计划复用预分配粒子空间，减少重复计数、前缀扫描和整屏绘制。增量更新仍考虑移除或重设父级造成的损伤区域，混合分组和文本保留常规分配路径。

Metal compute dispatch、纹理拷贝和绘制保留独立的 encoder 边界，保证裁剪发射与绘制之间的资源依赖。

宿主导入的纹理若作为绘制目标，必须同时带有 `ShaderRead | ShaderWrite | RenderTarget` usage。增量更新、背景混合及大于视口的目标会加载原有内容，保留未覆盖的像素；整目标替换无需加载旧颜色。
