# M4 fine interpreter and shared scene staging

The maintained `fine_tile_main` HLSL executes the packed coarse particle stream:
solid/image/SDF/fill/path glyph rendering, all six glyph formats, nested clips,
opacity and blend groups. Record access, brush/SDF evaluation, glyph composition,
clip storage, group storage, specialization and the root dispatch are separate
modules. Resources are explicit `FineInputs` fields; the texture table is passed
explicitly. Failed tile specialization restarts the general interpreter once.

The root respects the logical output rectangle, 2D dispatch width, compact active
tile list and target-load policy. Clip/group spills preserve tile, depth and lane
addressing. Local group parents retain floating precision; the five-word spilled
representation intentionally quantizes its parent to RGBA8, matching WGSL.
Glyph coordinates retain signed placement and inverse transforms, exact draw
bounds and invalid-image skipping. Perceptual text dispatch uses the separately
validated text helpers.

Glyph/particle tags and fine stack layout constants are maintained in HLSLI.
The WGSL build imports those declarations; Rust spill allocation imports the same
three layout values. A regression test first reproduced the missing Rust export,
then passed after removing the duplicated numeric definitions. This fixes the
source-of-truth problem rather than adding a synchronization workaround.

Six focused GPU tests compare all four production texture variants on all four
APIs. They include logical/padded/compact tiles, target loading, specialization
restart, ordinary/linear glyph pairs at endpoint and intermediate coverage,
SDF and shadow offsets, inverse Jacobians, path-glyph linear composition, stack
underflow and local/spilled depths, noncontiguous active tiles, lane-dependent
values and untouched inactive/tail spill storage. Independent pixel/spill oracles
supplement the production WGSL references; the glyph perceptual oracle bypasses
glyph-format dispatch and asserts that each ordinary/linear pair differs.

The test reference caches pipelines per device using the complete source hash,
entry, binding layout and descriptor group partition. Each test checks that all
four variants were built and reused. The native runtime continues to use its
persistent artifact and pipeline caches.

`SceneUploadStaging` now lives in `render/upload/scene.rs`. Its CPU path/glyph
planning, tile bins and stable-capacity rules are shared, and the wgpu adapter
already consumes them. Device uploads remain in the API adapter. The existing
capacity growth/shrink tests moved with the implementation without changing their
semantics.

Final verification is recorded in [the receipt](m4-fine-interpreter-verification.json). This
kernel milestone does not complete M4: the public NativeRenderer/Canvas adapter
and its immediate SVG/example four-API acceptance are still required. No
performance comparisons are part of this work, per the user's instruction.

Transformed image coverage includes one and repeated IMAGE particles, atlas and
texture-table placement, draw inverses, quarter-turn brush transforms and clipped
nearest sampling. Single-image expectations independently reproduce the integer
brush-opacity boundary. Repeated images retain floating accumulation until the
RGBA8 store. A temporary raw-float probe measured 3.5372548 before storage and 3
afterward; the probe was removed. This is permitted by the [Direct3D FLOAT-to-UNORM
conversion contract](https://microsoft.github.io/DirectX-Specs/d3d/archive/D3D11_3_FunctionalSpec.htm#3.2.3.6),
which permits 0.6 integer ULP. The independent f64 oracle checks that conversion
bound on one reference, then all four APIs and all four texture variants must
match that reference byte-for-byte. There is no tolerance in backend parity.