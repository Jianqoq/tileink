# M4 native scene raster image resources

SceneImages now materializes shared GpuImageResourceUpload placements into native
RGBA8 atlas layers, standalone textures and a complete descriptor table. Empty
entries use initialized transparent textures. Page shape/order/byte lengths and
checked allocation arithmetic are validated before atlas allocation. Source pixels,
including atlas padding and premultiplication, are copied without conversion.
Unrendered vector placements report an error; vector frame rendering remains work.

The texture-table capacity is generated from its canonical HLSLI header for
production Rust as well as tests. The wgpu image resource limit now references the
same value instead of a second literal. No shader ABI or numerical value changed.

The actual Canvas test verifies atlas and standalone images, nearest and bilinear
sampling, and two differently colored atlas pages. Each wgpu variant receives
placements matching its table capability. All four APIs and fine variants produce
identical final RGBA bytes, including both dense and chunked coarse dispatch.
Independent expected pixels catch missing textures and wrong array page selection.
The GPU test passed in 127.24 seconds including reference shader compilation.

Four CPU tests cover empty initialization, source-byte preservation, unresolved
vectors, and malformed atlas pages. Both review axes are closed. No performance
comparison was run. Full verification results are recorded in the progress log.

This completes raster scene resource recording, not M4. Full native frame/effect
assembly, vector rendering, GPU resource/submission integration and complete
four-renderer SVG/example acceptance still remain.