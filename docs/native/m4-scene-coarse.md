# M4 shared coarse scheduling and Canvas pixel chain

CoarsePlan now owns dense, compact-active and chunked count/prefix/emit ordering.
Both wgpu and native DX12/Vulkan consume it. The wgpu profiling mode uses the same
schedule with separate pass boundaries. Pipeline resolution uses the plan's own
fixed storage; adapters cannot silently truncate stages through a second capacity.
CoarseBatch and the host CoarseConfig moved out of the wgpu adapter. HLSL bindings
remain explicit per entry, and no shader fallback or ABI JSON was introduced.

The native recorder takes scan GPU resources in the same ComputeBatch. A real
Canvas integration harness uses shared scene lengths, tile bins, paint preparation
and execution membership, then records scan, cumsum, coarse and fine without
intermediate readback. Integer full rectangles have an independent exact RGBA
oracle. Fractional triangles and 257 overlapping paths compare final pixels across
wgpu-DX12, wgpu-Vulkan, native-DX12 and native-Vulkan, in dense/chunked modes and
all four fine texture variants. Equality is byte-for-byte with no tolerance.

The five schedule tests cover dense/sparse ordering, chunk allocation, two-dimensional
dispatch limits, empty scenes/draw ranges, malformed dimensions/ranges, and complete
pipeline resolution. The Canvas GPU test passes (129.54 seconds, including pipeline
compilation). All 1,003 ordinary release unit tests and integration tests pass,
as do formatting, strict release Clippy, native-only checking and complete SVG and
examples. The 3,471 PNG baseline has only the previously human-approved turbulence
change. Spec and Standards reviews are closed. No performance comparison was run.

This slice completes shared coarse scheduling and a real geometry-to-pixels chain,
not M4 as a whole. Public NativeRenderer construction remains unavailable. Full
layer/filter frame assembly, shared GPU resource/submission integration and all
immediate SVG/examples on four full renderers remain required. The full SVG/example
runs above are existing wgpu regression checks, not full native renderer acceptance.
