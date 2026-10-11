use super::*;
use crate::{
    ShaderBinding, ShaderBindingKind, ShaderDispatch, ShaderExtent, ShaderKernel, ShaderPass,
    ShaderPipeline, ShaderResource, ShaderSources, ShaderTextureFormat,
};

#[test]
fn shader_pipeline_physical_coordinates_and_read_footprint_follow_dpi() {
    let pipeline = ShaderPipeline::new(
        vec![ShaderResource::Output],
        vec![ShaderPass {
            kernel: ShaderKernel::new(
                ShaderSources {
                    metal: Some("native".into()),
                    hlsl: None,
                },
                "main",
                [1; 3],
                vec![ShaderBinding {
                    name: "output".into(),
                    slot: 3,
                    kind: ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                }],
            )
            .unwrap(),
            bindings: vec![(3, 0)],
            dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
        }],
    )
    .unwrap()
    .with_input_radius(3.0)
    .unwrap();
    let canvas = Canvas::new(64, 48, 2.0);
    let Filter::ShaderPipeline(physical) = canvas.physical_filter(pipeline.clone().into()) else {
        unreachable!()
    };
    assert_eq!(physical.transform, [0.5, 0.0, 0.0, 0.5, 0.0, 0.0]);
    assert_eq!(physical.sample_outset(), Some(7));
    assert_eq!(physical.with_output_outset(3.0).unwrap().output_outset(), 6);
    let kind = FilterPrimitiveKind::ShaderPipeline {
        pipeline,
        inputs: vec![],
    };
    let FilterPrimitiveKind::ShaderPipeline { pipeline, .. } =
        canvas.physical_filter_primitive_kind(kind)
    else {
        unreachable!()
    };
    assert_eq!(pipeline.sample_outset(), Some(7));
}
