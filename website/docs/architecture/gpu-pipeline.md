---
title: GPU pipeline
---

# GPU pipeline

各原生 API 从同一场景模型准备路径、画刷、layer、文本和图片。Path scan 与 prefix 计算工作量，coarse binning 将绘制分配到 16×16 tile，fine raster 计算 coverage 与混合。Filter pass 按需使用中间 surface。Retained 帧复用未变化的记录与输出历史。通过 `NativeImageSubmission::readback` 显式读回图像；host target 使用提交凭据协调呈现。


Coarse 会为每个 tile 记录最后一个已证明完全覆盖、alpha 为 255 的纯色粒子。Fine 可以跳过它之前的绘制，减少不透明面板覆盖下的光栅化工作。该优化保留粒子分配区间和 glyph 索引，适用于 retained 区间复用；部分 clip、opacity/blend group、滤镜和 backdrop 的依赖仍按原顺序执行。它减少 fine 的工作量，未省去场景准备、上传和 coarse 分配，因此普通无遮挡场景通常没有明显提速。
