use super::super::SceneCache;
use super::*;
use crate::native::runtime::compute::ComputeBatch;
use peniko::kurbo::{Affine, Rect, Shape};

#[test]
fn opacity_only_stacks_need_no_clip_dispatch() -> super::super::Result<()> {
    let mut canvas = Canvas::new(64, 64, 1.0);
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    for _ in 0..128 {
        canvas.push_opacity_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1, 0.5);
        canvas.push_rect(rect, crate::Radius::ZERO, peniko::Color::BLACK);
        canvas.pop_layer();
    }
    let mut cache = SceneCache::default();
    let scene = cache.record(&mut ComputeBatch::new(), &canvas, None, None, 65535)?;
    assert_eq!(
        crate::shared::gpu_plan::plan_stack_depths(scene.plan()).0,
        0
    );
    assert!(scene.clip_dispatch.ranges.is_empty());
    assert!(scene.clip_dispatch.data.is_empty());
    assert!(!scene.clip_dispatch.preallocated);
    Ok(())
}

#[test]
#[cfg(any(feature = "dx12", feature = "vulkan"))]
fn broad_clip_only_schedules_tiles_with_child_draws() -> super::super::Result<()> {
    let mut canvas = Canvas::new(512, 512, 1.0);
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 512.0, 512.0), crate::Radius::ZERO);
    for rect in [
        Rect::new(17.0, 17.0, 31.0, 31.0),
        Rect::new(481.0, 481.0, 495.0, 495.0),
    ] {
        canvas.push_rect(rect, crate::Radius::ZERO, peniko::Color::BLACK);
    }
    canvas.pop_layer();
    let mut cache = SceneCache::default();
    let scene = cache.record(&mut ComputeBatch::new(), &canvas, None, None, 65535)?;
    let dispatch = ClipDispatch::new(
        &canvas,
        scene.plan(),
        None,
        cache.staging.tile_draw_bins.upload_records(),
        scene.lengths,
        1,
    )?;
    assert_eq!(dispatch.data, [33, 990]);
    let content = [(17, 17, 31, 31), (481, 481, 495, 495)];
    assert_eq!(
        tiles(
            &canvas,
            scene.plan(),
            0..1,
            Some(&[0, 1, 2, 3, 33, 990]),
            Some(&content)
        ),
        None
    );
    // Retained damage may expose old content outside the current
    // child draw bounds when a node is reparented into this clip.
    assert_eq!(
        tiles(&canvas, scene.plan(), 0..1, Some(&[0]), Some(&content)),
        Some(vec![0])
    );
    Ok(())
}

#[test]
fn small_clip_limits_dispatch_and_intersects_damage() {
    let mut canvas = Canvas::new(1280, 800, 1.0);
    canvas.push_clip_sdf_rect_layer(Rect::new(17.0, 17.0, 31.0, 31.0), crate::Radius::ZERO);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1280.0, 800.0),
        crate::Radius::ZERO,
        peniko::Color::BLACK,
    );
    canvas.pop_layer();
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let expected = 1280 / TILE_SIZE + 1;
    assert_eq!(
        tiles(&canvas, &plan, 0..1, None, None),
        Some(vec![expected])
    );
    assert_eq!(
        tiles(&canvas, &plan, 0..1, Some(&[0, expected]), None),
        Some(vec![expected])
    );
    assert_eq!(tiles(&canvas, &plan, 0..1, Some(&[0]), None), Some(vec![]));
    // A retained clip's batch can be empty while later draws still
    // consume its mask after a scene reparent.
    assert_eq!(
        tiles(&canvas, &plan, 0..1, None, Some(&[])),
        Some(vec![expected])
    );
    assert_eq!(tiles(&canvas, &plan, 0..0, None, None), None);
}

#[test]
fn broad_clip_keeps_dense_binning() {
    let mut canvas = Canvas::new(512, 512, 1.0);
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 384.0, 512.0), crate::Radius::ZERO);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 512.0, 512.0),
        crate::Radius::ZERO,
        peniko::Color::BLACK,
    );
    canvas.pop_layer();
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    assert_eq!(tiles(&canvas, &plan, 0..1, None, None), None);
}

