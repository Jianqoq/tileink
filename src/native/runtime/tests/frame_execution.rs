use super::FrameOptions;

#[test]
fn direct_root_plan_has_no_offscreen_filter_resources() {
    use peniko::kurbo::{Rect, Shape};
    let rect = Rect::new(0.0, 0.0, 32.0, 32.0);
    let mut direct = crate::Canvas::new(32, 32, 1.0);
    direct.push_opacity_layer(rect.to_path(0.1), peniko::kurbo::Affine::IDENTITY, 0.1, 0.5);
    direct.push_rect(rect, crate::Radius::ZERO, peniko::Color::BLACK);
    direct.pop_layer();
    let plan = direct.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    assert!(!super::needs_offscreen_resources(&plan));

    let mut filtered = crate::Canvas::new(32, 32, 1.0);
    filtered.push_filter_layer(
        crate::Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 2.0,
            sampling: Default::default(),
        },
        crate::Region::rect(rect, crate::Radius::ZERO),
    );
    filtered.push_rect(rect, crate::Radius::ZERO, peniko::Color::BLACK);
    filtered.pop_layer();
    let plan = filtered.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    assert!(super::needs_offscreen_resources(&plan));
}
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
fn frame_records_directly_into_supplied_target_without_full_frame_copy() -> Result<()> {
    use crate::native::runtime::compute::Command;
    let mut batch = ComputeBatch::new();
    let target = batch.texture_rgba8([3, 2], vec![255; 24])?;
    let upload = Default::default();
    let images = Images::record(&mut batch, &upload)?;
    let canvas = Canvas::new(3, 2, 1.0);
    let output = Execution::record(
        &mut SceneCache::default(),
        &mut batch,
        &canvas,
        &images,
        None,
        FrameOptions {
            target: Some(target),
            clear_color: 0x80402010,
            ..Default::default()
        },
        65535,
    )?;
    assert_eq!(output, target);
    assert!(
        !batch
            .commands()
            .iter()
            .any(|command| matches!(command, Command::CopyTexture(_)))
    );
    assert!(
        batch
            .passes()
            .iter()
            .any(|pass| pass.shader.entry == "filter_clear_region")
    );
    Ok(())
}

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
        crate::native::runtime::renderer::FrameOptions {
            chunked: false,
            clear_color: 0,
            ..Default::default()
        },
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
    let mut retained = Default::default();
    let mut filter_scenes = super::filter_scenes::FilterSceneCache::default();
    let mut execution = Execution::prepare(
        cache.prepare(&canvas),
        &mut batch,
        super::FrameResources {
            filter_scenes: filter_scenes.frame(),
            images: &images,
            text: None,
            retained: &mut retained,
        },
        FrameOptions::default(),
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
    let mut retained = Default::default();
    let mut filter_scenes = super::filter_scenes::FilterSceneCache::default();
    let mut execution = Execution::prepare(
        cache.prepare(&canvas),
        &mut batch,
        super::FrameResources {
            filter_scenes: filter_scenes.frame(),
            images: &images,
            text: None,
            retained: &mut retained,
        },
        FrameOptions::default(),
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
