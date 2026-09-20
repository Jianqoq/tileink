use super::*;
use crate::{Radius, RetainedNodeId, canvas::SceneBufferChanges};
use peniko::{Color, kurbo::Rect};

#[test]
fn full_scene_upload_consumes_tile_dirty_journals_across_retained_updates() -> Result<()> {
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
        // Native recording copies a complete tile snapshot. No incremental
        // journal may remain, even if this batch is subsequently abandoned.
        let (full, records, pages) = cache.staging.tile_draw_bins.take_dirty();
        assert!(
            !full && records.is_empty() && pages.is_empty(),
            "frame {frame}: unconsumed upload journal"
        );
        cache.staging.tile_draw_bins.recycle_dirty(records, pages);
        batch.acceptance().set(frame % 3 != 0);
    }
    Ok(())
}
