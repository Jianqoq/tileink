---
sidebar_position: 4
title: Images and brushes
---

# Images and brushes

Create images with `Image::new`, `from_rgba8`, or `from_premultiplied_rgba8`. Direct draws use `Canvas::push_image`; shared retained resources use `ImageKey`, `renderer.insert_image`, and `push_image_key`.

APIs accepting `impl Into<Brush>` accept a `Color` directly. Other brushes include linear, radial, sweep, four-corner, mesh, and image patterns. `PatternSampling` controls sampling and `PatternBrush::from_image_key*` selects explicit or natural sizing.

Replacing an image registry entry invalidates only the required resource tables and retained history; unrelated scene chunks remain intact.

SVG patterns and nested SVG images remain vector subscenes rendered on the GPU before image sampling. Standalone textures bind the child output directly, and the cache owns its GPU storage without a full transparent upload or image copy. Atlas images still receive a one-pixel border for correct edge sampling.

Sweep gradients accept finite start/end angles in radians. Start orientation wraps by full turns while the original signed span is retained, including reversed and multi-turn sweeps. `Brush::from_gradient*` panics for nonfinite angles. Zero or very small spans use the ramp start.

## Mesh Gradient

`MeshGradient` takes row-major position and color arrays for a grid of at least 2×2 vertices. Each cell is a straight-sided convex quad with movable vertices. Construction rejects folded, degenerate, inconsistently wound, or self-intersecting meshes; either consistent winding direction is accepted.

Colors interpolate in premultiplied sRGB. The default `Smooth` mode uses cubic smoothstep weights in both mesh parameters; `MeshInterpolation::Bilinear` is also available. Colors are continuous at shared edges; spatial derivatives need not be continuous on distorted meshes. The brush is transparent outside its boundary, so cover the painted shape with the mesh. Bézier boundary handles and SVG mesh parsing are not provided.

```rust
use tileink::{MeshGradient, Radius};
use peniko::{Color, kurbo::Rect};

let mesh = MeshGradient::new(
    3, 3,
    [[0., 0.], [150., 0.], [300., 0.],
     [0., 100.], [190., 70.], [300., 100.],
     [0., 200.], [150., 200.], [300., 200.]],
    [Color::from_rgb8(40, 60, 180), Color::from_rgb8(230, 70, 150), Color::WHITE,
     Color::from_rgb8(20, 190, 210), Color::from_rgb8(130, 70, 240), Color::from_rgb8(255, 100, 100),
     Color::BLACK, Color::from_rgb8(60, 140, 220), Color::from_rgb8(230, 110, 160)],
)?;
canvas.push_rect(Rect::new(0., 0., 300., 200.), Radius::all(20.), mesh);
```

Native Metal/HLSL share the brush layout and spatial bins. Mesh brushes work with paths, SDFs, filter brushes, coordinate conversion, and retained updates. Run `cargo run --release --no-default-features --features metal --example mesh_gradient` to render the example PNG.


## Programmable Brush and Filter

`ShaderProgram::new` compiles complete application-provided Metal/HLSL source verbatim, using the specified entry. There is no expression DSL, code generator or GPU interpreter. `brush(uniforms)` and `filter(uniforms)` instantiate paints/effects; uniforms are raw `u32` words, with floats encoded using `f32::to_bits`. Brushes use `with_uniforms`; `filter(uniforms)` returns an ordinary `ShaderPipeline`, whose parameter buffer can be updated with `with_resource(3, ShaderResource::Buffer(words.into()))`. `Filter::Program` and `ProgramFilter` have been removed.

```rust
use tileink::{ShaderProgram, ShaderSources, ShaderInput};
let program = ShaderProgram::new(
    ShaderSources {
        metal: Some(include_str!("glass.metal").into()),
        hlsl: Some(include_str!("glass.hlsl").into()),
    },
    "glass", ShaderInput::Sampled { radius: 32.0 }, 29,
)?;
let filter = program.filter(uniform_words)?;
canvas.push_backdrop_layer(filter.into(), region);
canvas.pop_layer();
```

Provide source for each supported backend. Missing source, compilation and reflected interface errors fail rendering explicitly. Complete ABI declarations and pixel/sampling helpers are application-owned files in `examples/shaders/`; see `docs/native/program-brush-filter.md`. Workgroups have 256 lanes. The public `ShaderConfig` supplies output bounds, coordinate mapping, compact tiles and sampling declarations. The shader owns bounds checks, sampling and premultiplied sRGB output.

Container, reflection-process and interface validation errors on DXC cache hits propagate without silently recompiling. Truncated or checksum-invalid cache entries remain cache misses and compile the same source again.

