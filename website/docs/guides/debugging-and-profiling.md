---
title: 调试与性能分析
---

# 调试与性能分析

使用 `NativeContextOptions { validation: true, ..Default::default() }` 请求 API validation。渲染提交显式返回凭据；需要 CPU 图像时，调用 `render_to_image(...).readback()`。Retained 场景通过 `incremental_render_stats()` 检查 full redraw、dirty tiles、上传量和复用情况。评估性能改动时，固定物理设备、场景、尺寸和工具链，测量实际应用的整帧耗时，并在完成后移除临时测量代码。
