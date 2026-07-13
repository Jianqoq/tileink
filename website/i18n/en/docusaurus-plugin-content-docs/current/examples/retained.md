---
sidebar_position: 2
title: Retained example
---

# Retained Scene example

```rust
use std::rc::Rc;
use peniko::{Color, kurbo::{Affine, Rect}};
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene, WgpuRenderer};

let root = RetainedNodeId::for_owner(1);
let card = RetainedNodeId::for_owner(2);
let mut scene = RetainedScene::new(800, 480, 1.0, root)?;
let mut leaf = Canvas::new(240, 140, 1.0);
leaf.push_rect(Rect::new(0.0, 0.0, 240.0, 140.0), Radius::all(24.0), Color::from_rgb8(86, 76, 230));
scene.transaction().insert_scene(RetainedParent::content(root), None, card, Rc::new(leaf), Affine::translate((40.0, 60.0))).commit()?;
let mut renderer = WgpuRenderer::new_default_device(800, 480, Color::TRANSPARENT);
renderer.render_retained(&scene);
scene.transaction().set_transform(card, Affine::translate((360.0, 180.0)) * Affine::rotate(0.12)).commit()?;
renderer.render_retained(&scene);
```

Transform-only updates preserve local geometry and paint blobs. Scene versions and node generations are managed automatically.