`ShaderInput::None` has no input and supports brushes; `Current` reads only the current input pixel; `Sampled { radius }` declares the maximum logical sampling displacement for backdrop capture and retained damage. The declaration does not rewrite source. Ordinary filters read their layer; backdrop filters read earlier background. Input-reading filters use a distinct output texture and region copy-back, including in chains/graphs.

`ShaderProgram::filter()` constructs an ordinary `ShaderPipeline` using the fixed single-pass ABI. Use `ShaderPipeline` below for extra resources, floating-point intermediates and explicit scheduling. Both compile application source without an interpreter. Source versions are Metal 2.4 / HLSL `cs_6_0`; they do not expose every graphics API capability.

Brush kernels first rasterize their draw rectangle on the GPU, then the existing path/SDF/clip compositor samples the result. This has texture and prepass costs. Shader source is never uploaded as GPU instructions. `program_brush` shows procedural paint and scanlines; both panels are custom. `program_glass` shows a basic lens on the left and RGB dispersion on the right.

Existing whole-filter-layer rotation/shear sample-region placement in `append_transformed` can clip built-in and programmable filters. Brush affine transforms and filter DPI/translation/offscreen coordinates have regression tests.

### Multipass native ShaderPipeline

`ShaderPipeline` accepts complete application Metal/HLSL source plus explicit resources, bindings and passes. It supports multiple input images, samplers, read/write buffers, RGBA8/RGBA16F/RGBA32F intermediates, in-place texture access, custom workgroups and multiresolution dispatch. Tileink pools resources and synchronizes passes without CPU readbacks.

All built-in filter GPU stages use the public `ShaderKernel::precompiled`, `ShaderPipeline::new`, `ShaderResource::External` and `ShaderPass` constructors, including progressive blur and liquid glass. Existing `Filter` APIs retain their build-compiled Metal/HLSL kernels and bind already-resident GPU resources. Compact tiles, shared memory and multiresolution optimizations remain, without extra pixel copies or per-frame shader compilation.

`ShaderKernel::new` declares source, entry, workgroup and bindings; `ShaderPipeline::new` declares resources and execution order. `with_resource` replaces an immutable parameter/image snapshot. Input dependencies are global by default; `with_input_radius` declares the complete pipeline's maximum logical displacement to narrow backdrop capture and retained damage. Include every pass and interpolation footprint.

`with_output_outset(outset)` independently declares logical output growth on every side for shadows, blurs and offsets. It controls output bounds, offscreen surfaces and retained damage. DPI scales both output growth and the input footprint. These are scheduling contracts; application source still owns all shader math.

`ShaderResource::Input(index)` declares a dynamic image input; indices must be unique and contiguous from zero. Standalone filters bind their layer/backdrop at Input(0). A `FilterPrimitiveKind::ShaderPipeline { pipeline, inputs }` graph node binds ordered SourceGraphic, SourceAlpha or preceding Primitive(index) results directly as GPU textures, including repeated edges. No CPU readback/upload is required. The vector must have `pipeline.input_count()` entries; missing/forward references fail rendering. This variant ignores the shared input/input2 fields and always clips output to its explicit primitive region.

`examples/shader_graph.rs` demonstrates SourceAlpha → offset → blur followed by a user shader compositing SourceGraphic with the GPU intermediate. Complete source is in `examples/shaders/composite.metal` and `.hlsl`.

`NativeShaderBatch` exposes GPU buffer/texture/array/table/sampler creation and zero-copy `NativeTexture` import. Bind handles in External index order with `record`, then `submit`. It shares pipeline descriptors, validation and execution with Canvas filters; foreign handles, type/format mismatches and writable aliases are rejected. `ShaderBindingKind::Uniform(size)` supports arbitrary constant blocks; `TextureArray` and `TextureTable(count)` expose arrays and descriptor tables.

`batch.sampler(ShaderSampler::Linear)` returns a `NativeShaderResource` directly, without `?`. Recording stores an immutable sampler descriptor; native allocation happens during `submit`, which reports allocation failures.

Resource slots are 0..=29; texture tables may also use slot30, samplers 0..=15. Optional Config uses ShaderConfig at slot0. Initial texture writers must initialize all subsequently read texels; buffers reset from their initial data on every invocation. Final output is premultiplied-sRGB RGBA8, while floating-point intermediates retain negatives and HDR.

See `examples/shader_pipeline.rs` and `examples/common/shader_blur.rs`: four application kernels downsample, convolve horizontally/vertically, and upsample with an additional tint image.
