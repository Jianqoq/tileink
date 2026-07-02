use super::*;

#[test]
fn scan_wgpu_emits_closed_open_vertical_contour_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_path(
        peniko::kurbo::Line::new((4.0, 0.0), (4.0, 16.0)).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    run_scan_stage(&mut renderer, &scene);

    let ranges_start = renderer
        .scan
        .tile_segment_range_starts
        .read(renderer.client());
    let ranges_end = renderer
        .scan
        .tile_segment_range_ends
        .read(renderer.client());
    let segment_bumps = renderer.scan.segment_bumps.read(renderer.client());
    let backdrops = renderer.scan.backdrops.read(renderer.client());
    let p0x = renderer.scan.segment_p0x.read(renderer.client());
    let p0y = renderer.scan.segment_p0y.read(renderer.client());
    let p1x = renderer.scan.segment_p1x.read(renderer.client());
    let p1y = renderer.scan.segment_p1y.read(renderer.client());

    assert_eq!(backdrops, vec![0]);
    assert_eq!(segment_bumps, vec![2]);
    assert_eq!(ranges_start, vec![0]);
    assert_eq!(ranges_end, vec![2]);
    assert!((p0x[0] - 4.0).abs() < 1e-3);
    assert!((p1x[0] - 4.0).abs() < 1e-3);
    assert!((p0y[0] - 0.0).abs() < 1e-6);
    assert!((p1y[0] - 16.0).abs() < 1e-6);
    assert!((p0x[1] - 4.0).abs() < 1e-3);
    assert!((p1x[1] - 4.0).abs() < 1e-3);
    assert!((p0y[1] - 16.0).abs() < 1e-6);
    assert!((p1y[1] - 0.0).abs() < 1e-6);
}

#[test]
fn scan_wgpu_emits_closed_open_non_integer_horizontal_contour_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut path = BezPath::new();
    path.move_to((2.0, 8.5));
    path.line_to((30.0, 8.5));

    let mut scene = Scene::new(32, 16);
    scene.push_path(path, Color::BLACK, Affine::IDENTITY, FillRule::NonZero, 0.0);

    let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    run_scan_stage(&mut renderer, &scene);

    let counts = renderer.scan.segment_tile_counts.read(renderer.client());
    let starts = renderer
        .scan
        .tile_segment_range_starts
        .read(renderer.client());
    let ends = renderer
        .scan
        .tile_segment_range_ends
        .read(renderer.client());

    assert_eq!(counts, vec![2, 2]);
    assert_eq!(starts, vec![0, 2]);
    assert_eq!(ends, vec![2, 4]);
}

#[test]
fn scan_wgpu_emits_top_clipped_backdrop_bump_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let scene = top_clipped_rect_scene();
    let record = scene.bd_records[0];
    let stride = (record.tile_x1 - record.tile_x0) as usize;

    let mut renderer = WgpuRenderer::new_default_device(128, 48, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    run_scan_stage(&mut renderer, &scene);

    let backdrops = renderer.scan.backdrops.read(renderer.client());
    let first_row = &backdrops[0..stride];
    let second_row = &backdrops[stride..stride * 2];

    assert_eq!(first_row[0], 0);
    assert_eq!(first_row[1], second_row[1]);
    assert_eq!(first_row[1].abs(), 1);
}

#[test]
fn scan_wgpu_keeps_generated_horizontal_path_dash_backdrops_empty_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(1071, 651);
    scene.push_stroke(
        peniko::kurbo::Line::new((0.0, 216.5), (652.0, 216.5)).to_path(0.25),
        Stroke::new(1.0).with_dashes(0.0, [1.0_f64, 2.0_f64]),
        Color::BLACK,
        Affine::translate((387.0, 104.0)),
        FillRule::NonZero,
        0.25,
    );
    let record = scene.bd_records[0];
    let stride = (record.tile_x1 - record.tile_x0) as usize;

    let mut renderer = WgpuRenderer::new_default_device(1071, 651, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    run_scan_stage(&mut renderer, &scene);

    let backdrops = renderer.scan.backdrops.read(renderer.client());
    let first_row = &backdrops[0..stride];

    assert!(
        first_row.iter().all(|&backdrop| backdrop == 0),
        "expected dash cap top-edge bumps to cancel, got {first_row:?}"
    );
}
