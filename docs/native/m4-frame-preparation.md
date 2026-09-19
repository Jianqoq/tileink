# M4 native immediate frame preparation

`renderer::recording::Recording` assembles a Canvas without exposing upload/scene
association details to its caller. It merges renderer and scene image namespaces,
reuses placement metadata when resource signatures match, prepares text through
the shared text lifecycle, and invokes the shared native frame scheduler.

Vector images recursively record into the parent's ComputeBatch before their
copies and draws. Child scenes have their own resource namespace and use the same
non-text preparation policy as wgpu vector children. They never inherit the root
renderer image store: that would recursively include unrelated vector sources.
Raster and vector allocations are populated for every fresh batch even when their
CPU placement metadata is reused. Child recorders reuse shared vector-cache
ownership and are pruned when their source leaves the current graph; their plans
and upload metadata remain reusable without claiming that fresh GPU allocations
are ready. No child submit, CPU wait or intermediate
readback is introduced. Explicit readback remains a separate caller operation.

Text preparation belongs to this recorder. A frame without text preparation drops
the previous prepared text slot, so glyph records cannot survive a subsequent
text-disabled render. Shared text reconciliation also covers localized filters.

Tests exercise two-level vector nesting around a raster source, changed image
contents, repeated frames, namespace collisions and invalid device limits. Full
prepared-text GPU coverage now enters this assembly path. Four-API nested/image
namespace comparisons pass (157.24s); assembled text frames pass (159.63s). Release
(1,042), integrations, strict lint, native-only compilation and full SVG/examples
pass. All 3,471 PNGs remain present, with only the approved turbulence difference.
Both reviews are closed. No performance comparison was run. Public NativeRenderer/
context integration and full immediate four-renderer SVG/example acceptance remain
outstanding for M4.
