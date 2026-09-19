use crate::render::output::RenderTargetId;
use crate::{
    Canvas,
    native::runtime::{
        Result,
        compute::ComputeBatch,
        program::scene::SceneCache,
        renderer::{Execution, Images},
    },
};

#[test]
fn frame_rejects_foreign_image_upload_before_recording() -> Result<()> {
    let mut owner = ComputeBatch::new();
    let upload = Default::default();
    let images = Images::record(&mut owner, &upload)?;
    let mut other = ComputeBatch::new();
    let canvas = Canvas::new(2, 2, 1.0);
    let error = Execution::record(
        &mut SceneCache::default(),
        &mut other,
        &canvas,
        &images,
        None,
        false,
        65535,
    )
    .unwrap_err();
    assert!(error.to_string().contains("another compute batch"));
    assert!(other.resources().is_empty());
    assert!(other.commands().is_empty());
    Ok(())
}

#[test]
fn local_filter_context_restores_parent_after_scan_failure() -> Result<()> {
    use crate::render::filters::FilterAdapter;
    use crate::shared::layer::filter::Filter;
    let mut batch = ComputeBatch::new();
    let upload = Default::default();
    let images = Images::record(&mut batch, &upload)?;
    let canvas = Canvas::new(4, 4, 1.0);
    let mut cache = SceneCache::default();
    let mut execution = Execution::prepare(
        cache.prepare(&canvas),
        &mut batch,
        &images,
        None,
        false,
        65535,
    )?;
    let parent = execution.targets.get(RenderTargetId::Main)?.image();
    let plan = execution.scene.as_ref().unwrap().plan_handle();
    let state =
        execution.begin_filter_scene(&canvas, &plan, &Filter::Opacity(0.5), 2, (7, 9), false)?;
    assert!(
        execution.scene.is_none(),
        "scan must be recorded at its shared scheduling boundary"
    );
    assert_ne!(execution.targets.get(RenderTargetId::Main)?.image(), parent);
    execution.limit = 0;
    assert!(execution.scan_filter_scene(&canvas).is_err());
    execution.end_filter_scene(state);
    assert_eq!(execution.origin, (0, 0));
    assert_eq!(execution.targets.get(RenderTargetId::Main)?.image(), parent);
    assert!(execution.scene.is_some());
    assert!(execution.pending_plan.is_none());
    Ok(())
}

#[test]
fn failed_filter_context_preparation_keeps_parent_active() -> Result<()> {
    use crate::render::filters::FilterAdapter;
    use crate::shared::layer::filter::{ConvolveEdgeMode, ConvolveMatrix, Filter};
    let mut batch = ComputeBatch::new();
    let upload = Default::default();
    let images = Images::record(&mut batch, &upload)?;
    let canvas = Canvas::new(4, 4, 1.0);
    let mut cache = SceneCache::default();
    let mut execution = Execution::prepare(
        cache.prepare(&canvas),
        &mut batch,
        &images,
        None,
        false,
        65535,
    )?;
    let parent = execution.targets.get(RenderTargetId::Main)?.image();
    let plan = execution.scene.as_ref().unwrap().plan_handle();
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
    assert!(execution.scene.is_some());
    assert!(execution.pending_plan.is_none());
    Ok(())
}
