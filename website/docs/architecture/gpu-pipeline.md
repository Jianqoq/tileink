---
title: GPU pipeline
---

# GPU pipeline

各原生 API 从同一场景模型准备路径、画刷、layer、文本和图片。Path scan 与 prefix 计算工作量，coarse binning 将绘制分配到 16×16 tile，fine raster 计算 coverage 与混合。Filter pass 按需使用中间 surface。Retained 帧复用未变化的记录与输出历史。通过 `NativeImageSubmission::readback` 显式读回图像；host target 使用提交凭据协调呈现。


CPU tile binning 会先证明纯色不透明 SDF 矩形完整覆盖的 tile，在同一 raster batch 内跳过被它遮挡的 draw 引用，从而减少 GPU coarse 计数、粒子生成和 fine 绘制。圆角、抗锯齿边缘，以及 clip 未完全覆盖的 tile 会保留；不确定的画刷和变换交给原有 GPU 证明。CPU 空间索引保留全部源内容，确保遮挡移开、透明度变化和滤镜局部化后仍能恢复正确绘制。单 batch 的 retained 索引可以只更新 tile 的可见起点，避免透明度切换时重写完整索引。

Coarse 仍为每个 tile 记录最后一个已证明完全覆盖、alpha 为 255 的纯色粒子，供 Fine 跳过更早的粒子。这层证明保留粒子分配区间和 glyph 索引，补充 CPU 的保守剔除。融合的 opacity/blend wrapper、滤镜和 backdrop 依赖仍按原顺序执行。场景准备、路径 scan 和源数据上传仍然存在，因此普通无遮挡场景通常没有明显提速。Metal 和 HLSL 使用相同的 CPU tile 引用和执行边界。

融合栈由类型化的 clip、opacity、blend 记录编码，只有 clip 可以使用 SDF 几何；opacity 和 blend 的边界使用路径。每个 emitter 先对当前 tile 计算有效 wrapper 数量，排除无覆盖的路径或 SDF，并省去已证明覆盖整个 tile 的 clip。随后按嵌套顺序发射 begin，按相反顺序发射 end，复用同一组只读场景输入。Metal 和 HLSL 都遵循这一契约，发射阶段无需重复处理未知栈类型或已排除的路径。
