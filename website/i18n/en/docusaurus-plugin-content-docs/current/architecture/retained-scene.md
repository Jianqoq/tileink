---
sidebar_position: 3
title: Retained Scene
---

# Retained Scene

`RetainedScene` is a persistent transactional graph. Each renderer consumes its 256-commit change journal independently.

```mermaid
flowchart TB
  Tx[Transaction] --> Validate[Validate all mutations]
  Validate -->|error| Rollback[Atomic rollback]
  Validate -->|ok| Graph[Hierarchy + sparse order keys]
  Validate --> Journal[Change journal]
  Journal --> Chunk[Changed SceneChunks]
  Chunk --> Arena[Stable record/blob arenas]
  Journal --> Pages[Changed tile pages]
  Journal --> Fragments[Changed plan fragments]
  Arena --> GPU[Dirty-range upload]
  Pages --> GPU
  Fragments --> GPU
```

Scene leaves own an immutable `Rc<Canvas>` and a `kurbo::Affine`; groups provide hierarchy; layers provide clip, isolate, opacity, blend, filter, backdrop, or mask semantics. Transactions validate IDs, parents, cycles, mask branches, scale, closed canvases, transforms, and sizes before committing atomically.

Opacity/blend groups containing offscreen children apply to the composited subtree. Parameter
updates patch the actual fused stack or offscreen operation while preserving batch order and
membership, producing the same result as a fresh render. Empty groups retain updateable plan
entries; changing the layer kind recompiles the corresponding execution structure.

Full compilation also replaces leaf batch classifications and root fragment operation ranges.
A leaf spanning several batches retires its old single-batch classification, so later removal
and content updates use the current plan's membership and ranges. Plain leaf insertion reuses
only an execution batch adjacent to its actual painter position; it cannot cross clip or filter
boundaries. Neighbour lookup flattens groups, and empty layer content and mask branches retain
independent batches.

Each non-group node becomes a local-indexed `SceneChunk`. Stable arenas prevent unrelated variable-length updates from moving live data. Tile draw pages and plan fragments update only affected regions/ancestors. During continuous resize, Tileink preserves chunks, defers exact spatial metadata, uses dense one-frame tile bins, and restores the persistent reverse index on the first later incremental mutation. Responsive hierarchy changes committed with a resize also preserve the previous immutable frame/spatial pages because the full redraw makes them unobservable; the first later non-resize mutation reconstructs the exact current hierarchy before incremental damage tracking resumes.
