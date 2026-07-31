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

## Delta snapshot

持久 frame snapshot 可以共享不可变 node 数组，并在 delta overlay 中记录删除和更新。当 renderer 无法直接消费连续 journal delta、必须比较两个 snapshot 时，必须先解析 overlay，再按 node ID 比较。因此，一个被 delta 删除后又以相同 ID 插入的节点仍然产生插入 damage，即使旧 base array 中还保留该 ID。只存在于 overlay 的节点和显式 delta damage 采用局部保守 damage，不升级为全屏重绘。Fallback diff 保持连续 base array 扫描，只对 delta overlay 和 compacted state page 中实际变化的 ID 做稀疏解析；禁止给每个 base node 增加 overlay hash lookup。scratch storage 跨 frame 复用，平均复杂度为 `O(N + P)`，额外 scratch 空间为 `O(N + P)`；其中每个被比较 frame 的 base-index 标记为每节点一字节。

## ForceFull

`IncrementalRenderMode::ForceFull` 是正确性 oracle 和性能对照。它强制完整 raster，但 retained materialization、chunk reuse 和 dirty upload 仍按正常语义运行。