#[test]
#[cfg(any(feature = "dx12", feature = "vulkan"))]
fn broad_pure_clip_uses_one_preallocated_dense_emit() -> super::super::Result<()> {
    let mut canvas = Canvas::new(512, 512, 1.0);
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 384.0, 512.0), crate::Radius::ZERO);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 512.0, 512.0),
        crate::Radius::ZERO,
        peniko::Color::BLACK,
    );
    canvas.pop_layer();
    let mut cache = SceneCache::default();
    let mut batch = ComputeBatch::new();
    let scene = cache.record(&mut batch, &canvas, None, None, 65535)?;
    assert!(scene.clip_dispatch.ranges.is_empty());
    assert!(scene.clip_dispatch.preallocated);
    let before = batch.passes().len();
    scene.encode_coarse(&mut batch, 0..1, 0..1, true, 65535)?;
    assert_eq!(batch.passes().len() - before, 1);
    assert_eq!(batch.passes()[before].shader.entry, "coarse_emit_bins");
    Ok(())
}

#[test]
#[cfg(any(feature = "dx12", feature = "vulkan"))]
fn fixed_clip_slots_are_disjoint_bounded_and_drop_out_for_mixed_passes() -> super::super::Result<()>
{
    let mut canvas = Canvas::new(64, 64, 1.0);
    for _ in 0..6 {
        canvas.push_clip_sdf_rect_layer(Rect::new(17.25, 17.25, 46.75, 46.75), crate::Radius::ZERO);
    }
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        peniko::Color::BLACK,
    );
    for _ in 0..6 {
        canvas.pop_layer();
    }
    let mut cache = SceneCache::default();
    let mut batch = ComputeBatch::new();
    let scene = cache.record(&mut batch, &canvas, None, None, 65535)?;
    assert!(scene.clip_dispatch.preallocated);
    let slots = ClipDispatch::new(
        &canvas,
        scene.plan(),
        None,
        cache.staging.tile_draw_bins.upload_records(),
        scene.lengths,
        6,
    )?;
    let mut end = 0;
    for slot in &slots.slots {
        assert_eq!(slot.ptcl_start, end);
        assert!(slot.ptcl_end > end);
        end = slot.ptcl_end;
    }
    assert!(end as usize <= scene.lengths.coarse_ptcl_capacity);
    let (&(start, stop), _) = scene.clip_dispatch.ranges.iter().next().unwrap();
    let before = batch.passes().len();
    scene.encode_coarse(&mut batch, 0..1, start..stop, true, 65535)?;
    assert_eq!(
        batch.passes().len() - before,
        1,
        "clip emission needs no GPU prefix allocation"
    );
    assert_eq!(
        batch.passes()[before].grid,
        [4, 1, 1],
        "small clip selections retain the parallel emitter"
    );
    canvas.push_opacity_layer(
        Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
        Affine::IDENTITY,
        0.1,
        0.5,
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        peniko::Color::BLACK,
    );
    canvas.pop_layer();
    let mixed = cache.record(&mut ComputeBatch::new(), &canvas, None, None, 65535)?;
    assert!(!mixed.clip_dispatch.preallocated);
    Ok(())
}

#[test]
fn nested_disjoint_clips_have_no_tiles() {
    let mut canvas = Canvas::new(64, 64, 1.0);
    for rect in [
        Rect::new(-4.0, -4.0, 12.0, 12.0),
        Rect::new(32.0, 32.0, 48.0, 48.0),
    ] {
        canvas.push_clip_sdf_rect_layer(rect, crate::Radius::ZERO);
    }
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        peniko::Color::BLACK,
    );
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    assert_eq!(tiles(&canvas, &plan, 0..2, None, None), Some(Vec::new()));
}

#[test]
#[cfg(any(feature = "dx12", feature = "vulkan"))]
fn clip_emit_dispatch_covers_both_sides_of_a_scalar_workgroup() -> super::super::Result<()> {
    // 255, 256 and 272 selected tiles: the last scalar group must cover its tail.
    for (width, height, groups) in [(240.0, 272.0, 255), (256.0, 256.0, 1), (272.0, 256.0, 2)] {
        let mut canvas = Canvas::new(512, 512, 1.0);
        canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, width, height), crate::Radius::ZERO);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 512.0, 512.0),
            crate::Radius::ZERO,
            peniko::Color::BLACK,
        );
        canvas.pop_layer();
        let mut cache = SceneCache::default();
        let mut batch = ComputeBatch::new();
        let scene = cache.record(&mut batch, &canvas, None, None, 65535)?;
        assert!(scene.clip_dispatch.preallocated);
        let before = batch.passes().len();
        scene.encode_coarse(&mut batch, 0..1, 0..1, true, 65535)?;
        assert_eq!(batch.passes().len() - before, 1);
        assert_eq!(batch.passes()[before].grid, [groups, 1, 1]);
    }
    Ok(())
}
