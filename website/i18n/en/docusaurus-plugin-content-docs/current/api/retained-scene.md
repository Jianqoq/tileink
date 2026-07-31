---
sidebar_position: 4
title: Retained Scene API
---

# Retained Scene API

Create identity with `RetainedNodeId::new(owner, slot)` or `for_owner(owner)`. Select branches with `RetainedParent::content(node)` and `RetainedParent::mask(node)`. `SceneVersion::INITIAL` is zero and `get()` returns its `u64`.

`RetainedScene::new`, `version`, `root`, `logical_size`, `physical_size`, `scale_factor`, and `transaction` form the scene API.

Transaction methods chain and mutate only on `commit()`:

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
