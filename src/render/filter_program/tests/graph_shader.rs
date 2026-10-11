use super::*;
use crate::{
    ShaderBinding, ShaderBindingKind as K, ShaderDispatch, ShaderExtent, ShaderKernel, ShaderPass,
    ShaderPipeline, ShaderResource, ShaderSources, ShaderTextureFormat,
};
fn pipeline(count: usize) -> ShaderPipeline {
    let mut resources: Vec<_> = (0..count).map(ShaderResource::Input).collect();
    resources.push(ShaderResource::Output);
    ShaderPipeline::new(
        resources,
        vec![ShaderPass {
            kernel: ShaderKernel::new(
                ShaderSources {
                    metal: Some("native".into()),
                    hlsl: None,
                },
                "main",
                [1; 3],
                vec![ShaderBinding {
                    slot: 3,
                    name: "output".into(),
                    kind: K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                }],
            )
            .unwrap(),
            bindings: vec![(3, count)],
            dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
        }],
    )
    .unwrap()
}
fn node(inputs: Vec<filter_model::FilterInput>) -> filter_model::FilterPrimitive {
    let pipeline = pipeline(inputs.len());
    let mut node = primitive(
        filter_model::FilterInput::SourceGraphic,
        filter_model::FilterPrimitiveKind::ShaderPipeline { pipeline, inputs },
    );
    node.region = Bounds::new(7, 5, 29, 23);
    node
}
#[test]
fn graph_scratch_counts_source_alpha_in_merge_input_vectors() {
    let filter = graph(vec![primitive(
        filter_model::FilterInput::SourceGraphic,
        filter_model::FilterPrimitiveKind::Merge {
            inputs: vec![filter_model::FilterInput::SourceAlpha],
        },
    )]);
    assert_eq!(crate::shared::gpu_plan::filter_scratch_extra(&filter), 2);
}
#[test]
fn shader_graph_checks_edges_and_counts_and_releases_all_failure_paths() {
    use filter_model::FilterInput as I;
    let filter = graph(vec![node(vec![
        I::SourceAlpha,
        I::SourceGraphic,
        I::SourceAlpha,
    ])]);
    let mut success = Recorder::default();
    assert!(run(&mut success, &filter));
    assert!(success.live.is_empty());
    assert_eq!(
        success
            .kernels
            .iter()
            .filter(|s| s.starts_with("SourceAlphaToTarget"))
            .count(),
        1
    );
    let kernel = success
        .kernels
        .iter()
        .find(|s| s.starts_with("ShaderPipelineToTarget"))
        .unwrap();
    assert!(kernel.contains("inputs: [Scratch(0), Main, Scratch(0)]"));
    assert!(
        success
            .kernels
            .iter()
            .any(|s| s.starts_with("CopyRegionToTarget") && s.contains("x0: 7"))
    );
    for failure in 1..=success.kernels.len() {
        let mut recorder = Recorder {
            fail_kernel: Some(failure),
            ..Default::default()
        };
        assert!(!run(&mut recorder, &filter));
        assert!(recorder.live.is_empty(), "kernel {failure}");
    }
    for failure in 0..success.next {
        let mut recorder = Recorder {
            fail_allocation: Some(failure),
            ..Default::default()
        };
        assert!(!run(&mut recorder, &filter));
        assert!(recorder.live.is_empty(), "allocation {failure}");
    }
    let invalid = graph(vec![node(vec![I::SourceAlpha, I::Primitive(0)])]);
    let mut recorder = Recorder::default();
    assert!(!run(&mut recorder, &invalid));
    assert!(recorder.live.is_empty());
    let mut mismatch = node(vec![I::SourceGraphic]);
    if let filter_model::FilterPrimitiveKind::ShaderPipeline { inputs, .. } = &mut mismatch.kind {
        inputs.clear();
    }
    let mut recorder = Recorder::default();
    assert!(!run(&mut recorder, &graph(vec![mismatch])));
    assert!(recorder.live.is_empty());
    let mut recorder = Recorder::default();
    assert!(!run(&mut recorder, &pipeline(2).into()));
    assert_eq!(recorder.next, 0);
    let mut recorder = Recorder::default();
    assert!(run(&mut recorder, &graph(vec![node(vec![])])));
    assert!(recorder.live.is_empty());
}
