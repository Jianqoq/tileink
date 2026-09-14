# M4 native vector image uploads

SceneImages can ask frame assembly to render each child Canvas into the same
ComputeBatch, then populate its atlas or standalone texture using the shared
image_copy_regions plan. Atlas copies reproduce all nine content/border regions;
standalone textures copy once. Child outputs must be owned non-array textures
matching their Canvas dimensions. Errors return no ready image handles, and the
frame must discard the unsubmitted batch. There is no per-child submission or
intermediate image readback.

Fresh native allocations are always populated even if CPU placement metadata is
clean. A reproduced release failure exposed that vector standalone uploads have
no CPU pixels by design. Those placements now allocate complete checked storage
before GPU copying; malformed raster uploads retain their error behavior. Invalid
vector dimensions are rejected before pixel allocation or child rendering.

Eight CPU image tests cover initialization, cache state, rejected child outputs,
missing vector CPU pixels and oversized dimensions. The actual child Canvas ->
image -> parent Canvas GPU test compares both parent pixels and the entire image
(including atlas borders) against independent raster uploads. Atlas variants and
standalone-table variants, nearest/bilinear sampling, and four APIs pass exactly
(124.64 seconds including reference compilation). Ordinary release (1,026), all
integrations, formatting, strict Clippy, native-only, full SVG and examples pass.
Only the approved turbulence PNG differs among 3,471 images. Both reviews are
closed. No performance comparison was run.

The child-render callback is an integration boundary for the full native frame
renderer. M4 is not complete: public renderer/frame/effect orchestration, remaining
GPU resource/submission integration and complete immediate four-renderer SVG/
example acceptance still remain.