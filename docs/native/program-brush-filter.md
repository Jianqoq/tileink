# Native application shaders

`ShaderProgram::new` accepts complete Metal and/or HLSL source files and an entry
name. Tileink compiles the supplied source **verbatim**. There is no expression
builder, shader generator, instruction stream, interpreter or fallback program.
This replaces the previous experimental expression architecture permanently.

```rust
use tileink::{ShaderProgram, ShaderSources, ShaderInput};
let program = ShaderProgram::new(
    ShaderSources {
        metal: Some(include_str!("glass.metal").into()),
        hlsl: Some(include_str!("glass.hlsl").into()),
    },
    "glass",
    ShaderInput::Sampled { radius: 32.0 },
    29, // raw 32-bit uniform words
)?;
let filter = program.filter(uniform_words)?;
canvas.push_backdrop_layer(filter.into(), region);
canvas.pop_layer();
```

Provide the language for every supported backend. Missing backend source,
compilation failure, missing entry and invalid reflected interfaces return a
rendering error. Metal compiles with the device compiler; DX12 compiles HLSL to
DXIL and Vulkan compiles HLSL to SPIR-V using the pinned DXC toolchain. Sources
must be self-contained; resolve application includes before submission. The
framework does not insert includes, helper functions, declarations or wrappers.

## Resource and dispatch contract

Use the complete files in `examples/shaders/` as application-owned templates.
`ShaderConfig` is a public 112-byte scalar structure; its Rust fields define
byte order and offsets. Copy its declaration into each source file. HLSL resource
and configuration field names follow these templates because DXC reflection
checks names as well as types, slots, offsets and workgroup size. Metal checks
configuration scalar offsets/types and the active resource slots. Unused
resources may be optimized out; the writable output must remain active.

| Slot | Metal | HLSL | Meaning |
| --- | --- | --- | --- |
| 0 | `constant ShaderConfig&` buffer(0) | `ConstantBuffer<ShaderConfig> config : register(b0)` | Geometry, logical mapping and sampling declaration |
| 1 | read-only texture(1) | `Texture2D<float4> input_image : register(t1)` | Immutable input, only when declared |
| 3 | write-only texture(3) | `RWTexture2D<float4> output_image : register(u3)` | Premultiplied sRGB RGBA8 output |
| 8 | `const device uint*` buffer(8) | `ByteAddressBuffer active_tiles : register(t8)` | Compact 16×16 tile IDs |
| 10 | `const device uint*` buffer(10) | `ByteAddressBuffer uniforms : register(t10)` | Application's raw uniform words |

Vulkan uses descriptor set 0 and binding numbers equal to the slots. HLSL
storage images declare `[[vk::image_format("rgba8")]]` under `__spirv__`.
The workgroup is **256×1×1**. Linear lane index is
`(group.x + group.y * config.dispatch_width) * 256 + lane`. Dense regions map
that index across `region_width`; compact regions index the tile list and then
the 16×16 pixel within each tile. Guard the lane count, canvas extent and output
region. Write at `physical_pixel - (output_x0, output_y0)`. The example helpers
implement both dense and compact dispatch, including pooled texture padding.

Logical positions apply `inverse_*` to physical pixel centers. Logical input
displacements apply `sample_*` before sampling the full input texture. Those
maps preserve DPI, translated appends, offscreen surfaces and brush transforms.
Uniforms are arbitrary `u32` words: floats use `f32::to_bits` / `as_type<float>`
/ `asfloat`; integers and bit patterns remain unchanged. Supply exactly the
word count declared by the program. `with_uniforms` changes parameters without
changing the pipeline key. Zero-word uniforms bind a four-byte placeholder.

The application shader owns arithmetic, bounds checks, sampling and output
premultiplication. Tileink does not sanitize or rewrite native code. In particular,
`ShaderInput` is a dependency declaration, not an injected sampling restriction.
The example shader explicitly clamps logical offsets, handles transparent-border
bilinear samples and rounds premultiplied output to RGBA8.

