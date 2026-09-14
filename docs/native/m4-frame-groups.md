# M4 native frame groups and masks

The production native Execution records shared draw and recursive group/mask
scheduling into a single ComputeBatch. It handles isolate, opacity and blend
groups, alpha/luminance masks, rectangular and path regions, outer layer stacks,
and sibling scratch reuse. Existing Canvas/image/vector GPU tests now use this
executor instead of their hand-written direct-root loop.

Targets separates logical scratch occupancy from physical allocation ownership.
Release permits subsequent ordered commands to reuse a slot; take transfers its
surface lease and the next acquire allocates another image. Replacement and
release never remove a resource referenced by earlier commands. ComputeBatch,
then the native submitted frame, retain physical resources through completion.
These registries are per-batch and do not claim cross-frame retained GPU reuse.

A review exposed an unsafe association at the frame boundary: callers could
supply an upload and unrelated SceneImages from the same batch. Images now has
private fields and constructs both together. Execution accepts only that pair,
checks batch ownership before allocating, and cannot silently swap placements.
HLSLI filter operation tags are also exported to both native and wgpu host code.

The focused GPU test passed (167.87 seconds). It covers shared group/mask execution,
colored luminance, a nonrectangular path, nested and sibling surfaces, dense and
chunked coarse, and four fine/filter variants. Expected full-frame pixels come
from real production wgpu renderers on DX12 and Vulkan and are also independently
asserted for the rectangular cases. No tolerance or transparent-pixel exemption
is used. No performance comparison is run.

The full wgpu reference emits DX12's optimized-clear-value advisory. Only this
whole-renderer test path accepts that exact message ID at WARNING severity,
retaining and printing the original message. Ordinary native validation remains
strict; a real info-queue regression proves the same ID at ERROR severity and an
unrelated WARNING still fail. No pixel rule changes. The clear value is optional
optimization metadata, as documented in [D3D12_CLEAR_VALUE](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/ns-d3d12-d3d12_clear_value).

The four existing Canvas/image/vector GPU regressions also pass (483.98 seconds).
The four focused CPU ownership tests, 1,030 ordinary release tests, integration
tests, formatting, strict Clippy, native-only build, full SVG and examples pass.
Among 3,471 PNGs only the previously approved turbulence image differs, with no
missing or added outputs. Both code reviews are closed.

M4 remains incomplete: the public NativeRenderer is unavailable. Full filter/
backdrop and text/frame assembly, shared GPU resource/submission integration, and
the complete immediate four-renderer SVG/example acceptance still remain.
