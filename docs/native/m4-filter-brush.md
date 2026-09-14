# M4 unified brushes, flood and drop-shadow composition

Maintained HLSL now shares one explicit-resource brush dispatcher between the
fine validation probe and both brush-dependent filter kernels. It covers solid,
linear, radial, sweep, four-corner, legacy pattern and resource-pattern tags.
Atlas and independent texture-table sampling preserve the production coordinate,
extend, interpolation, channel rounding and opacity semantics. Every directly
used helper/header is explicitly included. Placement flags are canonical HLSLI.

Flood replaces pixels only in the requested region/active tiles. Drop-shadow
composition places the existing foreground over the scaled shadow; it does not
place the shadow over the foreground. Both preserve allocation padding. The
portable WGSL reference snapshots its destination before composition. Its table
bind group is separate from uniforms, as required by wgpu; algorithms remain the
production WGSL algorithms with binding locations remapped explicitly.

Tests independently distinguish gradient endpoints and four-corner colors, both
image placement types, and every table slot. Texture sampling covers 4x4, 2x2,
3x5, 1x5 and 5x1 images; negative coordinates; clamp/repeat/reflect; nearest/linear;
varying valid premultiplied RGBA, half-channel rounding and opacity 0/127/255.
Odd dimensions use texel centers for the independent byte oracle, while the
power-of-two corpus also exercises interpolation ties. Kernel integration tests
cover all four production variants, compact/rectangular regions and poisoned
padding with an independent integer composition oracle.

Validation: 992 ordinary release library tests, 144 native runtime tests, 21 final
focused tests, strict Clippy, HLSL editor/standalone-header checks and 80 SPIR-V
modules. All three GPU tests replay without DXC and with pipeline-cache hits.
Full SVG/examples retain 3,471 PNGs with only the already approved turbulence
baseline change. Both code-review axes are closed.

Eight production inventory rows are now validated: 175/179 total. These are
kernel tests with explicitly validated fixture dispatch domains, not a claim of
an available full renderer. The final fine entry and real NativeRenderer/Canvas
execution, resource ownership and full immediate-scene integration remain M4 work.
No performance comparison was run, following the user's waiver.
