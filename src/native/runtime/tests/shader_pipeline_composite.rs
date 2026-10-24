use super::*;
pub(super) fn filter(custom: bool) -> Result<Filter> {
    use FilterInput as I;
    let pipeline = ShaderPipeline::new(
        vec![
            ShaderResource::Input(0),
            ShaderResource::Input(1),
            ShaderResource::Output,
            ShaderResource::config(ShaderExtent::FULL),
        ],
        vec![ShaderPass {
            kernel: ShaderKernel::new(
                ShaderSources {
                    metal: Some(
                        include_str!("../../../../examples/shaders/composite.metal").into(),
                    ),
                    hlsl: Some(include_str!("../../../../examples/shaders/composite.hlsl").into()),
                },
                "composite",
                [8, 8, 1],
                [
                    (0, "config", K::Config),
                    (1, "source", K::Texture),
                    (2, "backdrop", K::Texture),
                    (
                        3,
                        "output_image",
                        K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                    ),
                ]
                .into_iter()
                .map(|(slot, name, kind)| ShaderBinding {
                    slot,
                    name: name.into(),
                    kind,
                })
                .collect(),
            )?,
            bindings: vec![(0, 3), (1, 0), (2, 1), (3, 2)],
            dispatch: ShaderDispatch::Region {
                extent: ShaderExtent::FULL,
                outset: 0,
            },
        }],
    )?;
    let primitive = |input, input2, kind| FilterPrimitive {
        input,
        input2,
        region: Bounds::canvas(128, 80),
        linear_rgb: false,
        kind,
    };
    Ok(Filter::Graph {
        fixed_region: true,
        primitives: vec![
            primitive(
                I::SourceAlpha,
                None,
                FilterPrimitiveKind::Filter(Box::new(Filter::Offset { dx: 14.0, dy: 16.0 })),
            ),
            primitive(
                I::Primitive(0),
                None,
                FilterPrimitiveKind::Filter(Box::new(Filter::Blur {
                    std_dev_x: 12.0,
                    std_dev_y: 12.0,
                    sampling: BlurSampling::FULL_RES,
                })),
            ),
            primitive(
                I::SourceGraphic,
                Some(I::Primitive(1)),
                if custom {
                    FilterPrimitiveKind::ShaderPipeline {
                        pipeline,
                        inputs: vec![I::SourceGraphic, I::Primitive(1)],
                    }
                } else {
                    FilterPrimitiveKind::Composite {
                        operator: CompositeOperator::Over,
                    }
                },
            ),
        ],
    })
}
