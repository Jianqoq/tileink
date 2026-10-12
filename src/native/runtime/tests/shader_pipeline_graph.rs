//! Dynamic graph edges and output growth exercise the actual layer renderer.
use super::*;
use ShaderBindingKind as K;
#[path = "shader_pipeline_composite.rs"]
mod composite;
#[path = "shader_pipeline_graph_retained.rs"]
mod retained;
fn kernel(entry: &str, bindings: &[(u32, &str, K)]) -> Result<ShaderKernel> {
    Ok(ShaderKernel::new(
        ShaderSources {
            metal: Some(include_str!("shaders/graph.metal").into()),
            hlsl: Some(include_str!("shaders/graph.hlsl").into()),
        },
        entry,
        [8, 8, 1],
        bindings
            .iter()
            .map(|&(slot, name, kind)| ShaderBinding {
                slot,
                name: name.into(),
                kind,
            })
            .collect(),
    )?)
}
pub(super) fn spatial_pipeline(entry: &str, dx: f32, dy: f32) -> Result<ShaderPipeline> {
    Ok(ShaderPipeline::new(
        vec![
            ShaderResource::Input(0),
            ShaderResource::Output,
            ShaderResource::config(ShaderExtent::FULL),
            ShaderResource::Buffer(Rc::from([dx.to_bits(), dy.to_bits()])),
        ],
        vec![ShaderPass {
            kernel: kernel(
                entry,
                &[
                    (0, "config", K::Config),
                    (1, "input_image", K::Texture),
                    (
                        3,
                        "output_image",
                        K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                    ),
                    (4, "parameters", K::ReadBuffer),
                ],
            )?,
            bindings: vec![(0, 2), (1, 0), (3, 1), (4, 3)],
            dispatch: ShaderDispatch::Region {
                extent: ShaderExtent::FULL,
                outset: 0,
            },
        }],
    )?
    .with_input_radius(dx.abs().max(dy.abs()))?
    .with_output_outset(dx.abs().max(dy.abs()))?)
}
pub(super) fn channels_pipeline() -> Result<ShaderPipeline> {
    Ok(ShaderPipeline::new(
        vec![
            ShaderResource::Input(1),
            ShaderResource::Input(2),
            ShaderResource::Input(0),
            ShaderResource::Output,
            ShaderResource::config(ShaderExtent::FULL),
        ],
        vec![ShaderPass {
            kernel: kernel(
                "graph_channels",
                &[
                    (0, "config", K::Config),
                    (1, "first", K::Texture),
                    (2, "second", K::Texture),
                    (5, "third", K::Texture),
                    (
                        3,
                        "output_image",
                        K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                    ),
                ],
            )?,
            bindings: vec![(0, 4), (1, 2), (2, 0), (5, 1), (3, 3)],
            dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
        }],
    )?)
}
pub(super) fn graph_filter(pipeline: ShaderPipeline, duplicate: bool) -> Filter {
    use FilterInput as I;
    let primitive = |region, kind| FilterPrimitive {
        input: I::SourceGraphic,
        input2: None,
        region,
        linear_rgb: false,
        kind,
    };
    Filter::Graph {
        fixed_region: true,
        primitives: vec![
            primitive(
                Bounds::new(2, 0, 13, 17),
                FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                    brush: Color::from_rgb8(200, 77, 0).into(),
                })),
            ),
            primitive(
                Bounds::new(0, 5, 33, 15),
                FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                    brush: Color::from_rgb8(30, 150, 0).into(),
                })),
            ),
            primitive(
                Bounds::new(3, 2, 30, 16),
                FilterPrimitiveKind::ShaderPipeline {
                    pipeline,
                    inputs: vec![
                        I::Primitive(0),
                        I::Primitive(usize::from(!duplicate)),
                        I::SourceAlpha,
                    ],
                },
            ),
        ],
    }
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_graph_binds_ordered_dynamic_inputs_and_crops_outputs() -> Result<()> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 33, 19)?;
    for duplicate in [false, true, false] {
        let mut canvas = Canvas::new(33, 19, 1.0);
        canvas.push_filter_layer(
            graph_filter(channels_pipeline()?, duplicate),
            Region::rect(Rect::new(0.0, 0.0, 33.0, 19.0), Radius::ZERO),
        );
        canvas.push_rect(
            Rect::new(0.0, 0.0, 33.0, 19.0),
            Radius::ZERO,
            Color::from_rgba8(50, 60, 70, 128),
        );
        canvas.pop_layer();
        let image = renderer.render_to_image(&canvas)?.readback()?;
        for y in 0..19 {
            for x in 0..33 {
                let expected = if (3..30).contains(&x) && (2..16).contains(&y) {
                    u32::from_le_bytes([
                        if x < 13 { 200 } else { 0 },
                        if duplicate {
                            if x < 13 { 77 } else { 0 }
                        } else if (5..15).contains(&y) {
                            150
                        } else {
                            0
                        },
                        128,
                        255,
                    ])
                } else {
                    0
                };
                assert_eq!(
                    image.pixels[y * 33 + x],
                    expected,
                    "x={x},y={y},duplicate={duplicate}"
                );
            }
        }
    }
    context.check_validation()?;
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_output_growth_matches_offset_and_shadow_at_multiple_dpis() -> Result<()> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 96, 64)?;
    for dpi in [1.0, 1.5, 2.0] {
        for origin in [(8.0, 8.0), (-2.0, 2.0), (40.0, 22.0)] {
            for shadow in [false, true] {
                let rect = Rect::new(origin.0, origin.1, origin.0 + 8.0, origin.1 + 6.0);
                let build = |filter| {
                    let mut c = Canvas::new(96, 64, dpi);
                    c.push_filter_layer(filter, Region::rect(rect, Radius::ZERO));
                    c.push_rect(rect, Radius::ZERO, Color::from_rgba8(180, 80, 31, 128));
                    c.pop_layer();
                    c
                };
                let native = if shadow {
                    Filter::DropShadow {
                        offset_x: 4.0,
                        offset_y: -2.0,
                        std_dev: 0.0,
                        brush: Color::BLACK.into(),
                    }
                } else {
                    Filter::Offset { dx: 4.0, dy: -2.0 }
                };
                let custom = spatial_pipeline(
                    if shadow {
                        "shadow_image"
                    } else {
                        "offset_image"
                    },
                    4.0,
                    -2.0,
                )?;
                let expected = renderer.render_to_image(&build(native))?.readback()?;
                let actual = renderer
                    .render_to_image(&build(custom.into()))?
                    .readback()?;
                assert_eq!(
                    actual.pixels, expected.pixels,
                    "dpi={dpi},origin={origin:?},shadow={shadow}"
                );
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_full_span_output_growth_remains_visible_and_reuses_renderer() -> Result<()>
{
    let context = context()?;
    let program = ShaderProgram::new(
        ShaderSources {
            metal: Some(include_str!("../../../../examples/shaders/paint.metal").into()),
            hlsl: Some(include_str!("../../../../examples/shaders/paint.hlsl").into()),
        },
        "paint",
        ShaderInput::None,
        8,
    )?;
    let filter = program
        .filter([0.0f32, 0.0, 0.0, 0.0, 0.8, 0.8, 0.8, 0.8].map(f32::to_bits))?
        .with_output_outset(f32::MAX)?;
    let mut renderer = NativeRenderer::with_context(&context, 33, 19)?;
    let mut expected = None;
    for rect in [
        Rect::new(0.0, 0.0, 33.0, 19.0),
        Rect::new(-3.0e9, -3.0e9, -3.0e9 + 512.0, -3.0e9 + 512.0),
        Rect::new(0.0, 0.0, 33.0, 19.0),
    ] {
        let mut canvas = Canvas::new(33, 19, 1.0);
        canvas.push_filter_layer(filter.clone().into(), Region::rect(rect, Radius::ZERO));
        canvas.push_rect(rect, Radius::ZERO, Color::WHITE);
        canvas.pop_layer();
        let image = renderer.render_to_image(&canvas)?.readback()?;
        assert!(image.pixels.iter().all(|pixel| *pixel != 0));
        if let Some(expected) = &expected {
            assert_eq!(&image.pixels, expected);
        } else {
            expected = Some(image.pixels);
        }
    }
    context.check_validation()?;
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_graph_composite_matches_builtin_and_nested_graphs() -> Result<()> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 128, 80)?;
    for nested in [false, true] {
        let build = |custom| -> Result<_> {
            let mut filter = composite::filter(custom)?;
            if nested {
                filter = Filter::Graph {
                    fixed_region: true,
                    primitives: vec![FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::canvas(128, 80),
                        linear_rgb: false,
                        kind: FilterPrimitiveKind::Filter(Box::new(filter)),
                    }],
                };
            }
            let mut c = Canvas::new(128, 80, 1.0);
            c.push_filter_layer(
                filter,
                Region::rect(Rect::new(0.0, 0.0, 128.0, 80.0), Radius::ZERO),
            );
            c.push_rect(
                Rect::new(16.0, 12.0, 84.0, 52.0),
                Radius::all(9.0),
                Color::from_rgba8(180, 80, 31, 128),
            );
            c.pop_layer();
            Ok(c)
        };
        let expected = renderer.render_to_image(&build(false)?)?.readback()?;
        let actual = renderer.render_to_image(&build(true)?)?.readback()?;
        assert_eq!(actual.pixels, expected.pixels, "nested={nested}");
    }
    context.check_validation()?;
    Ok(())
}
