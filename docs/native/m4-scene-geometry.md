# M4 Canvas geometry assembly

The native recorder now consumes real Canvas lines and path records together with
shared PersistentPathPlans. PreparedScan refreshes the plan and borrows both scene
and plan until recording finishes, preventing stale same-sized geometry metadata.
Six scan stages feed cumsum in one ComputeBatch; their GPU resources are returned
to the next stage without intermediate readback.

Persistent arena layouts contain empty row and chunk slots. Cumsum now accepts
unowned zero-length chunks, still rejects unowned live chunks and overlapping
rows, and chooses row offsets from actual row lengths. This fixes the root cause:
table cardinality does not describe how many live chunks each row owns.

Tests cover empty scenes, invalid grids, capacity bounds, stale same-count plans,
filled triangles, nonzero backdrops and a path spanning multiple cumsum chunks.
The cumsum arena regression failed before the fix. Four API geometry comparisons
sort segment records within each tile because atomic allocation may permute that
intermediate list. Other outputs compare exactly; final pixel equality remains a
strict byte-for-byte requirement.

Validation: 998 ordinary release unit tests, all integration tests, 15 focused
scan/cumsum tests and a final six-test wide-scene run pass. Formatting, strict
release Clippy, native-only checking, full SVG and all examples pass. The fixed
3,471-image baseline has only the previously human-approved turbulence change.
Both implementation review axes are closed. No performance comparison was run.

M4 remains incomplete. This is geometry assembly, not the public NativeRenderer.
Coarse/fine scene assembly, shared GPU resources/submission and complete immediate
SVG/example four-route acceptance remain required. The SVG/example runs above
validate existing wgpu regression only.
