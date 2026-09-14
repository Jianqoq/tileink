# M4 scene GPU geometry for layer filters

Native layer masks and stack compositing can now consume the Scene-owned scan,
paint and layer buffers. Internal constructors state the validated upload contract;
recording still checks resource ownership. Geometry is neither read back nor
uploaded a second time. Filter and fine reference variants are selected explicitly
for mixed batches, allowing an actual scan/fine/mask/composite chain on four APIs.

Review found a root-cause bounds bug: replaceable Scene plan metadata could make
coarse and filter-stack bounds exceed their already-uploaded layer storage. The
release regression failed before the fix. Scene now exposes only a read-only plan
and retains the upload's layer_count for both checks. The regression deliberately
replaces internal metadata and verifies neither consumer records an invalid pass.

Validation: 60 focused scene/retained tests, 1,020 ordinary release tests and all
integration tests pass. The Canvas GPU test uses a fractional triangle clip and
checks that fine alpha equals layer-mask coverage and stack-composite coverage,
then compares exact bytes on wgpu-DX12/Vulkan and native DX12/Vulkan across all four
fine/filter texture variants. It passed in 111.86 seconds including reference
compilation. Formatting, strict release Clippy, native-only checking, full SVG and
examples pass. The PNG set has only the previously approved turbulence delta.
Both review axes are closed. No performance comparison was run.

M4 is still incomplete: these are production stage/resource integrations, not a
public complete NativeRenderer. Full frame/layer/filter orchestration, vector
images, GPU resource/submission integration and all immediate SVG/example cases
on four complete renderers remain.