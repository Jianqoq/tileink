# M4 atlas pattern sampling

Maintained HLSL helpers decode serialized atlas brush records and receive paint,
atlas and sampler resources explicitly. Pad/repeat/reflect, nearest and bilinear
sampling, opacity, transformed coordinates and empty images match production WGSL.
The canonical bilinear selector is defined in HLSLI. Texture-table resource variants
remain later work; these validation adapters are not production inventory entries.

The independent reference calls production `sample_brush`, relocating only its
atlas/sampler binding locations. CPU expected bytes independently check coordinate
extension, hardware versus staged bilinear interpolation and opacity. The corpus
covers 89,046 requests across pages, transforms, singleton/empty images and 3x5
non-power-of-two images, plus a minimal negative-repeat regression.

For non-power-of-two images, independent ideal-coordinate expectations use texel
centers with identity/rotation. A translated ideal boundary can differ from the
serialized f32 transform: all four APIs agreed in that observed case. Other image
sizes retain translated boundary coverage.

## Root-cause fix: negative repeat coordinates

The first 3x5 repeat case at (-0.5,-0.5) returned the origin texel only on native
Vulkan. A one-request regression reproduced it. The HLSL helper used negative
`value % positive_period`; DXC documents that HLSL modulo is only defined for
same-sign operands. See [DXC's SPIR-V mapping documentation](https://github.com/microsoft/DirectXShaderCompiler/blob/main/docs/SPIR-V.rst).

The helper now computes unsigned magnitude/remainder and then the Euclidean sign
correction. Unsigned negation also avoids signed INT_MIN overflow. This removes
undefined source semantics rather than changing pixel tolerances or selecting an
API-specific result. Both focused tests pass on all four routes with clean native
validation. Full release (970 tests), 73 runtime tests, strict Clippy, all HLSL editor/header checks and 40 SPIR-V modules pass. Full SVG/examples leave all 3,471 PNG hashes unchanged. Both atlas tests replay without DXC and compile no pipelines; shader compiler/inventory/artifact checks pass. Both independent reviews are closed. M4 remains 27/179.