---
sidebar_position: 4
title: Retained Scene API
---

# Retained Scene API

Create identity with `RetainedNodeId::new(owner, slot)` or `for_owner(owner)`. Select branches with `RetainedParent::content(node)` and `RetainedParent::mask(node)`. `SceneVersion::INITIAL` is zero and `get()` returns its `u64`.

`RetainedScene::new`, `version`, `root`, `logical_size`, `physical_size`, `scale_factor`, and `transaction` form the scene API.

For native viewports, use `RetainedScene::with_surface(CanvasSurface::from_physical(size, scale)?, root)`.
`surface()` returns the exact record and transaction `set_surface(surface)` changes it atomically,
including fractional logical dimensions. An empty physical extent is rejected; scale must remain
compatible with live child canvases. `logical_size()` returns `(f64, f64)`.

Transaction methods chain and mutate only on `commit()`:

Groups and layers can own content children. A Canvas leaf created by `insert_scene` has no retained child branches and cannot be a parent. Only mask layers can own mask children. `before: Some(id)` must identify an existing sibling in the same branch, including when that branch is empty; `None` appends. `move_before(id, id)` is a no-op only for an existing node. Invalid-branch errors identify the requested parent.

| Method | Purpose |
|---|---|
| `insert_scene(parent, before, id, Rc<Canvas>, Affine)` | Insert a leaf |
| `insert_group` / `insert_layer` | Insert hierarchy or visual boundary |
| `replace_scene` | Replace content and advance generation |
| `set_transform` | Full affine placement |
| `update_layer` | Change layer descriptor |
| `reparent` / `move_before` | Hierarchy/order changes |
| `remove_subtree` | Remove a node and descendants |
| `resize` | Change surface extent/scale |
| `invalidate_rect` / `invalidate_all` | Raster-only invalidation |
| `commit` | Validate and atomically return the new version |

`RetainedLayerDescriptor` covers ClipPath, ClipSdf, Isolate, Opacity, Blend, Filter, Backdrop, and Mask. Only mask layers accept mask-branch children.

Validation errors include duplicate/missing nodes, invalid parent/sibling/branch, cycles, root removal, scale mismatch, unclosed Canvas, invalid position/transform, and invalid size. Failed commits leave no partial state.
