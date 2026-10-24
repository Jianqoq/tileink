# M4 compute textures

The existing compute batch now owns typed buffer and RGBA8 texture resources in
one resource collection. Handles retain batch ownership checks. Bindings reject
wrong resource types, foreign handles and writable aliases before recording.

DX12 uses device copy footprints for multirow upload and strips readback row
padding. SRV/UAV views, compute state transitions and lifetimes share the existing
command-list submission and receipt. Vulkan owns partial image allocations via
RAII; uploads transition into GENERAL for sampled/storage compute use, and image
readback transitions to TRANSFER_SRC_OPTIMAL. Existing buffer arenas remain in use.

The HLSL root explicitly annotates the SPIR-V RGBA8 storage format: unorm float4
alone was experimentally found to produce Rgba32f. Only the format attribute may
be conditional on DXC's built-in __spirv__; includes and shader resources remain
unconditional. Both formats pass strict compilation, and reflection checks the
actual storage image format rather than trusting the source annotation.

The first four-API test covers odd widths 1, 3 and 65, multiple rows, reversed
coordinates and channels, two-pass write-then-sample, and untouched output borders.
Independent CPU expected bytes match all four routes with clean native validation.
Invalid resources and corrupted SPIR-V format/dimension/access have focused tests.

Validation is complete: 968 library tests, 67 native runtime tests, shader
compiler/inventory/reflection tests, real Shader Tools diagnostics, 37 SPIR-V
modules and strict all-target Clippy pass. Full SVG/examples preserve all 3,471
PNG hashes. A no-DXC replay hits persistent caches with no runtime compilation.
Both code reviews are closed; mixed buffer/texture readbacks and upload-only
batches are also checked on all four APIs. This is resource support, not a
production inventory entry. M4 remains incomplete at 27/179.