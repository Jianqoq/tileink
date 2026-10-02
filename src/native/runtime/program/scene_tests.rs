use super::*;
use crate::{Radius, RetainedNodeId, canvas::SceneBufferChanges};
use peniko::{Color, kurbo::Rect};

#[test]
fn sparse_tile_bin_uploads_only_changed_records_and_pages() {
    use crate::shared::gpu_plan::TILE_DRAW_PAGE_WORDS;
    let records: Vec<_> = (0..8)
        .map(|start| TileDrawRecord { start, end: 1 })
        .collect();
    let indices: Vec<_> = (0..8 * TILE_DRAW_PAGE_WORDS)
        .map(|word| word as u32)
        .collect();
    let updates = tile_bin_dirty_updates(&records, &indices, 64, 256, &[1, 2, 6], &[2, 3, 7]);
    assert_eq!(updates.len(), 4);
    assert_eq!(updates[0].0, 64 + size_of::<TileDrawRecord>());
    assert_eq!(updates[0].1.len(), 2 * size_of::<TileDrawRecord>());
    assert_eq!(updates[1].0, 64 + 6 * size_of::<TileDrawRecord>());
    assert_eq!(updates[2].0, 256 + 2 * TILE_DRAW_PAGE_WORDS * 4);
    assert_eq!(updates[2].1.len(), 2 * TILE_DRAW_PAGE_WORDS * 4);
    assert_eq!(updates[3].0, 256 + 7 * TILE_DRAW_PAGE_WORDS * 4);
    assert_eq!(
        updates[0].1,
        bytemuck::cast_slice::<TileDrawRecord, u8>(&records[1..3])
    );
    assert_eq!(
        updates[2].1,
        bytemuck::cast_slice::<u32, u8>(
            &indices[2 * TILE_DRAW_PAGE_WORDS..4 * TILE_DRAW_PAGE_WORDS]
        )
    );
    assert_eq!(
        updates.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
        3 * size_of::<TileDrawRecord>() + 3 * TILE_DRAW_PAGE_WORDS * 4
    );
    assert!(tile_bin_dirty_updates(&records, &indices, 64, 256, &[], &[]).is_empty());
}

#[test]
fn unavailable_native_storage_uses_complete_snapshot() -> Result<()> {
    let mut cache = super::super::cached_buffer::CachedBuffer::default();
    let mut batch = ComputeBatch::new();
    let id = cache.patches_delta(&mut batch, 16, &[(4, &[1, 2, 3, 4])], &[])?;
    assert_eq!(
        batch.resources()[id.index()].bytes(),
        &[0, 0, 0, 0, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    Ok(())
}

#[test]
fn scene_upload_consumes_tile_dirty_journals_across_retained_updates() -> Result<()> {
    let mut cache = SceneCache::default();
    for frame in 0..256 {
        let mut canvas = Canvas::new(64, 16, 1.0);
        let x = f64::from(frame % 2 * 16);
        canvas.push_rect(
            Rect::new(x, 0.0, x + 16.0, 16.0),
            Radius::ZERO,
            Color::BLACK,
        );
        canvas.persistent_root = Some(RetainedNodeId::for_owner(71_001));
        if frame != 0 {
            canvas.buffer_changes = Some(SceneBufferChanges {
                draws: std::iter::once(0..1).collect(),
                ..Default::default()
            });
        }
        let mut batch = ComputeBatch::new();
        cache.record(&mut batch, &canvas, None, None, 65_535)?;
        // A recorded update consumes its journal. An abandoned batch forces
        // complete GPU initialization on the next recording.
        let (full, records, pages) = Rc::make_mut(&mut cache.staging.tile_draw_bins).take_dirty();
        assert!(
            !full && records.is_empty() && pages.is_empty(),
            "frame {frame}: unconsumed upload journal"
        );
        Rc::make_mut(&mut cache.staging.tile_draw_bins).recycle_dirty(records, pages);
        batch.acceptance().set(frame % 3 != 0);
    }
    Ok(())
}

#[test]
fn filter_candidates_are_spatial_and_keep_recorded_scene_identity() -> Result<()> {
    use crate::shared::bounds::Bounds;
    let mut cache = SceneCache::default();
    let mut canvas = Canvas::new(128, 16, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, Color::WHITE);
    canvas.push_rect(Rect::new(96.0, 0.0, 104.0, 8.0), Radius::ZERO, Color::BLACK);
    let first = cache.record(&mut ComputeBatch::new(), &canvas, None, None, 65535)?;
    assert_eq!(
        first.filter_candidates(Bounds::new(0, 0, 16, 16), first.plan()),
        [0]
    );
    assert!(
        first
            .filter_candidates(Bounds::new(48, 0, 64, 16), first.plan())
            .is_empty()
    );
    // A nested/local scene and a later root may have different coordinates and
    // physical IDs. Keeping an old scene alive must not attach it to new bins.
    let mut other = Canvas::new(32, 16, 1.0);
    other.push_rect(Rect::new(16.0, 0.0, 24.0, 8.0), Radius::ZERO, Color::BLACK);
    let second = cache.record(&mut ComputeBatch::new(), &other, None, None, 65535)?;
    assert!(
        second
            .filter_candidates(Bounds::new(0, 0, 16, 16), second.plan())
            .is_empty()
    );
    assert_eq!(
        second.filter_candidates(Bounds::new(16, 0, 32, 16), second.plan()),
        [0]
    );
    assert_eq!(
        first.filter_candidates(Bounds::new(96, 0, 112, 16), first.plan()),
        [1]
    );
    Ok(())
}

#[test]
fn filter_candidates_keep_offcanvas_sources() -> Result<()> {
    use crate::shared::bounds::Bounds;
    let mut canvas = Canvas::new(32, 16, 1.0);
    canvas.push_rect(Rect::new(-16.0, 0.0, -8.0, 8.0), Radius::ZERO, Color::WHITE);
    let scene =
        SceneCache::default().record(&mut ComputeBatch::new(), &canvas, None, None, 65535)?;
    // Offset(+16, 0) can bring these source pixels into the visible output.
    assert_eq!(
        scene.filter_candidates(Bounds::new(-16, 0, 16, 16), scene.plan()),
        [0]
    );
    Ok(())
}
