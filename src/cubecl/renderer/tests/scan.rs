use super::*;

#[test]
fn scan_wgpu_emits_one_tile_vertical_line_when_enabled() {
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
    renderer.scan();

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
    assert_eq!(segment_bumps, vec![1]);
    assert_eq!(ranges_start, vec![0]);
    assert_eq!(ranges_end, vec![1]);
    assert!((p0x[0] - 4.0).abs() < 1e-3);
    assert!((p1x[0] - 4.0).abs() < 1e-3);
    assert!((p0y[0] - 0.0).abs() < 1e-6);
    assert!((p1y[0] - 16.0).abs() < 1e-6);
}
