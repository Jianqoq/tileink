use super::*;

#[test]
fn tile_bins_recycle_dirty_record_and_page_capacities() {
    let mut bins = TileDrawBins::default();
    bins.dirty_records.extend(0..64);
    bins.dirty_pages.extend(0..64);
    let (_, records, pages) = bins.take_dirty();
    let capacities = (records.capacity(), pages.capacity());

    bins.recycle_dirty(records, pages);

    assert_eq!(
        (bins.dirty_records.capacity(), bins.dirty_pages.capacity()),
        capacities
    );
}

#[test]
fn tile_bins_release_oversized_dirty_capacity() {
    let mut bins = TileDrawBins::default();
    bins.dirty_records.extend(0..=RETAINED_TILE_DIRTY_CAPACITY);
    bins.dirty_pages
        .extend((0..=RETAINED_TILE_DIRTY_CAPACITY).map(|page| page as u32));
    let (_, records, pages) = bins.take_dirty();

    bins.recycle_dirty(records, pages);

    assert_eq!(bins.dirty_records.capacity(), 0);
    assert_eq!(bins.dirty_pages.capacity(), 0);
}

#[test]
fn tile_bins_switch_from_transient_flat_upload_back_to_persistent_pages() {
    let mut transient = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    transient.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    transient.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    let transient_plan = transient.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &transient,
        None,
        &transient_plan,
        &mut bins,
        &mut cursors,
        false,
        GpuLengthOverrides::default(),
    );

    let mut persistent = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    persistent.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    persistent.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let persistent = materialize(persistent);
    let persistent_plan = persistent.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &persistent,
        None,
        &persistent_plan,
        &mut bins,
        &mut cursors,
        false,
        GpuLengthOverrides::default(),
    );

    assert_eq!(bins.tile_draws(0), [0]);
    assert_eq!(bins.tile_draws(1), [1]);
    assert_eq!(lengths.tile_draw_chunk_count, 2);
    assert_eq!(lengths.tile_draw_index_count, 2);
}

#[test]
fn retained_surface_resize_uses_dense_bins_then_restores_incremental_index() {
    use crate::canvas::SceneBufferChanges;

    let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut canvas = materialize(canvas);
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    canvas.buffer_changes = Some(SceneBufferChanges {
        surface_changed: true,
        ..Default::default()
    });

    GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &canvas,
        None,
        &plan,
        &mut bins,
        &mut cursors,
        true,
        GpuLengthOverrides::default(),
    );

    assert!(!bins.page_arena_valid);
    assert_eq!(bins.tile_draws(0), [0]);
    assert_eq!(bins.tile_draws(1), [1]);

    canvas.buffer_changes = Some(SceneBufferChanges {
        draws: std::iter::once(0..1).collect(),
        ..Default::default()
    });
    GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &canvas,
        None,
        &plan,
        &mut bins,
        &mut cursors,
        true,
        GpuLengthOverrides::default(),
    );

    assert!(bins.page_arena_valid);
    assert_eq!(bins.tile_draws(0), [0]);
    assert_eq!(bins.tile_draws(1), [1]);
}

#[test]
fn transient_bins_preserve_persistent_tile_allocations_for_recovery() {
    let mut canvas = Canvas::new(128, 64, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 128.0, 64.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut cursors);
    let pages = bins
        .tile_pages
        .iter()
        .map(Vec::capacity)
        .collect::<Vec<_>>();
    let refs = bins.tile_refs.iter().map(Vec::capacity).collect::<Vec<_>>();
    bins.reset_transient(
        &canvas.draw_records,
        &plan.draw_order,
        None,
        (canvas.width_in_tiles(), canvas.height_in_tiles()),
        &mut cursors,
        true,
    );
    assert_eq!(
        bins.tile_pages
            .iter()
            .map(Vec::capacity)
            .collect::<Vec<_>>(),
        pages
    );
    assert_eq!(
        bins.tile_refs.iter().map(Vec::capacity).collect::<Vec<_>>(),
        refs
    );
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut cursors);
    assert!(bins.page_arena_valid);
    assert_eq!(
        bins.tile_pages
            .iter()
            .map(Vec::capacity)
            .collect::<Vec<_>>(),
        pages
    );
    assert_eq!(
        bins.tile_refs.iter().map(Vec::capacity).collect::<Vec<_>>(),
        refs
    );
    for tile in 0..bins.records.len() {
        assert_eq!(bins.tile_draws(tile), [0]);
    }
}

#[test]
fn tile_page_arena_compacts_after_fragmentation_exceeds_threshold() {
    let mut initial = Canvas::new(crate::TILE_SIZE * 20, crate::TILE_SIZE, 1.0);
    let mut moved = Canvas::new(crate::TILE_SIZE * 20, crate::TILE_SIZE, 1.0);
    for tile in 0..20 {
        initial.push_rect(
            Rect::new(f64::from(tile * 16), 0.0, f64::from(tile * 16 + 16), 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        let x = if tile < 8 {
            1000.0
        } else {
            f64::from(tile * 16)
        };
        moved.push_rect(
            Rect::new(x, 0.0, x + 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    let plan = initial.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(&initial, &plan, &mut bins, &mut cursors);
    let _ = bins.take_dirty();

    assert!(bins.update_changed(
        &moved.draw_records,
        &plan.draw_order,
        None,
        (20, 1),
        std::slice::from_ref(&(0..8)),
    ));
    let (full, _, _) = bins.take_dirty();
    assert!(full);
    assert_eq!(bins.active_page_count(), 12);
    assert_eq!(bins.draw_indices.len(), 12 * TILE_DRAW_PAGE_WORDS);
    assert!(bins.free_pages.is_empty());
    for tile in 0..20 {
        let expected = if tile < 8 {
            Vec::new()
        } else {
            vec![tile as u32]
        };
        assert_eq!(bins.tile_draws(tile), expected);
    }
}
