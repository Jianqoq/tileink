# Retained scene API

`RetainedScene` is the stateful rendering path for applications that update a small part of a
large scene. The scene owns hierarchy, order, generations, and a bounded change journal; each
`WgpuRenderer` consumes that journal independently.

```rust
use std::sync::Arc;
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene, WgpuRenderer};

let root = RetainedNodeId::for_owner(1);
let item = RetainedNodeId::for_owner(2);
let mut scene = RetainedScene::new(800, 600, 1.0, root)?;

let mut item_canvas = Canvas::new(800, 600, 1.0);
item_canvas.push_rect(
    Rect::new(20.0, 20.0, 120.0, 80.0),
    Radius::ZERO,
    Color::from_rgb8(30, 120, 220),
);
scene
    .transaction()
    .insert_scene(
        RetainedParent::content(root),
        None,
        item,
        Arc::new(item_canvas),
        (0.0, 0.0),
    )
    .commit()?;

let mut renderer = WgpuRenderer::new_default_device(800, 600, Color::TRANSPARENT);
renderer.render_retained(&scene);

scene
    .transaction()
    .set_position(item, (16.0, 0.0))
    .invalidate_rect(Rect::new(0.0, 0.0, 160.0, 100.0))
    .commit()?;
renderer.render_retained(&scene);
# Ok::<(), Box<dyn std::error::Error>>(())
```

A transaction validates every mutation before it becomes visible. Duplicate or missing IDs,
cycles, invalid mask branches, non-finite coordinates, scale mismatches, and unclosed Canvas
layers fail atomically. Content replacement and movement advance internal generations; callers do
not maintain revisions.

Use `insert_group`, `insert_layer`, `reparent`, `move_before`, and `remove_subtree` for structural
updates. `RetainedParent::content` and `RetainedParent::mask` select independent mask branches.
`resize`, `invalidate_rect`, and `invalidate_all` are committed through the same journal.

The renderer retains stable record/blob allocations, tile pages, batch membership, plan fragments,
resources, and compatible output history. A renderer that falls more than 256 commits behind does
one full synchronization and resumes incremental consumption on its next frame. ForceFull remains
available through `IncrementalRenderConfig` as a correctness oracle; it changes raster damage, not
scene materialization semantics.

`RetainedScene` is the only retained API. `Canvas` records immediate leaf content and cannot own
retained identity, revisions, or retained child snapshots. Keeping scene mutations in the
transaction journal is what makes update cost proportional to actual changes and prevents a second
metadata-diff/materialization implementation from diverging from the persistent backend.

WGPU output methods mirror the ordinary Canvas API:

- `render_retained` and `render_retained_with_text`
- `render_retained_to_wgpu_texture`
- `render_retained_to_persistent_wgpu_texture`
- text variants of both texture methods
- `render_retained_profiled` for stage timings and incremental counters

See [BENCHMARKS.md](BENCHMARKS.md) for the scale/dirty-ratio matrix, profiler counters, and baseline
commands.
