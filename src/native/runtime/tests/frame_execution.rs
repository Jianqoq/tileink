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
#[should_panic(expected = "occupied native scratch slot")]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn extracting_released_scratch_cannot_silently_discard_a_cache_entry() {
    use crate::render::surfaces::SurfaceAdapter;
    let context = crate::native::runtime::gpu_support::context().unwrap();
    let mut batch = crate::native::runtime::gpu_support::batch(&context);
    let upload = Default::default();
    let images = Images::record(&mut batch, &upload).unwrap();
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
    )
    .unwrap();
    let slot = execution.acquire_scratch().unwrap();
    execution.release_scratch(slot);
    execution.take_scratch(slot);
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn frame_records_directly_into_supplied_target_without_full_frame_copy() -> Result<()> {
    use crate::native::runtime::compute::Command;
    let context = crate::native::runtime::gpu_support::context()?;
    let mut batch = crate::native::runtime::gpu_support::batch(&context);
    let texture = context.create_texture(3, 2)?;
    let target = batch.import_texture(&texture)?;
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

#[path = "frame_execution/filter_context.rs"]
mod filter_context;
