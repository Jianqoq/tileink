# Metal TBDR drawing

The Metal fine stage uses hardware render passes on Apple7-or-newer GPUs,
with tile shaders for full-target passes and fragment shaders for local draws.
Both evaluate analytic coverage and ordered paint/clip/group operations against
the color attachment in tile memory. This is the actual pixel renderer, not a
presentation pass over a compute-rendered image. The previous compute backend
was already hardware accelerated; using Metal alone does not imply using the
hardware tile render pipeline.

Full-target fine passes dispatch directly to 16×16 hardware tiles with 256 threads
per tile. Clip-local and retained sparse passes instance quads for the unique active software tile list, fetching
current color from the attachment. This avoids dispatching a tile shader over
the whole attachment for a small dirty region, and needs no auxiliary activity
mask. Sparse passes let Metal choose the hardware tile size; a scissor clips
edge tiles to the logical viewport even when the backing texture is larger.

Software 16×16 tiles continue to index coverage records and per-pixel spill
storage. Path preparation, prefix scans, binning, and neighborhood filters remain
compute operations. Analytic antialiasing is preserved. Hardware TBDR does not
automatically remove the ordered interpreter's translucent overdraw.

The render pass discards old contents only when the entire attachment is
replaced. Blending over the destination, sparse updates, and oversized backing
textures load the attachment, preserving pixels outside the draw. Host textures
used directly as attachments must declare `RenderTarget` usage. Unsupported
non-Apple GPUs are rejected during context creation; there is no legacy compute
fine fallback. Tile, fragment, and vertex resource bindings and uniform layouts are checked
by reflection. All encoders close on error as well as success.

