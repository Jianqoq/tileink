# Retained scene API

`RetainedScene` is the stateful rendering path for applications that update a small part of a
large scene. The scene owns hierarchy, order, generations, and a bounded change journal; each
`WgpuRenderer` consumes that journal independently.

```rust
use std::rc::Rc;
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
        Rc::new(item_canvas),
        peniko::kurbo::Affine::IDENTITY,
    )
    .commit()?;

let mut renderer = WgpuRenderer::new_default_device(800, 600, Color::TRANSPARENT);
renderer.render_retained(&scene);

scene
    .transaction()
    .set_transform(item, peniko::kurbo::Affine::translate((16.0, 0.0)))
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

## Encoded geometry and scoped damage invariants

Each materialized chunk owns its encoded Canvas. Its cached visual bounds describe
that Canvas's unclipped output, before ancestor influences or final root clipping.
Damage collection and frame construction reuse this result. `ChunkCanvas` exposes
read-only Canvas access; every mutable access uses `edit()`, which clears the cache
before returning the mutable reference. Content encoding, retained transforms and
surface resizing all follow this rule. New chunks start with an empty cache. This
removes repeated geometry traversal at its source rather than skipping required
old or new damage extents.

Scoped damage propagation uses one work buffer with a separate active range for
each isolated input texture. Every child input is seeded with the same pending
unattributed bounds and accumulates its own local sources; it does not inherit
the work range accumulated by its parent or a mask sibling.
Completed child output merges into its parent in painter order with the same
exact-bound deduplication. Intermediate off-canvas extents remain available to
enclosing filters; clipping occurs only when delivering root damage. This shares
temporary storage without changing input-domain or dependency semantics.

Shared damage-history steps own exact-length bounds arrays. Root clipping must
not cause a retained event to keep a large, mostly empty work-buffer allocation.
Resolving an immediate step borrows its payload; resolving multiple steps produces
an owned merge. A caller that needs an independent snapshot explicitly takes an
owned copy. Backdrop ID arrays remain shared with renderer plans.

The permanent `scoped_damage` Criterion workload covers root layer changes,
isolated layer changes and leaf revisions at small and large backdrop counts.
Performance conclusions require formal forward/reverse comparisons and adjacent
same-version controls, followed by scoped and full-corpus pixel checks. Short
diagnostic probes cannot accept a production change.

## Uniform batch identity and lookup

Uniform arenas are keyed by the underlying resource allocation, including aliases
through cloned handles. Stage names cannot identify allocations from different
deferred renderers. Each batch keeps its arenas in a dense vector and uses a
bounded linear search for up to 16 resources; larger batches build a resource-to-
position index once and extend it on insertion. Indices are numeric positions,
so vector reallocation cannot invalidate them. This avoids unconditional hashing
for tiny batches without restoring quadratic lookup for large batches.

The adapter copies all pending bytes before clearing the batch. Clearing drops
each arena's byte buffer and removes index entries; only the outer arena vector
and index retain their capacities. A later batch must not resolve an old
resource to a reused position. Slot limits,
layout validation, independent allocation identities, zero padding and upload
contents remain identical on either side of the lookup boundary. Capacity checks
and writes retain their existing separate operations.
