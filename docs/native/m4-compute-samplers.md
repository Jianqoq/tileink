# M4 explicit sampler resources

Compute batches own explicit nearest/linear clamp-to-edge samplers for single-mip
image resources. Bindings validate sampler kind and batch ownership, and readback
rejects non-byte resources. Shader parameters are declared in the root and checked
against DXIL sampler registers and SPIR-V sampler types.

DX12 keeps sampler and resource descriptor heaps in a dedicated table module.
Root indices include only tables actually present, then internal dispatch constants.
Both heaps remain owned by the frame. Sampler pipelines use a separate layout cache
identity; existing buffer-only layouts preserve their cache identity. Vulkan owns
samplers until the submission frame retires and uses explicit SAMPLER descriptors.

The four-route test independently checks nearest/linear/nearest pass switching,
clamp boundaries, exact centers and half texels, three array layers, padding guards,
2D dispatch with internal constants, and switching to a pipeline without samplers.
All channel bytes match and native validation is clean in the first GPU run.

The test exposed a pre-existing wgpu DX12 descriptor warning: non-comparison
samplers used the default ALWAYS comparison function. The maintained HAL now uses
NEVER when comparison is absent, preserving explicitly requested comparisons.
This fixes the descriptor root cause; the ignored field has no sampling effect.
Full release (970 library tests), 71 native runtime tests, shader/compiler and
editor checks, 39 SPIR-V modules, strict Clippy and SVG/examples pass. All 3,471
PNG hashes are unchanged. No-DXC replay compiles no new pipelines. Both reviews
are closed. M4 remains incomplete at 27/179.
