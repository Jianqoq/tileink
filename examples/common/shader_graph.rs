//! User source composites graph GPU results; kernels have no graph-specific code.
use tileink::*;
pub fn composite() -> Result<ShaderPipeline, ShaderError> {
    use ShaderBindingKind as K;
    ShaderPipeline::new(
        vec![
            ShaderResource::Input(0),
            ShaderResource::Input(1),
            ShaderResource::Output,
            ShaderResource::config(ShaderExtent::FULL),
        ],
        vec![ShaderPass {
            kernel: ShaderKernel::new(
                ShaderSources {
                    metal: Some(include_str!("../shaders/composite.metal").into()),
                    hlsl: Some(include_str!("../shaders/composite.hlsl").into()),
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
    )?
    .with_input_radius(0.0)
}
pub fn shadow_graph(width: u32, height: u32, custom: bool) -> Result<Filter, ShaderError> {
    use FilterInput as I;
    let bounds = Bounds::canvas(width, height);
    let primitive = |input, input2, kind| FilterPrimitive {
        input,
        input2,
        region: bounds,
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
                        pipeline: composite()?,
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
