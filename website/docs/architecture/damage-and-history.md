---
sidebar_position: 5
title: Damage 与输出历史
---

# Damage 与输出历史

Retained raster 只有在目标像素历史可信时才能只更新 dirty tiles。

## Damage 来源

- node revision、transform、bounds、add/remove/reparent/reorder；
- `invalidate_rect` / `invalidate_all`；
- filter/backdrop dependency propagation；
- renderer state、image resource、device/pipeline 变化；
- surface size、scale、texture identity/format 变化。

## 输出模式

| `IncrementalOutputMode` | 含义 |
|---|---|
| `InternalHistory` | renderer-owned texture 保存历史，再复制到 transient output |
| `ExternalHistory` | caller 保证外部 texture 内容持续有效 |
| `DirectTransient` | 直接完整渲染 transient target，不维护 history |
| `RebuildHistory` | 从 direct 模式重新建立内部 history |

外部 texture 被重新创建或被其他代码修改时，必须换新的 `ExternalTextureHistoryId`。错误复用 ID 会让 renderer 把未知像素当作有效历史。

## ForceFull

`IncrementalRenderMode::ForceFull` 是正确性 oracle 和性能对照。它强制完整 raster，但 retained materialization、chunk reuse 和 dirty upload 仍按正常语义运行。
