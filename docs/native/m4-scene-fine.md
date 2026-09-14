# M4 shared fine planning and native recording

FinePlan now owns the direct two-dimensional dispatch, uniform construction and
physical-lane spill layout for wgpu and native recording. Sparse frames retain
spill space for physical tile IDs. The native encoder verifies target viewport,
resource ownership and spill capacity before recording explicit HLSL bindings.
No intermediate GPU readback is introduced. The real Canvas scan/coarse/fine
integration harness now uses this production fine encoder.

Review found that unchecked host capacity arithmetic could wrap before a u32
conversion in release builds. The new regression failed before the fix. Shared
coarse work validation now checks multiplication/addition for every packed section,
including the active tile list, and enforces the raw u32 byte-address limit before
either coarse or fine computes offsets. Spill arithmetic is checked separately.
This fixes the arithmetic root cause, not a fixture-specific workaround.

Validation: 11 focused CPU tests, the real Canvas four-API GPU test (all texture
variants and dense/chunked coarse; 130.89 seconds including reference compilation),
1,009 ordinary release unit tests and all integration tests pass. Formatting,
strict release Clippy, native-only checking, full SVG and examples pass. The
3,471 PNG baseline still has only the previously human-approved turbulence
change. Both review axes are closed. No performance comparison was run.

M4 remains incomplete. Full native scene/frame assembly, layers and filters,
GPU resource/submission integration and complete immediate four-renderer SVG/
example acceptance remain. Existing complete SVG/example runs validate wgpu
regression; the public NativeRenderer is still unavailable.
