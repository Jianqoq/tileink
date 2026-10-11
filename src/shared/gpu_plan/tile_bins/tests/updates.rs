use super::*;

#[test]
fn incremental_tile_pages_rewrite_only_old_and_new_bounds_tiles() {
    let mut initial = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
    initial.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    initial.push_rect(
        Rect::new(32.0, 0.0, 48.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let plan = initial.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(&initial, &plan, &mut bins, &mut cursors);
    let _ = bins.take_dirty();
    let unaffected_page = bins.records[2].start;
    let unaffected_words = bins.draw_indices[unaffected_page as usize * TILE_DRAW_PAGE_WORDS
        ..(unaffected_page as usize + 1) * TILE_DRAW_PAGE_WORDS]
        .to_vec();

    let mut moved = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
    moved.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    moved.push_rect(
        Rect::new(32.0, 0.0, 48.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    assert!(bins.update_changed(
        &moved.draw_records,
        &plan.draw_order,
        None,
        (3, 1),
        std::slice::from_ref(&(0..1)),
    ));
    let (_, dirty_records, dirty_pages) = bins.take_dirty();

    assert_eq!(bins.tile_draws(0), Vec::<u32>::new());
    assert_eq!(bins.tile_draws(1), vec![0]);
    assert_eq!(bins.tile_draws(2), vec![1]);
    assert_eq!(dirty_records, vec![0, 1]);
    assert!(!dirty_pages.contains(&unaffected_page));
    assert_eq!(
        &bins.draw_indices[unaffected_page as usize * TILE_DRAW_PAGE_WORDS
            ..(unaffected_page as usize + 1) * TILE_DRAW_PAGE_WORDS],
        unaffected_words
    );
}

#[test]
fn overlapping_changed_ranges_update_each_draw_and_tile_once() {
    let mut initial = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
    initial.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    initial.push_rect(
        Rect::new(32.0, 0.0, 48.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let plan = initial.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(&initial, &plan, &mut bins, &mut cursors);
    let _ = bins.take_dirty();

    let mut moved = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
    moved.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    moved.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let overlapping = [0..2, 0..1, 1..2, 0..2];
    assert!(bins.update_changed(
        &moved.draw_records,
        &plan.draw_order,
        None,
        (3, 1),
        &overlapping,
    ));
    assert_eq!(bins.tile_draws(0), [1]);
    assert_eq!(bins.tile_draws(1), [0]);
    assert!(bins.tile_draws(2).is_empty());

    assert!(bins.update_changed(
        &initial.draw_records,
        &plan.draw_order,
        None,
        (3, 1),
        &overlapping,
    ));
    assert_eq!(bins.tile_draws(0), [0]);
    assert!(bins.tile_draws(1).is_empty());
    assert_eq!(bins.tile_draws(2), [1]);
}

#[test]
fn dense_retained_spatial_change_stays_flat_only_while_geometry_keeps_moving() {
    let mut initial = Canvas::new(crate::TILE_SIZE * 5, crate::TILE_SIZE, 1.0);
    for tile in 0..4 {
        initial.push_rect(
            Rect::new(
                f64::from(tile * crate::TILE_SIZE),
                0.0,
                f64::from((tile + 1) * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE),
            ),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    initial.persistent_root = Some(RetainedNodeId::for_owner(71_000));
    let initial_plan = initial.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(&initial, &initial_plan, &mut bins, &mut cursors);
    let _ = bins.take_dirty();

    let mut moved = Canvas::new(crate::TILE_SIZE * 5, crate::TILE_SIZE, 1.0);
    for tile in 1..5 {
        moved.push_rect(
            Rect::new(
                f64::from(tile * crate::TILE_SIZE),
                0.0,
                f64::from((tile + 1) * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE),
            ),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    moved.persistent_root = initial.persistent_root;
    moved.buffer_changes = Some(SceneBufferChanges {
        draws: std::iter::once(0..moved.draw_records.len()).collect(),
        ..Default::default()
    });
    let moved_plan = moved.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &moved,
        None,
        &moved_plan,
        &mut bins,
        &mut cursors,
        true,
        GpuLengthOverrides::default(),
    );

    assert!(!bins.page_arena_valid);
    assert_eq!(lengths.tile_draw_index_count, 4);
    assert!(bins.take_dirty().0);
    assert_eq!(
        crate::shared::gpu_plan::ActiveBatches::default().query_tiles(
            &bins,
            &[0, 1, 2, 3, 4],
            &[9; 4]
        ),
        vec![9]
    );

    let mut moved_again = Canvas::new(crate::TILE_SIZE * 5, crate::TILE_SIZE, 1.0);
    for tile in 0..4 {
        moved_again.push_rect(
            Rect::new(
                f64::from(tile * crate::TILE_SIZE),
                0.0,
                f64::from((tile + 1) * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE),
            ),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    moved_again.persistent_root = initial.persistent_root;
    moved_again.buffer_changes = Some(SceneBufferChanges {
        draws: std::iter::once(0..moved_again.draw_records.len()).collect(),
        ..Default::default()
    });
    let moved_again_plan = moved_again.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let second_lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &moved_again,
        None,
        &moved_again_plan,
        &mut bins,
        &mut cursors,
        true,
        GpuLengthOverrides::default(),
    );
    assert!(!bins.page_arena_valid);
    assert_eq!(second_lengths.tile_draw_index_count, 4);
    assert!(bins.take_dirty().0);

    // When geometry stops moving (or only paint changes), rebuild the persistent reverse
    // index once. Staying transient forever would turn all later retained edits into full
    // tile-bin uploads and also prevents resize recovery.
    GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &moved_again,
        None,
        &moved_again_plan,
        &mut bins,
        &mut cursors,
        true,
        GpuLengthOverrides::default(),
    );
    assert!(bins.page_arena_valid);
}

#[test]
fn local_motion_does_not_trigger_whole_scene_bin_rebuild() {
    let mut canvas = Canvas::new(320, 320, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 320.0, 320.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 80.0, 80.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut Vec::new());
    canvas.draw_records[1].pixel_bounds.x0 += 16;
    canvas.draw_records[1].pixel_bounds.x1 += 16;
    assert!(!bins.spatial_change_is_dense(&canvas.draw_records, (20, 20), &[1..2]));
}

#[test]
fn dirty_draw_with_stable_bounds_and_rank_does_not_rewrite_tile_pages() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut cursors);
    let _ = bins.take_dirty();
    let compact_indices = bins.upload_index_count();

    assert!(bins.update_changed(
        &canvas.draw_records,
        &plan.draw_order,
        None,
        (2, 1),
        std::slice::from_ref(&(0..1)),
    ));
    let (full, dirty_records, dirty_pages) = bins.take_dirty();
    assert!(!full);
    assert!(dirty_records.is_empty());
    assert!(dirty_pages.is_empty());
    assert_eq!(bins.upload_index_count(), compact_indices);
    assert!(bins.upload_indices().len() < bins.draw_indices.len());
}
