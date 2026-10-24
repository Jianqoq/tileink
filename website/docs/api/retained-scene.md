---
sidebar_position: 4
title: Retained Scene API
---

# Retained Scene API

## Identity 与 parent

```rust
RetainedNodeId::new(owner: u64, slot: u32)
RetainedNodeId::for_owner(owner: u64)
RetainedParent::content(node)
RetainedParent::mask(node)
```

`owner` 通常映射应用 element/entity ID，`slot` 区分同一 owner 的多个独立内容。ID 在一个 scene 内必须唯一。

`SceneVersion::INITIAL` 是 0；`get()` 返回原始 `u64`。

## Scene

| 方法 | 说明 |
|---|---|
| `RetainedScene::new(width, height, scale, root_id)` | 创建 root group |
| `RetainedScene::with_surface(surface, root_id)` / `surface()` | 使用或读取精确的 `CanvasSurface`；空物理尺寸被拒绝 |
| `version()` | 当前 commit version |
| `root()` | root ID |
| `logical_size()` / `physical_size()` | scene extent |
| `scale_factor()` | logical→physical scale |
| `transaction()` | 独占 mutable transaction |

Render 接受 `&RetainedScene`，transaction 持有 `&mut RetainedScene`，类型系统保证渲染期间不能提交修改。

## Transaction mutations

所有 mutation 返回 `&mut Self`，可以链式调用；只有 `commit()` 改变 scene。

Group 和 Layer 可以拥有 Content 子节点；`insert_scene` 创建的 Canvas leaf 没有 retained 子分支，不能用作 parent。只有 mask layer 可以拥有 Mask 子节点。`before: Some(id)` 必须指向同一分支中已有的 sibling，即使该分支为空也会验证；`None` 表示追加。`move_before(id, id)` 仅对已有节点是 no-op。非法分支错误中的 ID 指向实际请求的 parent。

| 方法 | 语义与复杂度 |
|---|---|
| `insert_scene(parent, before, id, Rc<Canvas>, Affine)` | 插入 leaf；通常 O(log siblings) |
| `insert_group(parent, before, id)` | 插入 container |
| `insert_layer(parent, before, id, descriptor)` | 插入 layer boundary |
| `replace_scene(id, Rc<Canvas>)` | 自动推进 generation |
| `set_transform(id, Affine)` | GPU affine placement；支持 rotate/scale/skew |
| `update_layer(id, descriptor)` | 更新 layer semantics |
| `reparent(id, new_parent, before)` | 移动 subtree，拒绝 cycle |
| `move_before(id, sibling)` | 同 parent reorder |
| `remove_subtree(id)` | 删除 node 和 descendants |
| `resize(width, height, scale)` | 更新 surface；scale 必须与 live child Canvas 相容 |
| `set_surface(surface)` | 原子更新已校验的 viewport，保留小数逻辑尺寸与精确物理尺寸；scale 必须与 live child Canvas 相容 |
| `invalidate_rect(rect)` | raster-only damage |
| `invalidate_all()` | full raster damage，不强制重建未变 scene data |
| `commit()` | 原子验证、应用并返回新 `SceneVersion` |

## Layer descriptors

`RetainedLayerDescriptor` variants：

- `ClipPath { path, transform, rule, tolerance }`
- `ClipSdf { sdf, transform }`
- `Isolate { path, transform, tolerance }`
- `Opacity { path, transform, tolerance, opacity }`
- `Blend { path, transform, tolerance, mix, compose }`
- `Filter { filter, sample_region }`
- `Backdrop { filter, sample_region }`
- `Mask(Mask)`

只有 mask layer 接受 `RetainedParent::mask(layer_id)` children；其他 parent 的 Mask branch 会返回 `InvalidParentBranch`。

## Errors

`RetainedSceneError`：`DuplicateNode`、`MissingNode`、`InvalidParentBranch`、`InvalidSibling`、`Cycle`、`CannotRemoveRoot`、`ScaleMismatch`、`UnclosedCanvas`、`InvalidPosition`、`InvalidTransform`、`InvalidSize`。

失败 commit 会执行完整 rollback，不会留下部分 hierarchy/order/generation/surface 修改。

Scene placement 和 `ClipSdf` 的变换必须在当前 surface scale 下转换为 GPU `f32`
后仍然有限、可逆，且逆矩阵的系数也可表示。小但非零的缩放不会仅因行列式小于
固定 epsilon 而被拒绝；求逆使用 `f64` 中间值避免行列式上下溢。转换后变为奇异矩阵、
物理位移溢出或逆矩阵溢出时，commit 返回 `InvalidTransform` 并完整回滚。
同一规则覆盖插入、placement/layer 更新和 surface scale 修改。这修复了校验通过后
materializer 在动画小缩放上 panic 的根因。
