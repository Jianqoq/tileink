---
sidebar_position: 7
title: Debug 与 Profile API
---

# Debug capture

`RenderOptions` 包含普通 render 配置与可选 `RenderDebugOptions`。Debug options：

| 方法 | 说明 |
|---|---|
| `RenderDebugOptions::new(output_dir)` | 创建目录，失败时 panic |
| `try_new(output_dir)` | 返回 `io::Result` |
| `with_tile((x, y))` | 只深挖一个 tile |
| `with_tile_overlay(grid_color, text_color)` | 输出 overlay |
| `output_dir()` / `tile()` / `tile_overlay()` | getters |
| `debug_capture_json(capture)` | 稳定 JSON 文本 |

Capture 类型：`RenderDebugCapture`、`RenderDebugText`、`RenderDebugImage`、`DebugTileSummary`、`DebugTilePathSummary`、`DebugTileDump`、`DebugTilePath`、`DebugLineSegment`。

# Profile

`WgpuRenderProfileEntry` 同时容纳 optional CPU/GPU duration；GPU readback 完成前 duration 可能尚不可用。

| API | 说明 |
|---|---|
| `entries()` | 原始 stage entries |
| `cpu_time()` / `gpu_time()` | 整帧合计 |
| `incremental_stats()` | profile 对应的 retained counters |
| `summary()` | 按 stage name 聚合 |

`WgpuRenderProfileReport::new/push/iterations/profile` 用于跨多帧累计报告。

# Incremental stats

`IncrementalRenderStats` 公开以下类别字段：

- redraw：`full_redraw`、`full_redraw_reason`、dirty/changed tiles 与 ratios；
- output：`output_mode`、history copy、queue submissions；
- scene：retained nodes、draw batches、surface reuse/rerender；
- scan：paths、lines、chunks；
- filters：dispatch 与 compact dispatch；
- materialization：chunks/plan fragments/full sync；
- bytes/pages/arenas：CPU copied、GPU uploaded、tile pages、live/capacity/fragmentation/compactions。

`FullRedrawReason` 精确说明 FirstFrame、SurfaceChanged、RendererStateChanged、ExplicitInvalidation、DirtyTileThreshold、Forced 等原因。不要只看 `full_redraw` bool 猜测性能行为。
