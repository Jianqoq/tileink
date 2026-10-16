use super::*;

// Native backing pixels are authoritative even when their logical extent is fractional.
#[test]
fn native_surface_keeps_exact_backing_pixels_and_fractional_logical_size() {
    for physical in [(1, 1), (13, 7), (1471, 1419)] {
        for scale in [1.1, 1.25, 1.5, 2.0, 4.0] {
            let surface = CanvasSurface::from_physical(physical, scale).unwrap();
            let mut canvas = Canvas::with_surface(surface);
            assert_eq!(canvas.physical_size(), physical);
            assert_eq!(
                canvas.logical_size(),
                (
                    f64::from(physical.0) / f64::from(scale),
                    f64::from(physical.1) / f64::from(scale)
                )
            );
            canvas.reset_to_surface(surface);
            assert_eq!(canvas.surface(), surface);
        }
    }
}

#[test]
fn authored_surface_keeps_its_logical_extent_when_backing_allocation_rounds_up() {
    let canvas = Canvas::new(1, 3, 1.5);
    assert_eq!(canvas.logical_size(), (1.0, 3.0));
    assert_eq!(canvas.physical_size(), (2, 5));
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(CanvasSurface::from_physical((1, 1), scale).is_err());
        assert!(CanvasSurface::from_logical((1, 1), scale).is_err());
    }
    assert!(CanvasSurface::from_logical((u32::MAX, 1), 2.0).is_err());
}

#[test]
fn retained_surface_resize_and_rollback_preserve_native_extent() {
    let surface = CanvasSurface::from_physical((13, 7), 2.0).unwrap();
    let root = RetainedNodeId::new(1, 0);
    let mut scene = crate::RetainedScene::with_surface(surface, root).unwrap();
    assert_eq!(scene.physical_size(), (13, 7));
    assert_eq!(scene.logical_size(), (6.5, 3.5));
    let mut materializer = crate::retained_scene::PersistentSceneMaterializer::new(&scene);
    assert_eq!(materializer.canvas().surface(), surface);
    let smaller = CanvasSurface::from_physical((1, 1), 2.0).unwrap();
    let mut transaction = scene.transaction();
    transaction.set_surface(smaller);
    transaction.remove_subtree(RetainedNodeId::new(2, 0));
    assert!(transaction.commit().is_err());
    assert_eq!(scene.surface(), surface);
    scene.transaction().set_surface(smaller).commit().unwrap();
    assert_eq!(scene.surface(), smaller);
    materializer.update(&scene, None);
    assert_eq!(materializer.canvas().surface(), smaller);
}
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
