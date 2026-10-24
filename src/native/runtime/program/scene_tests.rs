use super::*;
use crate::native::runtime::gpu_support;
use crate::{Radius, RetainedNodeId, RetainedParent, RetainedScene};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

#[test]
fn sparse_tile_bin_uploads_only_changed_records_and_exact_index_ranges() {
    let records: Vec<_> = (0..8)
        .map(|start| TileDrawRecord { start, end: 1 })
        .collect();
    let indices: Vec<_> = (0..16u32).collect();
    let updates = tile_bin_dirty_updates(&records, &indices, 64, 256, &[1, 2, 6], &[4..8, 14..15]);
    assert_eq!(updates.len(), 4);
    assert_eq!(updates[0].0, 64 + size_of::<TileDrawRecord>());
    assert_eq!(updates[0].1, bytemuck::cast_slice::<_, u8>(&records[1..3]));
    assert_eq!(updates[1].0, 64 + 6 * size_of::<TileDrawRecord>());
    assert_eq!(updates[2].0, 256 + 4 * 4);
    assert_eq!(updates[2].1, bytemuck::cast_slice::<_, u8>(&indices[4..8]));
    assert_eq!(updates[3].0, 256 + 14 * 4);
    assert_eq!(updates[3].1.len(), 4);
    assert_eq!(
        updates.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
        3 * size_of::<TileDrawRecord>() + 5 * 4
    );
    assert!(tile_bin_dirty_updates(&records, &indices, 64, 256, &[], &[]).is_empty());
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn scene_upload_consumes_tile_dirty_journals_across_retained_updates() -> Result<()> {
    let context = gpu_support::context()?;
    let mut cache = SceneCache::default();
    let root = RetainedNodeId::for_owner(71_001);
    let child = RetainedNodeId::for_owner(71_002);
    let mut source = Canvas::new(64, 16, 1.0);
    source.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::BLACK);
    let mut scene = RetainedScene::new(64, 16, 1.0, root)?;
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            Rc::new(source),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut materializer = crate::retained_scene::PersistentSceneMaterializer::new(&scene);
    for frame in 0..256 {
        let x = f64::from(frame % 2 * 16);
        if frame != 0 {
            scene
                .transaction()
                .set_transform(child, Affine::translate((x, 0.0)))
                .commit()?;
            materializer.update(&scene, scene.changes_since(materializer.version()));
        }
        let canvas = materializer.canvas();
        let mut batch = gpu_support::batch(&context);
        cache.record(&mut batch, &canvas, None, None, 65_535)?;
        // A recorded update consumes its journal. An abandoned batch forces
        // complete GPU initialization on the next recording.
        let (full, records, pages) = Rc::make_mut(&mut cache.staging.tile_draw_bins).take_dirty();
        assert!(
            !full && records.is_empty() && pages.is_empty(),
            "frame {frame}: unconsumed upload journal"
        );
        Rc::make_mut(&mut cache.staging.tile_draw_bins).recycle_dirty(records, pages);
        if frame % 3 != 0 {
            context.submit_compute(&batch)?.wait()?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn filter_candidates_are_spatial_and_keep_recorded_scene_identity() -> Result<()> {
    let context = gpu_support::context()?;
    use crate::shared::bounds::Bounds;
    let mut cache = SceneCache::default();
    let mut canvas = Canvas::new(128, 16, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, Color::WHITE);
    canvas.push_rect(Rect::new(96.0, 0.0, 104.0, 8.0), Radius::ZERO, Color::BLACK);
    let first = cache.record(
        &mut gpu_support::batch(&context),
        &canvas,
        None,
        None,
        65535,
    )?;
    assert_eq!(
        first.filter_candidates(Bounds::new(0, 0, 16, 16), &canvas, first.plan()),
        [0]
    );
    assert!(
        first
            .filter_candidates(Bounds::new(48, 0, 64, 16), &canvas, first.plan())
            .is_empty()
    );
    // A nested/local scene and a later root may have different coordinates and
    // physical IDs. Keeping an old scene alive must not attach it to new bins.
    let mut other = Canvas::new(32, 16, 1.0);
    other.push_rect(Rect::new(16.0, 0.0, 24.0, 8.0), Radius::ZERO, Color::BLACK);
    let second = cache.record(&mut gpu_support::batch(&context), &other, None, None, 65535)?;
    assert!(
        second
            .filter_candidates(Bounds::new(0, 0, 16, 16), &other, second.plan())
            .is_empty()
    );
    assert_eq!(
        second.filter_candidates(Bounds::new(16, 0, 32, 16), &other, second.plan()),
        [0]
    );
    assert_eq!(
        first.filter_candidates(Bounds::new(96, 0, 112, 16), &canvas, first.plan()),
        [1]
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn filter_candidates_keep_offcanvas_sources() -> Result<()> {
    let context = gpu_support::context()?;
    use crate::shared::bounds::Bounds;
    let mut canvas = Canvas::new(32, 16, 1.0);
    canvas.push_rect(Rect::new(-16.0, 0.0, -8.0, 8.0), Radius::ZERO, Color::WHITE);
    let scene = SceneCache::default().record(
        &mut gpu_support::batch(&context),
        &canvas,
        None,
        None,
        65535,
    )?;
    // Offset(+16, 0) can bring these source pixels into the visible output.
    assert_eq!(
        scene.filter_candidates(Bounds::new(-16, 0, 16, 16), &canvas, scene.plan()),
        [0]
    );
    Ok(())
}
