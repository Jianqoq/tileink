---
title: Debug 与增量统计
---

# Debug 与增量统计

`NativeRenderer::incremental_render_config` 和 `incremental_render_stats` 提供 retained 重绘决策与计数。`set_incremental_render_config` 可调整策略；外部状态使输出历史失效时，调用 `invalidate_retained_history`。排查设备或 shader 问题时，启用 API validation，并在 release 模式单线程运行测试。
