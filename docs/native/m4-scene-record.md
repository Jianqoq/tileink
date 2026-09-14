# M4 native Canvas scene recording

SceneCache now assembles Canvas path plans, text/paint uploads, tile bins, logical
batch IDs, layer records and spill allocations into a native Scene. Scan, cumsum,
coarse and fine share one ComputeBatch with no intermediate readback. CPU execution
metadata is reused only for the current Canvas; failed preparation is rebuilt on
retry. GPU allocation reuse and retained incremental uploads remain later work.

LayerStackRecord conversion is shared with wgpu so opacity quantization and blend
encoding have one implementation. Batch selection uses logical IDs, which may be
sparse and exceed the number of physical draw records. Physical indices come from
validated shared tile bins. Upload helpers check raw-buffer sizes before copying.

The Canvas GPU test uses this production recorder and covers empty output, full
rectangles, fractional edges, 257 overlapping paths, metadata reuse and six nested
clip/opacity pairs exceeding local shader stacks. It compares exact final bytes
across wgpu-DX12, wgpu-Vulkan, native DX12 and native Vulkan, all four fine texture
variants, and dense/chunked coarse emission. It does not read back intermediate
geometry. CPU tests cover dimension changes, failed retries, resource ownership,
layer bounds and sparse logical batch IDs.

Validation: 1,014 ordinary release tests and integration tests pass, alongside
formatting, strict release Clippy, native-only checking, full SVG and examples.
The 3,471 PNG baseline has only the previously human-approved turbulence change.
Standards and Spec reviews are closed. No performance comparison was run.

M4 remains incomplete: the public NativeRenderer is still unavailable. Full frame
assembly, image/vector resources, offscreen layers and filters, GPU resource and
submission integration, and complete immediate four-renderer SVG/example acceptance
remain. The full existing SVG/example runs validate wgpu regression only.