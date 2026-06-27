use super::*;

#[test]
fn cumsum_wgpu_scans_backdrop_rows_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(48, 32);
    scene.push_path(
        Rect::new(0.0, 0.0, 48.0, 32.0).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let mut renderer = WgpuRenderer::new_default_device(48, 32, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    let client = renderer.client.clone();
    renderer
        .scan
        .backdrops
        .replace(&client, &[1, -1, 2, 3, 0, -2]);
    renderer.cumsum();

    assert_eq!(
        renderer.scan.backdrops.read(renderer.client()),
        vec![1, 0, 2, 3, 3, 1]
    );
}

#[test]
fn cumsum_wgpu_carries_across_chunks_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let row_tiles = CUMSUM_CHUNK_SIZE + 3;
    let mut scene = Scene::new(row_tiles * crate::TILE_SIZE, crate::TILE_SIZE * 2);
    scene.push_path(
        Rect::new(
            0.0,
            0.0,
            f64::from(row_tiles * crate::TILE_SIZE),
            f64::from(crate::TILE_SIZE * 2),
        )
        .to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let mut deltas = Vec::with_capacity((row_tiles * 2) as usize);
    let mut expected = Vec::with_capacity((row_tiles * 2) as usize);
    for row in 0..2 {
        let mut carry = 0;
        for x in 0..row_tiles {
            let value = if row == 0 {
                1
            } else if x % 2 == 0 {
                2
            } else {
                -1
            };
            carry += value;
            deltas.push(value);
            expected.push(carry);
        }
    }

    let mut renderer = WgpuRenderer::new_default_device(
        row_tiles * crate::TILE_SIZE,
        crate::TILE_SIZE * 2,
        Color::TRANSPARENT,
    );
    renderer.prepare_scene(&scene);
    let client = renderer.client.clone();
    renderer.scan.backdrops.replace(&client, &deltas);
    renderer.cumsum();

    assert_eq!(renderer.scan.backdrops.read(renderer.client()), expected);
}
