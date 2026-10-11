---
sidebar_position: 5
title: Paint, geometry, and filters
---

# Paint, geometry, and filters

`Brush` supports solid, linear, radial, sweep, four-corner, mesh, programmable, and pattern paint. `Brush::from_image_key` maps a renderer image into a target rect; `Brush::from_image_key_with_options` also controls extend, sampling, and opacity; `Brush::from_image_key_natural` maps natural image pixels from an origin. `PatternBrush::for_origin_resource` exposes the corresponding 1:1 resource brush. Zero-sized images or invalid target rects return `None`. Gradient helpers are `Brush::from_gradient`, `Brush::from_gradient_with_ramp_size`, and `Brush::four_corner`. The default gradient ramp size is 4096.

`MeshGradient::new(columns, rows, positions, colors)` validates and compiles row-major quad meshes. Pass the result directly to drawing APIs, or choose `MeshInterpolation::Bilinear` with `with_interpolation`; the default is `Smooth`. See [Images and brushes](../guides/images-and-brushes.md#mesh-gradient) for semantics and an example.

`ShaderProgram::new(sources, entry, input, uniform_words)` compiles complete user-provided Metal/HLSL source verbatim. `brush(uniforms)` and `filter(uniforms)` create instances; `with_uniforms` updates raw words without recompilation. `ShaderInput::{None, Current, Sampled { radius }}` declares immutable input dependencies. The shader owns sampling, bounds checks and premultiplied sRGB output. See [Images and brushes](../guides/images-and-brushes.md#programmable-brush-and-filter).

## Bounds

`Bounds::new(x0, y0, x1, y1)` creates an integer pixel extent and `Bounds::canvas(width, height)` creates a full-canvas extent. `intersect`, `union`, and `outset` compute set operations; `is_empty` detects an empty half-open range.

`Image` constructors are `new`, `from_rgba8`, and `from_premultiplied_rgba8`; read with `rgba8_at`/`rgba8_bytes` and write with `save`. `ImageKey::new` creates registry identity.

`Sdf` represents Rect, Circle, strokes, CandleStick, Line, DashLine, Arc, and Triangle; `SdfShadow` contains analytic shadow counterparts. Both expose `bounds`. Helpers include `Radius::all`, `StrokeWidths::all`, `ShadowOptions::new`, `SdfLine::new`, `SdfDashLine::new`, `SdfDashLine::with_offset`, `SdfArc::new`, `SdfTriangle::new`, and `CandleStick::new` plus width validators. A triangle's `corner_radius` is a uniform analytic expansion of its three edges, producing rounded vertices without path tessellation.

Rectangle, circle, arc, line and callout shadows all use `ShadowOptions` for their offset, falloff distance and intensity.

`Region::rect` and `Region::path` define filter/mask sampling regions. `Filter` covers blur, color transforms, shadows, convolution, morphology, displacement, turbulence, lighting, composite/blend, primitive graphs, and `RectLiquidGlass`; related public parameter types are re-exported from the crate root. `BlurSampling::downsampled(factor)` selects the reduced-resolution blur path; factors below one are normalized internally when evaluated.

`ShaderKernel::new` and `ShaderPipeline::new` expose native resource bindings, writable buffers, floating-point textures and explicit multipass/multiresolution dispatch. Use `with_resource` for snapshot updates and `with_input_radius` for a bounded aggregate read footprint. See the shader pipeline section in [Images and brushes](../guides/images-and-brushes.md#multipass-native-shaderpipeline).

`ShaderKernel::precompiled` loads build-compiled kernels through the same public interface. `ShaderResource::Uniform` describes arbitrary native constant blocks, and typed `External` descriptors bind existing GPU resources per invocation. `NativeShaderBatch::record` and `submit` expose this invocation path, including texture arrays and descriptor tables.

`with_output_outset` declares logical output growth independently of the read radius, including DPI, offscreen allocation and retained damage. `ShaderResource::Input(index)` numbers dynamic GPU inputs from zero; `input_count()` reports the required count. `FilterPrimitiveKind::ShaderPipeline { pipeline, inputs }` binds ordered SourceGraphic, SourceAlpha and preceding graph-node results directly, with explicit primitive-region clipping.
