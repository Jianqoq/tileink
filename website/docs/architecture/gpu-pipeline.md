---
title: GPU pipeline
---

# GPU pipeline

各原生 API 从同一场景模型准备路径、画刷、layer、文本和图片。Path scan 与 prefix 计算工作量，coarse binning 将绘制分配到 16×16 tile，fine raster 计算 coverage 与混合。Filter pass 按需使用中间 surface。Retained 帧复用未变化的记录与输出历史。通过 `NativeImageSubmission::readback` 显式读回图像；host target 使用提交凭据协调呈现。
