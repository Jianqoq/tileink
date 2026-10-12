//! Context transitions are checked independently from submission. Failed
//! recording still invalidates the owning batch; these tests never submit it.
use super::super::{FrameResources, SceneState, filter_scenes};
use super::*;
use crate::native::runtime::renderer::RetainedRenderState;
use crate::render::{draw_batches::DrawBatchAdapter, filters::FilterAdapter};
use crate::shared::layer::filter::{ConvolveEdgeMode, ConvolveMatrix, Filter};
use std::rc::Rc;

fn with_execution(
    canvas: &Canvas,
    run: impl FnOnce(&mut Execution<'_>) -> Result<()>,
) -> Result<()> {
    let context = crate::native::runtime::gpu_support::context()?;
    let mut batch = crate::native::runtime::gpu_support::batch(&context);
    let empty = crate::shared::image_resource::ImageResourceStore::default();
    let upload = empty.upload_merged(&empty, 4096, 4, 16, None);
    let images = Images::record(&mut batch, &upload)?;
    let mut cache = SceneCache::default();
    let mut retained = RetainedRenderState::default();
    let mut filter_scenes = filter_scenes::FilterSceneCache::default();
    let mut execution = Execution::prepare(
        cache.prepare(canvas),
        &mut batch,
        FrameResources {
            filter_scenes: filter_scenes.frame(),
            images: &images,
            text: None,
            retained: &mut retained,
        },
        FrameOptions::default(),
        65535,
    )?;
    run(&mut execution)
}

fn record_draw(
    execution: &mut Execution<'_>,
    target: crate::native::runtime::compute::ResourceId,
) -> Result<()> {
    let before = execution.batch.passes().len();
    execution.draw(0..1, 0..0, RenderTargetId::Main)?;
    assert!(execution.batch.passes().len() > before);
    let fine = execution.batch.passes().last().unwrap();
    assert_eq!(fine.shader.entry, "fine_tile_main");
    assert_eq!(
        fine.bindings
            .iter()
            .find(|(binding, _)| binding.slot == 1)
            .unwrap()
            .1,
        target
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn local_filter_context_restores_parent_after_scan_failure() -> Result<()> {
    let canvas = Canvas::new(4, 4, 1.0);
    with_execution(&canvas, |execution| {
        let parent = execution.targets.get(RenderTargetId::Main)?.image();
        let plan = execution.scene.scanned().plan_handle();
        let state = execution.begin_filter_scene(
            &canvas,
            &plan,
            &Filter::Opacity(0.5),
            2,
            (7, 9),
            false,
        )?;
        assert!(
            matches!(execution.scene, SceneState::Pending(_)),
            "scan must be recorded at its shared scheduling boundary"
        );
        assert_ne!(execution.targets.get(RenderTargetId::Main)?.image(), parent);
        execution.limit = 0;
        assert!(execution.scan_filter_scene(&canvas).is_err());
        execution.end_filter_scene(state);
        assert_eq!(execution.origin, (0, 0));
        assert_eq!(execution.targets.get(RenderTargetId::Main)?.image(), parent);
        assert!(matches!(execution.scene, SceneState::Scanned(_)));
        Ok(())
    })
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn failed_filter_context_preparation_keeps_parent_active() -> Result<()> {
    let canvas = Canvas::new(4, 4, 1.0);
    with_execution(&canvas, |execution| {
        let parent = execution.targets.get(RenderTargetId::Main)?.image();
        let plan = execution.scene.scanned().plan_handle();
        let invalid = Filter::ConvolveMatrix(ConvolveMatrix {
            columns: 1,
            rows: 1,
            target_x: 0,
            target_y: 0,
            data: vec![f32::NAN],
            divisor: 1.0,
            bias: 0.0,
            edge_mode: ConvolveEdgeMode::None,
            preserve_alpha: false,
        });
        assert!(
            execution
                .begin_filter_scene(&canvas, &plan, &invalid, 2, (7, 9), false)
                .is_err()
        );
        assert_eq!(execution.origin, (0, 0));
        assert_eq!(execution.targets.get(RenderTargetId::Main)?.image(), parent);
        assert!(Rc::ptr_eq(&execution.scene.scanned().plan_handle(), &plan));
        Ok(())
    })
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_filter_contexts_restore_each_parent_scene_before_recording_more_draws() -> Result<()> {
    let mut canvas = Canvas::new(4, 4, 1.0);
    canvas.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, 4.0, 4.0),
        crate::Radius::ZERO,
        peniko::Color::WHITE,
    );
    for fail_inner in [false, true] {
        with_execution(&canvas, |execution| {
            let root_plan = execution.scene.scanned().plan_handle();
            let root_target = execution.targets.get(RenderTargetId::Main)?.image();
            let outer = execution.begin_filter_scene(
                &canvas,
                &root_plan,
                &Filter::Opacity(0.5),
                0,
                (7, 9),
                false,
            )?;
            execution.scan_filter_scene(&canvas)?;
            let outer_plan = execution.scene.scanned().plan_handle();
            let outer_target = execution.targets.get(RenderTargetId::Main)?.image();
            let inner = execution.begin_filter_scene(
                &canvas,
                &outer_plan,
                &Filter::Opacity(0.25),
                0,
                (2, 3),
                false,
            )?;
            if fail_inner {
                execution.limit = 0;
                assert!(execution.scan_filter_scene(&canvas).is_err());
                execution.limit = 65535;
            } else {
                execution.scan_filter_scene(&canvas)?;
                let target = execution.targets.get(RenderTargetId::Main)?.image();
                record_draw(execution, target)?;
            }
            execution.end_filter_scene(inner);
            assert!(Rc::ptr_eq(
                &execution.scene.scanned().plan_handle(),
                &outer_plan
            ));
            assert_eq!(execution.origin, (7, 9));
            assert_eq!(
                execution.targets.get(RenderTargetId::Main)?.image(),
                outer_target
            );
            record_draw(execution, outer_target)?;
            execution.end_filter_scene(outer);
            assert!(Rc::ptr_eq(
                &execution.scene.scanned().plan_handle(),
                &root_plan
            ));
            assert_eq!(execution.origin, (0, 0));
            assert_eq!(
                execution.targets.get(RenderTargetId::Main)?.image(),
                root_target
            );
            record_draw(execution, root_target)
        })?;
    }
    Ok(())
}
