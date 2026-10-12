use super::*;

#[test]
fn tile_draw_bins_keep_each_tiles_draws_in_scene_order() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    canvas.push_rect(
        Rect::new(40.0, 0.0, 48.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );

    let bins = build_tile_draw_bins(&canvas);

    assert_eq!(bins.tile_draws(0), vec![0]);
    assert_eq!(bins.tile_draws(1), vec![0, 1]);
}

#[test]
fn tile_draw_bins_query_region_without_scanning_unrelated_tiles() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 4, crate::TILE_SIZE * 2, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 48.0, 16.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.push_rect(
        Rect::new(80.0, 0.0, 96.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let bins = build_tile_draw_bins(&canvas);

    assert_eq!(
        bins.draws_in_bounds(Bounds::new(32, 0, 64, 32), &plan.draw_order, None),
        vec![0]
    );
    assert!(
        bins.draws_in_bounds(Bounds::new(0, 16, 32, 32), &plan.draw_order, None)
            .is_empty()
    );
    // The index cannot rule out source draws beyond its canvas domain.
    assert_eq!(
        bins.draws_in_bounds(Bounds::new(0, 32, 32, 64), &plan.draw_order, None),
        *plan.draw_order
    );
}

#[test]
fn fused_lengths_reuse_the_same_tile_draw_bins() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    for _ in 0..=COARSE_CHUNK_SIZE {
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    canvas.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );

    let canvas = materialize(canvas);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &canvas,
        None,
        &plan,
        &mut bins,
        &mut cursors,
        false,
        GpuLengthOverrides::default(),
    );

    assert_eq!(lengths.tile_draw_index_count, bins.upload_index_count());
    assert_eq!(bins.upload_indices(), bins.draw_indices);
    // Complete flat capacity stays resident across later opacity changes.
    assert_eq!(lengths.tile_draw_chunk_count, 3);
    // All but the final opaque draw are removed before the coarse upload.
    assert_eq!(bins.upload_records()[0].end, 1);
    assert_eq!(bins.records[1].end, 1);
}

#[test]
fn tile_page_capacity_includes_fused_layer_stack_particles() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    canvas.push_opacity_layer(
        Rect::new(0.0, 0.0, 32.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
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
    canvas.pop_layer();
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    let lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        &canvas,
        None,
        &plan,
        &mut bins,
        &mut cursors,
        false,
        GpuLengthOverrides::default(),
    );

    // Two tiles each require begin-opacity, draw, end-opacity, and terminator particles.
    assert!(lengths.coarse_ptcl_capacity >= 8);
}

#[test]
fn tile_draw_chunk_count_sums_per_tile_draw_chunks() {
    let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
    for _ in 0..=COARSE_CHUNK_SIZE {
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    canvas.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );

    let lengths = GpuBufferLengths::from_scene(&canvas);

    assert_eq!(lengths.tile_draw_index_count, 3 * TILE_DRAW_PAGE_WORDS);
    assert_eq!(lengths.tile_draw_chunk_count, 3);
}
