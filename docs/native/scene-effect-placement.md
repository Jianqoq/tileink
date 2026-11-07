# Scene placement and compact filter surfaces

Scene placement transports a filter/backdrop sampling region and mask region in
the same physical coordinate frame as the layer's draw and clip records. This is
required for both `Canvas::append_transformed` and retained transform updates.
Filter kernel strengths, such as Gaussian sigma, remain authored physical pixel
values after Canvas DPI conversion.

## Sampling and mask geometry

The previous append path transformed GPU draw/path records but only translated
CPU effect metadata. An initial retained scale or rotation also left that metadata
at its authored position. A half-open popover with a 0.99 presentation scale could
therefore lose its right/bottom shadow, or its complete shadow at another origin.
This repair transports sample/mask geometry with the full physical affine; it does
not enlarge the window, bypass clipping, add layout padding, or change the shadow.

A uniformly scaled, axis-aligned rounded rectangle stays a rectangle. Reflections
also exchange its corner radii. Rotation, shear and nonuniform rounded scaling use
a transformed rounded path, preserving the shape instead of substituting its AABB.
Path regions compose their existing transform with the scene placement.

Retained leaf topology is fixed while the materializer handles transform-only
updates. The first update captures original effect commands with their list/index
positions; subsequent poses always recompute from those immutable commands. This
prevents accumulated rounding and double transforms across reversals. Ordinary
canvases without effects allocate no snapshot. Clones share immutable originals,
while `Canvas::reset` drops the snapshot before re-recording or changing topology.
The materializer's descriptor fingerprint still determines execution-plan changes.

## Offscreen coordinate frame

Compact offscreen rendering changes the destination origin, not a draw's local
geometry or paint frame. For a draw transform `A` and surface origin `o`, the local
presentation is `T(-o) * A`. Subtracting `o` from payloads and keeping `A` instead
produces `A * T(-o)`, which displaces scaled and rotated ink. At scale 0.99 this
created position-dependent shadow differences even after sampling bounds were fixed.

Draw/path transforms are rebased, and each draw inverse is recomputed. SDF, SDF
shadow, brush, path-line and glyph coordinates remain local and unchanged. Physical
draw bounds move into target coordinates; path backdrop/tile allocation is rebuilt
from those bounds and the rebased scan transform. Pure translations retain tight
scan capacity; general affines use the renderer's conservative transformed bound.
CPU filter descriptors already expressed in physical target coordinates still use
the ordinary offscreen coordinate conversion.

## Filter descriptor scope

The geometry guarantee above does not add a general filter-frame API. Existing
pure-translation placement continues to transport coordinate-aware filter fields
(graph primitive bounds, paint transforms, procedural origins and light positions)
through `SceneOffset`. Under non-translation placement, the filter descriptor itself
remains expressed in physical canvas coordinates; only its sample/mask geometry is
transported. Kernels and descriptor-internal absolute coordinates are not scaled or
rotated by this operation.

Applications requiring a fully transformed procedural filter field must author its
descriptor in the destination physical frame. Some paint/procedural types currently
store only axis-aligned coordinates, so transporting a subset of their fields would
not provide correct general affine support. This repair deliberately does not add
partial support or change the GPU ABI. A rotated/scaled rounded region may become a
path; filters restricted to rectangular regions retain their existing validation.

## Regressions and validation

`canvas::tests::effect_placement` checks append, retained first placement, reversals,
nested placement, rectangle/path sampling, backdrop/mask geometry, rounded coverage,
clone independence and reset/re-recording at DPI 1, 1.25, 1.5 and 2.
`shared::offscreen::tests::placement` checks physical draw/path positions, fine-sample
inverses, unchanged gradient/SDF/shadow/glyph payloads and sufficient transformed
scan allocation under scale, rotation and shear at the same four DPIs.

The native Trade App regression fixes two identical half-open Popovers at progress
0.5 and translates one by 320 pixels. It requires zero pixels differing by more
than two channel levels and visible shadow four logical pixels outside every side
of both panels. Before either repair it found 3545 mismatched pixels (maximum error
9) and missing edges; after region placement alone all sides appeared but 1248
pixels still differed (maximum error 4). Both root-cause repairs pass the unchanged
pixel tolerance and complete-edge requirements.

The complete Windows DX12 release suite passes (1044 tests, 228 existing opt-in
tests ignored), all examples build, and strict all-target release Clippy passes.
All 1712 SVG fixtures were rendered with both the unmodified `d534fb31` baseline
and the repaired renderer on the same machine; their PNG files are byte-identical.
