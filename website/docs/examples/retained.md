---
sidebar_position: 2
title: Retained 完整示例
---

# Retained Scene 示例

```rust
use std::sync::Arc;
use peniko::{Color, kurbo::{Affine, Rect}};
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene, WgpuRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = RetainedNodeId::for_owner(1);
    let card = RetainedNodeId::for_owner(2);
    let mut scene = RetainedScene::new(800, 480, 1.0, root)?;

    let mut card_canvas = Canvas::new(240, 140, 1.0);
    card_canvas.push_rect(
        Rect::new(0.0, 0.0, 240.0, 140.0),
        Radius::all(24.0),
        Color::from_rgb8(86, 76, 230),
    );
    let card_canvas = Arc::new(card_canvas);

    scene.transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            card,
            card_canvas,
            Affine::translate((40.0, 60.0)),
        )
        .commit()?;

    let mut renderer = WgpuRenderer::new_default_device(800, 480, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    // Transform-only update keeps local geometry/blobs and uploads only changed records.
    scene.transaction()
        .set_transform(card, Affine::translate((360.0, 180.0)) * Affine::rotate(0.12))
        .commit()?;
    renderer.render_retained(&scene);
    Ok(())
}
```

不需要为内容变化手动维护 revision。调用 `replace_scene`、`set_transform`、`update_layer` 等 mutation 后，scene 自动推进 generation 和 version。
