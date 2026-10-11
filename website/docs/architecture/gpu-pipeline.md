---
title: GPU pipeline
---

# GPU pipeline

各原生 API 从同一场景模型准备路径、画刷、layer、文本和图片。Path scan 与 prefix 计算工作量，coarse binning 将绘制分配到 16×16 tile，fine raster 计算 coverage 与混合。Filter pass 按需使用中间 surface。Retained 帧复用未变化的记录与输出历史。通过 `NativeImageSubmission::readback` 显式读回图像；host target 使用提交凭据协调呈现。


Coarse 会为每个 tile 记录最后一个已证明完全覆盖、alpha 为 255 的纯色粒子。Fine 可以跳过它之前的绘制，减少不透明面板覆盖下的光栅化工作。该优化保留粒子分配区间和 glyph 索引，适用于 retained 区间复用；部分 clip、opacity/blend group、滤镜和 backdrop 的依赖仍按原顺序执行。它减少 fine 的工作量，未省去场景准备、上传和 coarse 分配，因此普通无遮挡场景通常没有明显提速。

融合栈由类型化的 clip、opacity、blend 记录编码，只有 clip 可以使用 SDF 几何；opacity 和 blend 的边界使用路径。每个 emitter 先对当前 tile 计算有效 wrapper 数量，排除无覆盖的路径或 SDF，并省去已证明覆盖整个 tile 的 clip。随后按嵌套顺序发射 begin，按相反顺序发射 end，复用同一组只读场景输入。Metal 和 HLSL 都遵循这一契约，发射阶段无需重复处理未知栈类型或已排除的路径。
