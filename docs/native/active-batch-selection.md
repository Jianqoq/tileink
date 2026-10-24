# Shared native active-batch selection

Active batch selection previously scanned every draw incidence in the dirty tile
bins; transient bins also copied those incidences into scratch and rescanned them.
A large pan repeatedly visited the same draw and batch across thousands of tiles.
`ActiveBatches` now queries draw bounds for dense damage and streams bins for sparse
damage. This removes repeated membership work at its source, without changing GPU
coverage or frame pacing. Metal, DX12 and Vulkan use the common recording layer.

## Required invariants

- Scene preparation must reconcile bins, Canvas records and the execution plan
  before querying. Stable batch IDs take precedence over plan batch IDs.
- Output remains the sorted, deduplicated set of non-sentinel batch IDs present in
  at least one active tile. Empty damage produces an empty set.
- When dirty tile count exceeds draw table length, query each live draw's exact
  tile-rounded bounding box against `DamageTiles` words. Once a batch is found,
  further draws in that batch need no intersection query. With painter keys,
  inactive entries remain excluded; otherwise use execution-plan draw order.
- Sparse selection streams the existing paged or transient flat bins, avoiding
  the temporary incidence array. Both algorithms preserve partial edge tiles,
  conservative membership of degenerate pixel bounds, and holes in damage.
- Query generations, marks and output scratch belong to `SceneUploadStaging`,
  separate from the shared `Rc<TileDrawBins>`. A read-only query must not trigger
  copy-on-write of a recorded spatial snapshot. Real spatial updates still do so.
- Generation wrap clears marks before generation one is reused. Missing draw IDs
  and `u32::MAX` batch IDs remain ignored. Painter order, clip streams, GPU particle
  capacities, cached uploads, inactive history and resource retirement are unchanged.

## Validation

A focused regression first reproduced spatial-index copy-on-write during a query
with an outstanding shared snapshot. Semantic tests cover paged and flat bins,
multiple draw pages, sparse/dense/full/empty damage, inactive painter entries,
noncontiguous stable batch IDs, duplicates, missing IDs, word boundaries, partial
edge tiles, degenerate bounds and generation wrap. The dense path is compared with
the exact tile-bin query as well as explicit expected sets.

Real application results and release/rendering verification are recorded in the
application's bilingual `docs/replay-pan-metal-performance.md` report. Hardware
measurements here use Metal; common algorithm changes do not certify DX12/Vulkan
hardware speedups or a physical 120 Hz display.