This ABI exposes at most one input image, one RGBA8 output image, read-only tile
IDs and read-only uniform words. It does not expose application-bound extra
textures, writable storage buffers, multiple outputs or history images. Native
loops, branches, arithmetic and local threadgroup operations remain available
within the supported Metal 2.4 / HLSL `cs_6_0` compute language and this ABI.
`ShaderProgram::filter()` returns an ordinary single-pass `ShaderPipeline`.
`Filter::Program` and `ProgramFilter` were removed; no separate filter execution
path remains. Chains/graphs compose these pipelines normally.
For application-owned resources, writable buffers, floating-point intermediates,
in-place texture access and explicit multipass/resolution scheduling, use
[ShaderPipeline](shader-pipelines.md). That interface reuses the same native
resource pools, synchronization and renderer integration.

## Input semantics and scheduling

- `ShaderInput::None`: no input resource. Valid for procedural brushes and
  image-generating filters.
- `ShaderInput::Current`: only read the current output pixel's input. Filters
  still receive separate immutable input and output textures.
- `ShaderInput::Sampled { radius }`: maximum per-axis logical displacement,
  finite and nonnegative. Input damage/capture expands by the transformed radius
  plus the bilinear footprint. It does not enlarge output geometry.

An ordinary Filter reads its isolated layer. A Backdrop Filter reads earlier
painter-order background. Both support chains, graphs and retained scenes.
Input-reading filters render to scratch, then copy the affected region back;
input-free filters can write the destination directly. Backdrop capture includes
the entire input halo. Retained backdrop parameter replacement rebuilds that
halo before filtering, excluding later foreground. Permanent regression tests
cover both previously reproduced source-history bugs.

A `ProgramBrush` runs its complete kernel over the clipped draw rectangle into
a GPU texture, then the existing renderer composites that texture with path/SDF
coverage and clipping. This adds a raster prepass and texture sampling. The
texture covers the draw bounds rather than the whole canvas, and shader source
never becomes GPU paint data. Distinct brush instances/draw transforms use
existing image-table slots (64 minus existing standalone image textures).
Resource capacity errors are explicit. Native shader compilation does not remove
these scheduling or texture bandwidth costs.

Pipeline identity includes source bytes, entry and resource ABI; uniforms,
coordinates and radius values are configuration data. Each backend bounds its
application pipeline cache and evicts only when no submitted frame is in flight.
DXC's disk cache additionally includes compiler identity and flags and validates
both cached and newly compiled interfaces. Failed Metal entries/interfaces do
not retain libraries outside the successful-pipeline cache limit; a regression
test reproduces and covers that lifetime bug. First use can stall for compilation.

## Examples, verification and limits

`program_brush` renders two custom paint instances; the right panel adds a
scanline filter. Neither panel is a built-in/native-versus-custom comparison.
`program_glass` uses the complete `glass.metal` / `glass.hlsl` files. The left
lens uses one bilinear sample; the right adds red/blue dispersion. Blur can be
composed as an independent filter.

Release tests run in one thread. Coverage includes raw-source preservation,
uniform size and bit patterns, invalid source/entry/backend errors, reflection,
DPI/append/offscreen coordinates, alpha, chains, graphs, retained replacements,
compact dispatch, transparent borders, allocation padding and backdrop halos.
Actual GPU execution is validated on Apple M5. DX12/Vulkan device execution
requires their respective platforms and the pinned DXC toolchain.

The existing whole-filter-layer rotation/shear sample-region placement issue in
`append_transformed` also affects ordinary built-in filters. This change does
not repair that separate geometry issue. Brush affine transforms and filter
DPI/translation/offscreen coordinates have focused regressions.

See [glass measurements](program-glass-comparison.md) for the current same-source
runtime/offline comparison, whole-renderer costs and historical interpreter data.

For multiple inputs, writable buffers, floating-point intermediates and explicit
multipass scheduling, use [native shader pipelines](shader-pipelines.md).
