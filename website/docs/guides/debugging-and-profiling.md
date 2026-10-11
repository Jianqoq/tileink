---
title: 调试与性能分析
---

# 调试与性能分析

使用 `NativeContextOptions { validation: true, ..Default::default() }` 请求 API validation。渲染提交显式返回凭据；需要 CPU 图像时，调用 `render_to_image(...).readback()`。Retained 场景通过 `incremental_render_stats()` 检查 full redraw、dirty tiles、上传量和复用情况。评估性能改动时，固定物理设备、场景、尺寸和工具链，测量实际应用的整帧耗时，并在完成后移除临时测量代码。

`IncrementalRenderConfig` 提供 `mode`、`full_redraw_ratio`、`capture_active_tiles`
和 `retained_texture_budget_bytes`。重绘比例接受 `(0, 1]` 内的有限值；
原生 retained 纹理缓存默认预算为 256 MiB。启用 `capture_active_tiles`
可在统计中采集实际 active tile ID 和范围。
