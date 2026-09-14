# M4 fine gradient helpers

Native HLSL implements linear, radial, sweep and four-corner gradients, ramp
sampling and pad/repeat/reflect extension. Brush helpers receive paint buffers
and base offsets explicitly. Shared brush constants are declared in HLSLI.
These are helper validation entries, not completed production fine variants.

## Root causes covered by regression tests

Four-route tests exposed half-channel differences from implicit multiply-add
contraction in linear projection and radial quadratic coefficients. Both shader
languages now specify fused evaluation explicitly. This establishes common
arithmetic rather than rounding with a tolerance or selecting backend branches.

At the sweep center, atan2(0,0) returned different values on DX12 and Vulkan,
producing channel 64 versus 0. The center now has the explicit angle zero before
calling atan2. The GPU corpus independently requires the first ramp color there.

The reference adapter calls production WGSL brush functions, assembled through
the production atlas-only shader patcher. It does not implement a replacement
reference algorithm. Native shaders are independently maintained HLSL with cached
DXIL and SPIR-V artifacts.

## Verification

The corpus covers transforms, three extension modes, degenerate axes/circles,
positive/negative sweep spans, center coordinates, empty/singleton ramps and
four-corner interpolation. Guard words verify padded dispatch bounds.
Release verification passed: 967 library tests, 65 native runtime tests, strict
all-target Clippy, six shader compiler/header tests, inventory/artifact checks,
real Shader Tools and 36 SPIR-V modules. All SVG/examples passed and the immutable
3,471 PNG baseline is unchanged. The gradient test passed with nonexistent DXC
paths and zero new runtime pipeline compilations. Three existing wgpu gradient
regressions also passed. Spec and standards review findings are closed.
The production Rust FineConfig now lives in shared/fine_config.rs. Native interface
fields use offset_of! on that same type; GPU tests serialize the production type.
The validation entry receives an explicit logical request count, since physical
buffer extents can include allocation padding. An irregular 10,315-record corpus
retains output sentinels beyond the logical count.