Immutable uniforms of at most 4096 bytes now use Metal's copied encoder bytes,
avoiding one short-lived MTLBuffer upload allocation per constant resource.
Compute and tile passes bind their respective stages; sparse fine drawing binds
config to both vertex and fragment, keeping active tile IDs in their storage
buffer. The shared immutable-uniform classifier also serves DX12 and Vulkan's
existing packed upload arenas: all uses must be uniform reads, with no storage
alias, persistent-buffer identity or explicit readback. Larger constants retain
the ordinary buffer path. Metal copies bytes during encoding, so host recording
data can be dropped before GPU submission/completion. This addresses allocation
churn directly, rather than changing rendering, coverage or retained history.
Tests cover aliases, CPU readback identity, the 4096/4100-byte boundary and actual
GPU execution after host data is dropped. The limit follows Apple's
[setBytes contract](https://developer.apple.com/documentation/metal/mtlcomputecommandencoder/setbytes%28_%3Alength%3Aindex%3A%29).

Command completion now wakes a dispatch semaphore through a Metal completion
handler, replacing the old 1 ms status polling interval. The bounded wait and
pending-resource ownership are preserved; the handler captures only a semaphore.
A GPU regression checks timeout, subsequent completion, and repeated waits.

The migration also removes Metal's unconditional re-upload of unchanged tile
bin records and indices. They are read-only inputs in both render stages, and
now share the accepted-snapshot reuse used by the other native backends. The
existing GPU allocation/reuse regression failed before this correction and
passes afterward. A separate abandoned-upload test now uses a buffer large
enough to exercise sparse uploads; its old four-word input selected the
intentional dense-upload policy and did not exercise the asserted scenario.

Coarse classification selects stack-free color and analytic fast paths inside
the fine shader. Particle rounding and blend order are unchanged. Unexpected
tags restart the general interpreter from the original destination. GPU tests
cover full/sparse rendering, nonzero backgrounds, empty tiles, oversized targets,
mixed coarse kinds, terminators, and uniform or varying loaded destinations.

## Clip-local scheduling on Metal

Metal now participates in the shared conservative clip dispatch algorithm.
Previously an explicit Metal opt-out returned before any clip selection, and a
second backend guard disabled preallocated clip slots. Small clip batches therefore
repeated count/prefix/emit and full-viewport fine work despite affecting few tiles.
The opt-outs are removed after Mac validation; the fine rendering architecture
and general allocation algorithm remain intact. Validation exposed three Metal
emitter gaps: scalar emission ignored the active tile list, rejected clips left
stale particle streams, and parallel emission retained old EMPTY/COLOR kinds.
The emitters now honor sparse IDs and padded groups, write rejected-stream
terminators, and reset parallel classification. These semantic fixes are required
to enable reuse.

Pure clip stacks intersect clip bounds with the viewport and, for complete
repaints, known child bounds. Retained damage intersects the clip mask without
restricting to current child bounds, preserving removal and reparenting semantics.
Dense retained selection now stops at the first matching tile exceeding the sparse
budget, preserving the exact threshold and active-list order; this CPU change is
shared by all native backends. See the [clip selection contract](clip-tile-selection.md)
for required invariants, regression tests and measured limits.
Dense selections use the regular schedule. Complete non-text pure-clip plans
reuse bounded disjoint particle slots, requiring only emission instead of
recounting and reallocating each batch. Mixed group/opacity schedules and text
retain regular allocation where those slots cannot preserve semantics.

Planning regressions previously excluded Metal now run on it. GPU checks compare
reused slots against regular allocation, including rejected tiles, reused EMPTY
classifications, small/large emitter schedules, and nested clip spill depths.
This fixes a disabled optimization, not the known turbulence canonical mismatch.

## Validation of the first migration (historical)

The first tile-kernel implementation was tested on Apple M2, macOS 15.0.1 (24A348),
Rust 1.96.1, with `--no-default-features --features metal`. Correctness runs
used `MTL_DEBUG_LAYER=1` and a single test thread. The full release run across
34 test executables produced **910 passes and one existing failure**. Two
DXC-dependent shader compilation tests were explicitly excluded on this Metal
setup. The focused Metal run passed all 50 tests, including the new tile,
attachment-usage, and completion-timeout regressions.

All 1,712 SVG fixtures plus `examples/tiger.svg` were rendered with the old and
new binaries at the same 300-pixel width: **1,713 of 1,713 RGBA images match
exactly**. Both progressive-blur quality examples also match exactly. The native
presentation example completed eight frames and two window sizes using separate
rendering and presentation queues. `cargo fmt`, `git diff --check`, and release
`cargo clippy --all-targets` passed; clippy reports seven existing dead-code
warnings in test support.

The remaining test failure is
`scaled_turbulence_matches_canonical_rgba` at 1600 pixels. Both the original
compute renderer and this tile renderer produce SHA-256
`935384ae90337a0c8fc3342643e26e6d1ec6f804f653e7f7a45552ef9231a6f2`;
the test's four-route canonical reference expects
`759888711bfa14070fd01c6640d6be0ac41d65b96942784475110034506caf82`.
The 300-pixel canonical check passes. This migration preserves the original
Metal output; it does **not** resolve that pre-existing canonical mismatch or
change its expected hash. Consequently the full test command still exits with
a failure, and the migration must not be described as an entirely green run.

## Clip scheduling follow-up

Full-raster, indirect-routing, and SIMD experiments were rejected and removed;
they are not the shipped rendering architecture. The remaining clip render
passes cannot be merged across dependent resource updates.


## Large sparse regular coarse scheduling

Metal incremental coarse allocation uses one scalar lane per active tile once the
list contains at least 256 tiles, grouping 256 tiles per count/emission dispatch.
Smaller lists retain the parallel per-tile workgroup. Dense rendering and other
backends retain their schedules. This applies to regular allocation as well as
preallocated pure-clip slots; text and mixed stacks still require regular prefix
allocation and are not allowed to borrow pure-clip particle ranges.

The scalar counter maps through the active ID list rather than dense bin geometry.
IDs may be sparse and nonmonotonic. Padded lanes return before reading the list;
inactive counts, classifications and retained history must remain untouched.
Counting, glyph destinations, ordered particle emission and prefix offsets keep
the existing semantics. Eliminating count/prefix for preallocated slots must retain
an existing scalar emitter instead of accidentally removing all drawing.

This is a root-cause performance fix. Maximized AAPL Manual Replay at 3420x1966
on Apple M5 dirtied about 14,500 tiles across 21.6 batches per frame. The old regular
schedule launched 256 lanes per tile even when most UI draw lists were short.
A Metal trace attributed 84.5% of summed GPU channel durations to Compute, dominated
by the coarse count/prefix/emit groups. Three alternating real-application pairs
improved mean accepted-present interval from 33.432 ms to 16.664 ms and mean
acquisition/retirement from 25.325 ms to about 0.098 ms. The built-in display reports
60 Hz; these intervals are application cadence, not scanout or input latency.

Planner regressions cover empty work, empty draws, the 255/256/257 boundary and
retained preallocated emission. A physical Metal regression checks arbitrary active
ID order, inactive sentinels and padded groups; it failed before this fix. All 1712
SVG fixtures plus tiger and both progressive-blur qualities match frozen baseline
outputs byte-for-byte. Native presentation completes eight frames and two sizes.
Release all-targets Clippy and formatting pass with existing warnings.

The serial, API-validated release all-targets run on GPU 000000010000058b has 887
passes and six failures. Five coarse glyph/clip-oracle failures reproduce with the
same differing bytes on untouched f052665dd3df951bc34fd8326550143d065a035e. The existing
1600-pixel turbulence canonical mismatch also reproduces: both builds on this M5
produce ba95370899224599efa0bbd4d3d3915eb2bf9e4ed189a94553fecde0302231b2. Two unavailable
DXC tests and one test lacking historical WGSL evidence are explicitly excluded.
This is not an entirely green all-backend certification. Temporary Metal encoder
labels are removed; the app keeps an opt-in real Replay pan regression workload.
The full profile and remaining optimization candidates are documented in Northstar's
`docs/replay-pan-metal-performance.md` and its Chinese translation.

## Shared filter validation follow-up

The subsequent CPU optimization replaces repeated filter tile tree validation and
GPU uploads in the common recorder. It applies to Metal, DX12 and Vulkan; its
[ownership, content identity and range invariants](filter-tile-validation.md) are
required independently of the Metal coarse schedule. Three real AAPL pan pairs
reduce submission mean from 5.020 to 2.886 ms and total rendering from 6.132 to
4.281 ms. Cadence stays at the display's 60 Hz, with no consistent interval-tail
improvement. Seven new tests pass, full SVG/blur outputs are unchanged, and the
all-targets run has 894 passes with the same six existing failures.

## Shared blur output workgroups

The shared blur stage now rejects whole workgroups outside its output rectangle
before loading the axis halo. This removes GPU work hidden inside the expensive
filter/coarse Compute cluster. The [shared-blur contract](shared-blur-workgroups.md)
requires uniform rejection, partial-tile barrier participation and exact retained
history. HLSL uses the same fix for DX12/Vulkan. Validated AAPL diagnostic pairs
improve mean interval by 5.1%; they do not meet the 120 Hz tail budget.
