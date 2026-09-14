# M4 sampled texture arrays

Array views are explicit even for one layer. Resource validation rejects mixing
D2 and D2Array bindings, empty arrays, inconsistent bytes and oversized layer
counts. No implicit texture conversion is used.

DX12 queries each subresource footprint and shares the same row/layer mapping for
upload and readback. This handles independent layer offsets rather than assuming
one continuous row pitch. Vulkan and wgpu copy tightly packed array layers and
create explicit array views. Shader reflection checks the array dimension.

Tests select each layer in reverse order, exercise single-layer arrays and odd
widths, and independently compare all bytes on four APIs. Full release (969 library tests), 69 runtime tests, strict Clippy, shader/compiler
checks, 38 SPIR-V modules and real editor diagnostics pass. Full SVG/examples
preserve all 3,471 PNG hashes. Two four-route texture tests pass without DXC
and with no pipeline compilation; both reviews are closed.
This resource slice does not add production inventory entries; M4 is incomplete.
