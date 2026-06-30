use super::*;

#[test]
fn buffer_lengths_keep_empty_scene_allocations_zero_sized_except_target() {
    let scene = Scene::new(33, 17);
    let lengths = CubeBufferLengths::from_scene(&scene);

    assert_eq!(lengths.line_count, 0);
    assert_eq!(lengths.path_count, 0);
    assert_eq!(lengths.backdrop_len, 0);
    assert_eq!(lengths.segment_capacity, 0);
    assert_eq!(lengths.scan_chunk_count, 0);
    assert_eq!(lengths.cumsum_chunk_count, 0);
    assert_eq!(lengths.cumsum_row_count, 0);
    assert_eq!(lengths.coarse_chunk_count, 1);
    assert_eq!(lengths.coarse_ptcl_capacity, 6);
    assert_eq!(lengths.tiles_width, 3);
    assert_eq!(lengths.tiles_height, 2);
    assert_eq!(lengths.tile_count, 6);
    assert_eq!(lengths.image_pixels, 33 * 17);
}

#[test]
fn buffer_lengths_match_scene_preallocated_path_storage() {
    let mut scene = Scene::new(64, 48);
    scene.push_path(
        Rect::new(8.0, 8.0, 40.0, 32.0).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let lengths = CubeBufferLengths::from_scene(&scene);

    assert_eq!(lengths.line_count, scene.lines.len());
    assert_eq!(lengths.path_count, scene.path_records.len());
    assert_eq!(lengths.draw_count, scene.draw_records.len());
    assert_eq!(lengths.backdrop_record_count, scene.bd_records.len());
    assert_eq!(lengths.backdrop_len, scene.backdrop_pool_capacity as usize);
    assert_eq!(lengths.segment_capacity, scene.tile_cnt as usize);
    assert_eq!(lengths.scan_chunk_count, 1);
    assert_eq!(lengths.cumsum_chunk_count, 2);
    assert_eq!(lengths.cumsum_row_count, 2);
    assert_eq!(lengths.coarse_chunk_count, 1);
    assert_eq!(lengths.coarse_ptcl_capacity, 18);
    assert_eq!(lengths.tiles_width, 4);
    assert_eq!(lengths.tiles_height, 3);
    assert_eq!(lengths.tile_count, 4 * 3);
    assert_eq!(lengths.image_pixels, 64 * 48);
}

#[test]
fn buffer_lengths_count_sdf_draw_tiles_without_path_storage() {
    let mut scene = Scene::new(64, 48);
    scene.push_rect(
        Rect::new(8.0, 8.0, 40.0, 32.0),
        crate::Radius::ZERO,
        Color::BLACK,
        FillRule::NonZero,
    );
    let lengths = CubeBufferLengths::from_scene(&scene);

    assert_eq!(lengths.line_count, 0);
    assert_eq!(lengths.path_count, 0);
    assert_eq!(lengths.backdrop_record_count, 0);
    assert_eq!(lengths.backdrop_len, 0);
    assert_eq!(lengths.segment_capacity, 0);
    assert_eq!(lengths.scan_chunk_count, 0);
    assert_eq!(lengths.cumsum_chunk_count, 0);
    assert_eq!(lengths.cumsum_row_count, 0);
    assert_eq!(lengths.draw_count, 1);
    assert_eq!(lengths.coarse_ptcl_capacity, 18);
}

#[test]
fn buffer_lengths_count_sdf_clip_end_particles_without_path_storage() {
    let mut scene = Scene::new(64, 48);
    scene.push_clip_sdf_rect_layer(Rect::new(8.0, 8.0, 40.0, 32.0), crate::Radius::ZERO);
    scene.push_rect(
        Rect::new(8.0, 8.0, 40.0, 32.0),
        crate::Radius::ZERO,
        Color::BLACK,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let lengths = CubeBufferLengths::from_scene(&scene);
    assert_eq!(lengths.line_count, 0);
    assert_eq!(lengths.path_count, 0);
    assert_eq!(lengths.backdrop_record_count, 0);
    assert_eq!(lengths.segment_capacity, 0);
    assert_eq!(lengths.draw_count, 2);
    assert_eq!(lengths.coarse_ptcl_capacity, 30);
}
